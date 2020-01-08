use std::io;

use beserial::Serialize;
use hash::SerializeContent;
use keys::KeyPair;
use primitives::account::AccountType;
use transaction::{SignatureProof, Transaction};

use crate::proof::htlc_contract::HtlcProofBuilder;

pub mod htlc_contract;

pub enum TransactionProofBuilder {
    Basic(BasicProofBuilder),
    Vesting(BasicProofBuilder),
    Htlc(HtlcProofBuilder),
    Staking(BasicProofBuilder),
}

impl TransactionProofBuilder {
    pub fn new(transaction: Transaction) -> Self {
        match transaction.sender_type {
            AccountType::Basic => TransactionProofBuilder::Basic(
                BasicProofBuilder::new(transaction)
            ),
            AccountType::Vesting => TransactionProofBuilder::Vesting(
                BasicProofBuilder::new(transaction)
            ),
            AccountType::HTLC => TransactionProofBuilder::Htlc(
                HtlcProofBuilder::new(transaction)
            ),
            AccountType::Staking => TransactionProofBuilder::Staking(
                BasicProofBuilder::new(transaction)
            ),
        }
    }

    pub fn unwrap_basic(self) -> BasicProofBuilder {
        match self {
            TransactionProofBuilder::Basic(builder) => builder,
            TransactionProofBuilder::Vesting(builder) => builder,
            TransactionProofBuilder::Staking(builder) => builder,
            _ => panic!("TransactionProofBuilder was not a BasicProofBuilder"),
        }
    }

    pub fn unwrap_htlc(self) -> HtlcProofBuilder {
        match self {
            TransactionProofBuilder::Htlc(builder) => builder,
            _ => panic!("TransactionProofBuilder was not a HtlcProofBuilder"),
        }
    }
}

impl SerializeContent for TransactionProofBuilder {
    fn serialize_content<W: io::Write>(&self, writer: &mut W) -> Result<usize, io::Error> {
        match self {
            TransactionProofBuilder::Basic(builder) => SerializeContent::serialize_content(&builder.transaction, writer),
            TransactionProofBuilder::Vesting(builder) => SerializeContent::serialize_content(&builder.transaction, writer),
            TransactionProofBuilder::Htlc(builder) => SerializeContent::serialize_content(&builder.transaction, writer),
            TransactionProofBuilder::Staking(builder) => SerializeContent::serialize_content(&builder.transaction, writer),
        }
    }
}

pub struct BasicProofBuilder {
    pub transaction: Transaction,
    signature: Option<SignatureProof>,
}

impl BasicProofBuilder {
    pub fn new(transaction: Transaction) -> Self {
        BasicProofBuilder {
            transaction,
            signature: None,
        }
    }

    pub fn with_signature_proof(&mut self, signature: SignatureProof) -> &mut Self {
        self.signature = Some(signature);
        self
    }

    pub fn sign_with_key_pair(&mut self, key_pair: &KeyPair) -> &mut Self {
        let signature = key_pair.sign(self.transaction.serialize_content().as_slice());
        self.signature = Some(SignatureProof::from(key_pair.public, signature));
        self
    }

    pub fn generate(self) -> Option<Transaction> {
        let mut tx = self.transaction;
        tx.proof = self.signature?.serialize_to_vec();
        Some(tx)
    }
}
