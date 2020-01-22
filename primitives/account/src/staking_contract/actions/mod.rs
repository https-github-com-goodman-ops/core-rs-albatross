use std::collections::HashSet;
use std::mem;

use beserial::Deserialize;
use transaction::account::staking_contract::{StakingTransactionData, StakingSelfTransactionType};

use crate::{Account, AccountError, AccountTransactionInteraction, AccountType, Inherent, InherentType, StakingContract};
use crate::inherent::AccountInherentInteraction;
use crate::staking_contract::SlashReceipt;

pub mod validator;
pub mod staker;

/// We need to distinguish three types of transactions:
/// TODO: Should invalid incoming transactions just be no-ops?
/// 1. Incoming transactions, which include:
///     - Validator
///         * Create
///         * Update
///         * Retire
///         * Re-activate
///         * Unpark
///     - Staker
///         * Stake
///     The type of transaction is given in the data field.
/// 2. Outgoing transactions, which include:
///     - Validator
///         * Drop
///     - Staker
///         * Unstake
///     The type of transaction is given in the proof field.
/// 3. Self transactions, which include:
///     - Staker
///         * Retire
///         * Re-activate
///     The type of transaction is given in the data field.
impl AccountTransactionInteraction for StakingContract {
    fn new_contract(_: AccountType, _: Coin, _: &Transaction, _: u32) -> Result<Self, AccountError> {
        Err(AccountError::InvalidForRecipient)
    }

    fn create(_: Coin, _: &Transaction, _: u32) -> Result<Self, AccountError> {
        Err(AccountError::InvalidForRecipient)
    }

    fn check_incoming_transaction(transaction: &Transaction, _: u32) -> Result<(), AccountError> {
        // Do all static checks here.
        if transaction.sender != transaction.recipient {
            // Stake transaction.
            StakingTransactionData::parse(transaction)?;
        } else {
            // For retire & unpark transactions, we need to check a valid flag in the data field.
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;

            if transaction.data.len() != ty.serialized_size() {
                return Err(AccountError::InvalidForTarget);
            }
        }
        Ok(())
    }

    fn commit_incoming_transaction(&mut self, transaction: &Transaction, block_height: u32) -> Result<Option<Vec<u8>>, AccountError> {
        if transaction.sender != transaction.recipient {
            // Stake transaction
            let data = StakingTransactionData::parse(transaction)?;
            Ok(self.stake(&transaction.sender, transaction.value, data.validator_key, data.reward_address)?
                .map(|receipt| receipt.serialize_to_vec()))
        } else {
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;
            // XXX Get staker address from transaction proof. This violates the model that only the
            // sender account should evaluate the proof. However, retire/unpark are self transactions, so
            // this contract is both sender and receiver.
            let staker_address = Self::get_signer(transaction)?;

            match ty {
                StakingSelfTransactionType::RetireStake => {
                    // Retire transaction.
                    Ok(self.retire_recipient(&staker_address, transaction.value, block_height)?
                        .map(|receipt| receipt.serialize_to_vec()))
                },
                StakingSelfTransactionType::Unpark => {
                    Ok(Some(self.unpark_recipient(&staker_address, transaction.value)?.serialize_to_vec()))
                },
            }
        }
    }

    fn revert_incoming_transaction(&mut self, transaction: &Transaction, _block_height: u32, receipt: Option<&Vec<u8>>) -> Result<(), AccountError> {
        if transaction.sender != transaction.recipient {
            // Stake transaction
            let receipt = match receipt {
                Some(v) => Some(Deserialize::deserialize_from_vec(v)?),
                _ => None
            };
            self.revert_stake(&transaction.sender, transaction.value, receipt)
        } else {
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;
            let staker_address = Self::get_signer(transaction)?;

            match ty {
                StakingSelfTransactionType::RetireStake => {
                    // Retire transaction.
                    let receipt = match receipt {
                        Some(v) => Some(Deserialize::deserialize_from_vec(v)?),
                        _ => None
                    };
                    self.revert_retire_recipient(&staker_address, transaction.value, receipt)
                },
                StakingSelfTransactionType::Unpark => {
                    let receipt = Deserialize::deserialize_from_vec(receipt.ok_or(AccountError::InvalidReceipt)?)?;
                    self.revert_unpark_recipient(&staker_address, transaction.value, receipt)
                },
            }
        }
    }

    fn check_outgoing_transaction(&self, transaction: &Transaction, block_height: u32) -> Result<(), AccountError> {
        let staker_address = Self::get_signer(transaction)?;
        if transaction.sender != transaction.recipient {
            // Unstake transaction
            let inactive_stake = self.inactive_stake_by_address.get(&staker_address)
                .ok_or(AccountError::InvalidForSender)?;

            // Check unstake delay.
            if block_height < policy::macro_block_after(inactive_stake.retire_time) + policy::UNSTAKING_DELAY {
                return Err(AccountError::InvalidForSender);
            }

            Account::balance_sufficient(inactive_stake.balance, transaction.total_value()?)
        } else {
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;

            let active_stake = self.active_stake_by_address.get(&staker_address)
                .ok_or(AccountError::InvalidForSender)?;

            match ty {
                StakingSelfTransactionType::RetireStake => {
                    // Retire transaction.
                    Account::balance_sufficient(active_stake.balance, transaction.total_value()?)
                },
                StakingSelfTransactionType::Unpark => {
                    if active_stake.balance != transaction.total_value()? {
                        return Err(AccountError::InvalidForSender);
                    }

                    if !self.current_epoch_parking.contains(&staker_address) && !self.previous_epoch_parking.contains(&staker_address) {
                        return Err(AccountError::InvalidForSender);
                    }
                    Ok(())
                },
            }
        }
    }

    fn commit_outgoing_transaction(&mut self, transaction: &Transaction, block_height: u32) -> Result<Option<Vec<u8>>, AccountError> {
        self.check_outgoing_transaction(transaction, block_height)?;

        let staker_address = Self::get_signer(transaction)?;
        if transaction.sender != transaction.recipient {
            // Unstake transaction
            Ok(self.unstake(&staker_address, transaction.total_value()?)?
                .map(|receipt| receipt.serialize_to_vec()))
        } else {
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;

            match ty {
                StakingSelfTransactionType::RetireStake => {
                    // Retire transaction.
                    Ok(self.retire_sender(&staker_address, transaction.total_value()?, block_height)?
                        .map(|receipt| receipt.serialize_to_vec()))
                },
                StakingSelfTransactionType::Unpark => {
                    self.unpark_sender(&staker_address, transaction.total_value()?, transaction.fee)?;
                    Ok(None)
                },
            }
        }
    }

    fn revert_outgoing_transaction(&mut self, transaction: &Transaction, _block_height: u32, receipt: Option<&Vec<u8>>) -> Result<(), AccountError> {
        let staker_address = Self::get_signer(transaction)?;

        if transaction.sender != transaction.recipient {
            // Unstake transaction
            let receipt = match receipt {
                Some(v) => Some(Deserialize::deserialize_from_vec(v)?),
                _ => None
            };
            self.revert_unstake(&staker_address, transaction.total_value()?, receipt)
        } else {
            let ty: StakingSelfTransactionType = Deserialize::deserialize(&mut &transaction.data[..])?;

            match ty {
                StakingSelfTransactionType::RetireStake => {
                    // Retire transaction.
                    let receipt = match receipt {
                        Some(v) => Some(Deserialize::deserialize_from_vec(v)?),
                        _ => None
                    };
                    self.revert_retire_sender(&staker_address, transaction.total_value()?, receipt)
                },
                StakingSelfTransactionType::Unpark => {
                    self.revert_unpark_sender(&staker_address, transaction.total_value()?, transaction.fee)
                },
            }
        }
    }
}

impl AccountInherentInteraction for StakingContract {
    fn check_inherent(&self, inherent: &Inherent, _block_height: u32) -> Result<(), AccountError> {
        trace!("check inherent: {:?}", inherent);
        // Inherent slashes nothing
        if inherent.value != Coin::ZERO {
            return Err(AccountError::InvalidInherent);
        }

        match inherent.ty {
            InherentType::Slash => {
                // Invalid data length
                if inherent.data.len() != Address::SIZE {
                    return Err(AccountError::InvalidInherent);
                }

                // Address doesn't exist in contract
                let staker_address: Address = Deserialize::deserialize(&mut &inherent.data[..])?;
                if !self.active_stake_by_address.contains_key(&staker_address) && !self.inactive_stake_by_address.contains_key(&staker_address) {
                    return Err(AccountError::InvalidInherent);
                }

                Ok(())
            },
            InherentType::FinalizeEpoch => {
                // Invalid data length
                if !inherent.data.is_empty() {
                    return Err(AccountError::InvalidInherent);
                }

                Ok(())
            },
            InherentType::Reward => Err(AccountError::InvalidForTarget)
        }
    }

    fn commit_inherent(&mut self, inherent: &Inherent, block_height: u32) -> Result<Option<Vec<u8>>, AccountError> {
        self.check_inherent(inherent, block_height)?;

        match &inherent.ty {
            InherentType::Slash => {
                // Simply add staker address to parking.
                let staker_address: Address = Deserialize::deserialize(&mut &inherent.data[..])?;
                // TODO: The inherent might have originated from a fork proof for the previous epoch.
                // Right now, we don't care and start the parking period in the epoch the proof has been submitted.
                let newly_slashed = self.current_epoch_parking.insert(staker_address);
                let receipt = SlashReceipt { newly_slashed };
                Ok(Some(receipt.serialize_to_vec()))
            },
            InherentType::FinalizeEpoch => {
                // Swap lists around.
                let current_epoch = mem::replace(&mut self.current_epoch_parking, HashSet::new());
                let old_epoch = mem::replace(&mut self.previous_epoch_parking, current_epoch);

                // Remove all parked stakers.
                for address in old_epoch {
                    let balance = self.get_active_balance(&address);
                    // We do not remove stakers from the parking list if they send a retire transaction.
                    // Instead, we simply skip these here.
                    // This saves space in the receipts of retire transactions as they happen much more often
                    // than stakers are added to the parking lists.
                    if balance > Coin::ZERO {
                        self.retire_sender(&address, balance, block_height)?;
                        self.retire_recipient(&address, balance, block_height)?;
                    }
                }

                // Since finalized epochs cannot be reverted, we don't need any receipts.
                Ok(None)
            },
            _ => unreachable!(),
        }
    }

    fn revert_inherent(&mut self, inherent: &Inherent, _block_height: u32, receipt: Option<&Vec<u8>>) -> Result<(), AccountError> {
        match &inherent.ty {
            InherentType::Slash => {
                let receipt: SlashReceipt = Deserialize::deserialize_from_vec(&receipt.ok_or(AccountError::InvalidReceipt)?)?;
                let staker_address: Address = Deserialize::deserialize(&mut &inherent.data[..])?;

                // Only remove if it was not already slashed.
                // I kept this in two nested if's for clarity.
                if receipt.newly_slashed {
                    let has_been_removed = self.current_epoch_parking.remove(&staker_address);
                    if !has_been_removed {
                        return Err(AccountError::InvalidInherent);
                    }
                }
            },
            InherentType::FinalizeEpoch => {
                // We should not be able to revert finalized epochs!
                return Err(AccountError::InvalidForTarget);
            },
            _ => unreachable!(),
        }

        Ok(())
    }
}