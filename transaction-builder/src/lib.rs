extern crate nimiq_bls as bls;
extern crate nimiq_hash as hash;
extern crate nimiq_keys as keys;
extern crate nimiq_primitives as primitives;
extern crate nimiq_transaction as transaction;
extern crate nimiq_utils as utils;

use failure::Fail;

use bls::bls12_381::KeyPair as BlsKeyPair;
use keys::{Address, KeyPair};
use primitives::account::AccountType;
use primitives::coin::Coin;
use primitives::networks::NetworkId;
use transaction::Transaction;

pub use crate::proof::TransactionProofBuilder;
pub use crate::recipient::Recipient;

pub mod recipient;
pub mod proof;

#[derive(Debug, Fail)]
pub enum TransactionBuilderError {
    #[fail(display = "The transaction sender address is missing.")]
    NoSender,
    #[fail(display = "The transaction recipient is missing.")]
    NoRecipient,
    #[fail(display = "The transaction value is missing.")]
    NoValue,
    #[fail(display = "The transaction's validity start height is missing.")]
    NoValidityStartHeight,
    #[fail(display = "The network id is missing.")]
    NoNetworkId,
    #[fail(display = "The sender is invalid for this recipient.")]
    InvalidSender,
}

#[derive(Default)]
pub struct TransactionBuilder {
    sender: Option<Address>,
    sender_type: Option<AccountType>,
    value: Option<Coin>,
    fee: Option<Coin>,
    recipient: Option<Recipient>,
    validity_start_height: Option<u32>,
    network_id: Option<NetworkId>,
}

// Basic builder functionality.
impl TransactionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value(&mut self, value: Coin) -> &mut Self {
        self.value = Some(value);
        self
    }

    pub fn with_fee(&mut self, fee: Coin) -> &mut Self {
        self.fee = Some(fee);
        self
    }

    pub fn with_sender(&mut self, sender: Address) -> &mut Self {
        self.sender = Some(sender);
        self
    }

    pub fn with_sender_type(&mut self, sender_type: AccountType) -> &mut Self {
        self.sender_type = Some(sender_type);
        self
    }

    pub fn with_recipient(&mut self, recipient: Recipient) -> &mut Self {
        self.recipient = Some(recipient);
        self
    }

    pub fn with_network_id(&mut self, network_id: NetworkId) -> &mut Self {
        self.network_id = Some(network_id);
        self
    }

    pub fn with_validity_start_height(&mut self, validity_start_height: u32) -> &mut Self {
        self.validity_start_height = Some(validity_start_height);
        self
    }

    pub fn generate(self) -> Result<TransactionProofBuilder, TransactionBuilderError> {
        let sender = self.sender.ok_or(TransactionBuilderError::NoSender)?;
        let recipient = self.recipient.ok_or(TransactionBuilderError::NoRecipient)?;

        if !recipient.is_valid_sender(&sender, self.sender_type) {
            return Err(TransactionBuilderError::InvalidSender);
        }

        let value = self.value.ok_or(TransactionBuilderError::NoValue)?;
        let validity_start_height = self.validity_start_height
            .ok_or(TransactionBuilderError::NoValidityStartHeight)?;
        let network_id = self.network_id.ok_or(TransactionBuilderError::NoNetworkId)?;

        let tx = if recipient.is_creation() {
            Transaction::new_contract_creation(
                recipient.data(),
                sender,
                self.sender_type.unwrap_or(AccountType::Basic),
                recipient.account_type(),
                value,
                self.fee.unwrap_or(Coin::ZERO),
                validity_start_height,
                network_id
            )
        } else {
            Transaction::new_extended(
                sender,
                self.sender_type.unwrap_or(AccountType::Basic),
                recipient.address().cloned().unwrap(), // For non-creation recipients, this should never return None.
                recipient.account_type(),
                value,
                self.fee.unwrap_or(Coin::ZERO),
                recipient.data(),
                validity_start_height,
                network_id
            )
        };

        Ok(TransactionProofBuilder::new(tx))
    }
}

// Convenience functionality.
impl TransactionBuilder {
    pub fn new_simple(key_pair: &KeyPair, recipient: Address, value: Coin, fee: Coin, validity_start_height: u32, network_id: NetworkId) -> Transaction {
        let sender = Address::from(key_pair);
        let mut builder = Self::new();
        builder.with_sender(sender)
            .with_recipient(Recipient::new_basic(recipient))
            .with_value(value)
            .with_fee(fee)
            .with_validity_start_height(validity_start_height)
            .with_network_id(network_id);

        let proof_builder = builder.generate().unwrap();
        match proof_builder {
            TransactionProofBuilder::Basic(mut builder) => {
                builder.sign_with_key_pair(&key_pair);
                builder.generate().unwrap()
            },
            _ => unreachable!(),
        }
    }

    pub fn new_staking(key_pair: &KeyPair, staking_contract: Address, validator_key: &BlsKeyPair, value: Coin, fee: Coin, reward_address: Option<Address>, validity_start_height: u32, network_id: NetworkId) -> Transaction {
        let mut recipient = Recipient::new_staking_builder(staking_contract.clone());
        recipient.stake_with_bls_key(validator_key, reward_address);

        let mut builder = Self::new();
        builder.with_sender(Address::from(key_pair))
            .with_recipient(recipient.generate().unwrap())
            .with_value(value)
            .with_fee(fee)
            .with_validity_start_height(validity_start_height)
            .with_network_id(network_id);

        let proof_builder = builder.generate().unwrap();
        match proof_builder {
            TransactionProofBuilder::Basic(mut builder) => {
                builder.sign_with_key_pair(&key_pair);
                builder.generate().unwrap()
            },
            _ => unreachable!(),
        }
    }

    pub fn new_retire(key_pair: &KeyPair, staking_contract: Address, value: Coin, fee: Coin, validity_start_height: u32, network_id: NetworkId) -> Transaction {
        let mut recipient = Recipient::new_staking_builder(staking_contract.clone());
        recipient.retire();

        let mut builder = Self::new();
        builder.with_sender(staking_contract)
            .with_sender_type(AccountType::Staking)
            .with_recipient(recipient.generate().unwrap())
            .with_value(value)
            .with_fee(fee)
            .with_validity_start_height(validity_start_height)
            .with_network_id(network_id);

        let proof_builder = builder.generate().unwrap();
        match proof_builder {
            TransactionProofBuilder::Staking(mut builder) => {
                builder.sign_with_key_pair(&key_pair);
                builder.generate().unwrap()
            },
            _ => unreachable!(),
        }
    }

    pub fn new_unpark(key_pair: &KeyPair, staking_contract: Address, active_stake: Coin, fee: Coin, validity_start_height: u32, network_id: NetworkId) -> Transaction {
        let mut recipient = Recipient::new_staking_builder(staking_contract.clone());
        recipient.unpark();

        let mut builder = Self::new();
        builder.with_sender(staking_contract)
            .with_sender_type(AccountType::Staking)
            .with_recipient(recipient.generate().unwrap())
            .with_value(active_stake - fee)
            .with_fee(fee)
            .with_validity_start_height(validity_start_height)
            .with_network_id(network_id);

        let proof_builder = builder.generate().unwrap();
        match proof_builder {
            TransactionProofBuilder::Staking(mut builder) => {
                builder.sign_with_key_pair(&key_pair);
                builder.generate().unwrap()
            },
            _ => unreachable!(),
        }
    }
}
