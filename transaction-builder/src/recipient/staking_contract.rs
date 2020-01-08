use beserial::{Serialize, SerializingError, WriteBytesExt};
use bls::bls12_381::{CompressedSignature, KeyPair};
use keys::Address;
use transaction::account::staking_contract::{StakingTransactionData, StakingTransactionType};
use utils::key_rng::SecureGenerate;

use crate::recipient::Recipient;

pub enum StakingTransaction {
    Stake(StakingTransactionData),
    Retire,
    Unpark,
}

impl StakingTransaction {
    pub fn is_self_transaction(&self) -> bool {
        match self {
            StakingTransaction::Stake(_) => false,
            _ => true,
        }
    }
}

impl Serialize for StakingTransaction {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<usize, SerializingError> {
        match self {
            StakingTransaction::Stake(data) => data.serialize(writer),
            StakingTransaction::Retire => StakingTransactionType::Retire.serialize(writer),
            StakingTransaction::Unpark => StakingTransactionType::Unpark.serialize(writer),
        }
    }

    fn serialized_size(&self) -> usize {
        match self {
            StakingTransaction::Stake(data) => data.serialized_size(),
            StakingTransaction::Retire => StakingTransactionType::Retire.serialized_size(),
            StakingTransaction::Unpark => StakingTransactionType::Unpark.serialized_size(),
        }
    }
}

pub struct StakingRecipientBuilder {
    staking_contract: Address,
    staking_data: Option<StakingTransaction>,
}

impl StakingRecipientBuilder {
    pub fn new(staking_contract: Address) -> Self {
        StakingRecipientBuilder {
            staking_contract,
            staking_data: Default::default(),
        }
    }

    pub fn stake_with_new_bls_key(&mut self, reward_address: Option<Address>) -> KeyPair {
        let key = KeyPair::generate_default_csprng();
        self.stake_with_bls_key(&key, reward_address);
        key
    }

    pub fn stake_with_bls_key(&mut self, key_pair: &KeyPair, reward_address: Option<Address>) -> &mut Self {
        self.staking_data = Some(StakingTransaction::Stake(StakingTransactionData {
            validator_key: key_pair.public.compress(),
            reward_address,
            proof_of_knowledge: StakingRecipientBuilder::generate_proof_of_knowledge(&key_pair),
        }));
        self
    }

    pub fn retire(&mut self) -> &mut Self {
        self.staking_data = Some(StakingTransaction::Retire);
        self
    }

    pub fn unpark(&mut self) -> &mut Self {
        self.staking_data = Some(StakingTransaction::Unpark);
        self
    }

    pub fn generate_proof_of_knowledge(key_pair: &KeyPair) -> CompressedSignature {
        key_pair.sign(&key_pair.public).compress()
    }

    pub fn generate(self) -> Option<Recipient> {
        Some(Recipient::Staking {
            address: self.staking_contract,
            data: self.staking_data?,
        })
    }
}
