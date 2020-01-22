use std::collections::BTreeMap;
use std::mem;
use std::ops::Add;
use std::sync::Arc;

use parking_lot::Mutex;

use beserial::{Deserialize, Serialize};
use bls::bls12_381::CompressedPublicKey as BlsPublicKey;
use keys::Address;
use primitives::coin::Coin;

use crate::{Account, AccountError, StakingContract};
use crate::staking_contract::{InactiveValidator, Validator};
use crate::staking_contract::actions::staker::InactiveStakeReceipt;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct UnparkReceipt {
    current_epoch: bool,
    previous_epoch: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct UpdateValidatorReceipt {
    old_reward_address: Address,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct DropValidatorReceipt {
    reward_address: Address,
    retirement_by_address: BTreeMap<Address, (Coin, Option<InactiveStakeReceipt>)>,
    retire_time: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct InactiveValidatorReceipt {
    retire_time: u32,
}

/// Actions concerning a validator are:
/// 1. Create: Creates a validator entry.
/// 2. Update: Updates reward address and key of the validator entry.
/// 3. Retire: Inactivates a validator entry (also starts a cooldown period used for Drop).
/// 4. Re-activate: Re-activates a validator entry.
/// 5. Drop: Drops a validator entry (validator must have been inactive for the cooldown period).
///          This also automatically retires the associated stake (allowing immediate withdrawal).
/// 6. Unpark: Prevents a validator entry from being automatically inactivated.
///
/// The actions can be summarized by the following state diagram:
///        +--------+   retire    +----------+
/// create |        +------------>+          | drop
///+------>+ active |             | inactive +------>
///        |        +<------------+          |
///        +-+--+---+ re-activate +-----+----+
///          |  ^                       ^
///          |  |                       |
/// mis-     |  | unpark                | automatically
/// behavior |  |                       |
///          |  |     +--------+        |
///          |  +-----+        |        |
///          |        | parked +--------+
///          +------->+        |
///                   +--------+
///
/// Create, Update, Retire, Re-activate, and Unpark are transactions from an arbitrary address
/// to the staking contract.
/// Drop is a transaction from the staking contract to an arbitrary address.
impl StakingContract {
    /// Creates a new validator entry.
    /// The initial stake can only be retrieved by dropping the validator again.
    pub(super) fn create_validator(&mut self, validator_key: BlsPublicKey, reward_address: Address, initial_stake: Coin) -> Result<(), AccountError> {
        if self.active_validators_by_key.contains_key(&validator_key)
            || self.inactive_validators_by_key.contains_key(&validator_key) {
            return Err(AccountError::InvalidForRecipient);
        }

        self.balance = Account::balance_add(self.balance, initial_stake)?;

        // All checks passed, not allowed to fail from here on!
        let validator = Arc::new(Mutex::new(Validator {
            balance: initial_stake,
            reward_address,
            validator_key,
            active_stake_by_address: Default::default(),
        }));

        self.active_validators_sorted.insert(Arc::clone(&validator));
        self.active_validators_by_key.insert(validator_key.clone(), validator);
        Ok(())
    }

    /// Reverts creating a new validator entry.
    pub(super) fn revert_create_validator(&mut self, validator_key: BlsPublicKey, reward_address: Address, initial_stake: Coin) -> Result<(), AccountError> {
        if let Some(validator) = self.active_validators_by_key.remove(&validator_key) {
            self.balance = Account::balance_sub(self.balance, initial_stake)?;

            // All checks passed, not allowed to fail from here on!
            self.active_validators_sorted.remove(&validator);
            Ok(())
        } else {
            Err(AccountError::InvalidForRecipient)
        }
    }

    /// Update validator details.
    /// This can be used to update active and inactive validators.
    pub(super) fn update_validator(&mut self, old_validator_key: BlsPublicKey, new_validator_key: BlsPublicKey, new_reward_address: Address) -> Result<UpdateValidatorReceipt, AccountError> {
        let validator = self.get_validator(&old_validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        // All checks passed, not allowed to fail from here on!
        let mut validator_locked = validator.lock();

        let old_reward_address = mem::replace(&mut validator_locked.reward_address, new_reward_address);
        validator_locked.validator_key = new_validator_key;

        Ok(UpdateValidatorReceipt {
            old_reward_address,
        })
    }

    /// Reverts updating validator key.
    pub(super) fn revert_update_validator(&mut self, old_validator_key: BlsPublicKey, new_validator_key: &BlsPublicKey, receipt: UpdateValidatorReceipt) -> Result<(), AccountError> {
        let validator = self.get_validator(new_validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        // All checks passed, not allowed to fail from here on!
        let mut validator_locked = validator.lock();

        validator_locked.reward_address = receipt.old_reward_address;
        validator_locked.validator_key = old_validator_key;

        Ok(())
    }

    /// Drops a validator entry.
    /// This can be used to drop inactive validators.
    /// The validator must have been inactive for at least one macro block.
    pub(super) fn drop_validator(&mut self, validator_key: &BlsPublicKey, initial_stake: Coin) -> Result<DropValidatorReceipt, AccountError> {
        // Remove validator from inactive list (retire time has been already checked).
        // `initial_stake` has been checked, too.
        let inactive_validator = self.inactive_validators_by_key.remove(validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        let mut validator_locked = inactive_validator.validator.lock();
        // We first remove all stakes the validator holds and will re-add stakes afterwards
        // when calling `retire_recipient`.
        self.balance = Account::balance_sub(self.balance, validator_locked.balance)?;

        // All checks passed, not allowed to fail from here on!
        // Retire all stakes.
        let mut retirement_by_address = BTreeMap::new();
        for (staker_address, &stake) in validator_locked.active_stake_by_address.iter() {
            let receipt = self.retire_recipient(staker_address, stake, inactive_validator.retire_time)?;
            retirement_by_address.insert(staker_address.clone(), (stake, receipt));
        }

        Ok(DropValidatorReceipt {
            reward_address: validator_locked.reward_address.clone(),
            retirement_by_address,
            retire_time: inactive_validator.retire_time,
        })
    }

    /// Revert dropping a validator entry.
    pub(super) fn revert_drop_validator(&mut self, validator_key: BlsPublicKey, mut total_value: Coin, receipt: DropValidatorReceipt) -> Result<(), AccountError> {
        // First, revert retiring the stakers.
        let mut active_stake_by_address = BTreeMap::new();
        for (staker_address, (stake, receipt)) in receipt.retirement_by_address {
            self.revert_retire_recipient(&staker_address, stake, receipt)?;
            active_stake_by_address.insert(staker_address, stake);
            total_value += stake;
        }

        self.balance = Account::balance_add(self.balance, total_value)?;

        self.inactive_validators_by_key.insert(validator_key.clone(), InactiveValidator {
            validator: Arc::new(Mutex::new(Validator {
                balance: total_value,
                reward_address: receipt.reward_address,
                validator_key,
                active_stake_by_address,
            })),
            retire_time: receipt.retire_time,
        });

        Ok(())
    }

    /// Inactivates a validator entry.
    pub(super) fn retire_validator(&mut self, validator_key: BlsPublicKey, block_height: u32) -> Result<(), AccountError> {
        // Move validator from active map/set to inactive map.
        let validator = self.active_validators_by_key.remove(&validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        // All checks passed, not allowed to fail from here on!
        self.active_validators_sorted.remove(&validator);
        self.inactive_validators_by_key.insert(validator_key, InactiveValidator {
            validator,
            retire_time: block_height,
        });
        Ok(())
    }

    /// Revert inactivating a validator entry.
    pub(super) fn revert_retire_validator(&mut self, validator_key: BlsPublicKey) -> Result<(), AccountError> {
        self.reactivate_validator(validator_key).map(|_| ())
    }

    /// Revert inactivating a validator entry.
    pub(super) fn reactivate_validator(&mut self, validator_key: BlsPublicKey) -> Result<InactiveValidatorReceipt, AccountError> {
        // Move validator from inactive map to active map/set.
        let inactive_validator = self.inactive_validators_by_key.remove(&validator_key)
            .ok_or(AccountError::InvalidForRecipient)?;

        // All checks passed, not allowed to fail from here on!
        self.active_validators_sorted.insert(Arc::clone(&inactive_validator.validator));
        self.active_validators_by_key.insert(validator_key.clone(), inactive_validator.validator);
        Ok(InactiveValidatorReceipt {
            retire_time: inactive_validator.retire_time,
        })
    }

    /// Inactivates a validator entry.
    pub(super) fn revert_reactivate_validator(&mut self, validator_key: BlsPublicKey, receipt: InactiveValidatorReceipt) -> Result<(), AccountError> {
        self.retire_validator(validator_key, receipt.retire_time)
    }

    /// Removes a validator from the parking lists.
    pub(super) fn unpark_validator(&mut self, validator_key: &BlsPublicKey, total_value: Coin, fee: Coin) -> Result<UnparkReceipt, AccountError> {
        let current_epoch = self.current_epoch_parking.remove(validator_key);
        let previous_epoch = self.previous_epoch_parking.remove(validator_key);

        if !current_epoch && !previous_epoch {
            return Err(AccountError::InvalidForRecipient);
        }

        Ok(UnparkReceipt {
            current_epoch,
            previous_epoch,
        })
    }

    /// Reverts an unparking transaction.
    pub(super) fn revert_unpark_validator(&mut self, validator_key: &BlsPublicKey, value: Coin, receipt: UnparkReceipt) -> Result<(), AccountError> {
        if receipt.current_epoch {
            self.current_epoch_parking.insert(validator_key.clone());
        }

        if receipt.previous_epoch {
            self.previous_epoch_parking.insert(validator_key.clone());
        }

        Ok(())
    }
}