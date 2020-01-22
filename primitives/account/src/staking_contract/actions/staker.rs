use std::sync::Arc;

use parking_lot::Mutex;

use beserial::{Deserialize, Serialize};
use bls::bls12_381::CompressedPublicKey as BlsPublicKey;
use keys::Address;
use primitives::coin::Coin;

use crate::{Account, AccountError, StakingContract};
use crate::staking_contract::{InactiveStake, Validator};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct InactiveStakeReceipt {
    retire_time: u32,
}

/// Actions concerning a staker are:
/// 1. Stake: Delegate stake from an outside address to a validator.
/// 2. Retire: Remove stake from a validator and make it inactive
///            (starting the cooldown period for Unstake).
/// 3. Re-activate: Re-delegate inactive stake to a validator.
/// 4. Unstake: Remove inactive stake from the staking contract
///             (after it has been inactive for the cooldown period).
///
/// The actions can be summarized by the following state diagram:
///        +--------+   retire    +----------+
/// stake  |        +------------>+          | unstake
///+------>+ staked |             | inactive +--------->
///        |        +<------------+          |
///        +--------+ re-activate +----------+
///
/// Stake is a transaction from an arbitrary address to the staking contract.
/// Retire and Re-activate are self transactions on the staking address.
/// Unstake is a transaction from the staking contract to an arbitrary address.
impl StakingContract {
    /// Adds funds to stake of `address` for validator `validator_key`.
    /// XXX This is public to fill the genesis staking contract
    pub fn stake(&mut self, staker_address: Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        let validator = self.get_validator(validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        self.balance = Account::balance_add(self.balance, value)?;

        // All checks passed, not allowed to fail from here on!
        let mut validator_locked = validator.lock();
        // We do not need to check for overflows here, because self.balance is always larger.
        validator_locked.balance += value;
        validator_locked.active_stake_by_address.entry(staker_address)
            .or_insert(Coin::ZERO) += value;

        Ok(())
    }

    /// Reverts a stake transaction.
    pub(super) fn revert_stake(&mut self, staker_address: &Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        let validator = self.get_validator(validator_key)
            .ok_or(AccountError::InvalidForSender)?;

        let mut validator_locked = validator.lock();
        if let Some(&stake) = validator_locked.active_stake_by_address.get(staker_address) {
            if value > stake {
                return Err(AccountError::InsufficientFunds {
                    needed: value,
                    balance: stake,
                });
            }
        } else {
            return Err(AccountError::InvalidForRecipient);
        }

        self.balance = Account::balance_sub(self.balance, value)?;

        // All checks passed, not allowed to fail from here on!
        let mut stake = validator_locked.active_stake_by_address
            .get_mut(staker_address)
            .unwrap();
        stake -= value;

        if stake.is_zero() {
            validator_locked.active_stake_by_address.remove(staker_address);
        }

        Ok(())
    }

    /// Removes stake from the active stake list.
    pub(super) fn retire_sender(&mut self, staker_address: &Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        self.revert_stake(staker_address, value, validator_key)
    }

    /// Reverts the sender side of a retire transaction.
    pub(super) fn revert_retire_sender(&mut self, staker_address: Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        self.reactivate_recipient(staker_address, value, validator_key)
    }

    /// Adds state to the inactive stake list.
    pub(super) fn retire_recipient(&mut self, staker_address: &Address, value: Coin, block_height: u32) -> Result<Option<InactiveStakeReceipt>, AccountError> {
        self.balance = Account::balance_add(self.balance, value)?;

        // All checks passed, not allowed to fail from here on!
        if let Some(inactive_stake) = self.inactive_stake_by_address.remove(staker_address) {
            let new_inactive_stake = InactiveStake {
                balance: Account::balance_add(inactive_stake.balance, value)?,
                retire_time: block_height,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);

            Ok(Some(InactiveStakeReceipt {
                retire_time: inactive_stake.retire_time,
            }))
        } else {
            let new_inactive_stake = InactiveStake {
                balance: value,
                retire_time: block_height,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);

            Ok(None)
        }
    }

    /// Reverts a retire transaction.
    pub(super) fn revert_retire_recipient(&mut self, staker_address: &Address, value: Coin, receipt: Option<InactiveStakeReceipt>) -> Result<(), AccountError> {
        let inactive_stake = self.inactive_stake_by_address.get(staker_address)
            .ok_or(AccountError::InvalidForRecipient)?;

        if (inactive_stake.balance > value) != receipt.is_some() {
            return Err(AccountError::InvalidForRecipient);
        }

        let block_height = receipt.map(|r| r.retire_time).unwrap_or_default();
        self.reactivate_sender(staker_address, value, block_height).map(|_| ())
    }

    /// Reverts a retire transaction.
    pub(super) fn reactivate_sender(&mut self, staker_address: &Address, value: Coin, block_height: u32) -> Result<InactiveStakeReceipt, AccountError> {
        let inactive_stake = self.inactive_stake_by_address.remove(staker_address)
            .ok_or(AccountError::InvalidForRecipient)?;

        self.balance = Account::balance_sub(self.balance, value)?;

        // All checks passed, not allowed to fail from here on!
        if inactive_stake.balance > value {
            let new_inactive_stake = InactiveStake {
                // Balance check is already done in `check` functions.
                balance: Account::balance_sub(inactive_stake.balance, value)?,
                retire_time: block_height,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);
        }
        Ok(InactiveStakeReceipt {
            retire_time: inactive_stake.retire_time
        })
    }

    /// Adds state to the inactive stake list.
    pub(super) fn revert_reactivate_sender(&mut self, staker_address: &Address, value: Coin, receipt: InactiveStakeReceipt) -> Result<(), AccountError> {
        self.retire_recipient(staker_address, value, receipt.retire_time).map(|_| ())
    }

    /// Reverts the sender side of a retire transaction.
    pub(super) fn reactivate_recipient(&mut self, staker_address: Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        self.stake(staker_address, value, validator_key)
    }

    /// Removes stake from the active stake list.
    pub(super) fn revert_reactivate_recipient(&mut self, staker_address: &Address, value: Coin, validator_key: &BlsPublicKey) -> Result<(), AccountError> {
        self.retire_sender(staker_address, value, validator_key)
    }

    /// Removes stake from the inactive stake list.
    pub(super) fn unstake(&mut self, staker_address: &Address, total_value: Coin) -> Result<Option<InactiveStakeReceipt>, AccountError> {
        let inactive_stake = self.inactive_stake_by_address.remove(staker_address)
            .ok_or(AccountError::InvalidForSender)?;

        self.balance = Account::balance_sub(self.balance, total_value)?;

        // All checks passed, not allowed to fail from here on!
        if inactive_stake.balance > total_value {
            let new_inactive_stake = InactiveStake {
                balance: Account::balance_sub(inactive_stake.balance, total_value)?,
                retire_time: inactive_stake.retire_time,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);

            Ok(None)
        } else {
            assert_eq!(inactive_stake.balance, total_value);
            Ok(Some(InactiveStakeReceipt {
                retire_time: inactive_stake.retire_time,
            }))
        }
    }

    /// Reverts a unstake transaction.
    pub(super) fn revert_unstake(&mut self, staker_address: &Address, total_value: Coin, receipt: Option<InactiveStakeReceipt>) -> Result<(), AccountError> {
        self.balance = Account::balance_add(self.balance, total_value)?;

        if let Some(inactive_stake) = self.inactive_stake_by_address.remove(staker_address) {
            if receipt.is_some() {
                return Err(AccountError::InvalidReceipt);
            }

            let new_inactive_stake = InactiveStake {
                balance: Account::balance_add(inactive_stake.balance, total_value)?,
                retire_time: inactive_stake.retire_time,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);
        } else {
            let receipt = receipt.ok_or(AccountError::InvalidReceipt)?;
            let new_inactive_stake = InactiveStake {
                balance: total_value,
                retire_time: receipt.retire_time,
            };
            self.inactive_stake_by_address.insert(staker_address.clone(), new_inactive_stake);
        }
        Ok(())
    }
}