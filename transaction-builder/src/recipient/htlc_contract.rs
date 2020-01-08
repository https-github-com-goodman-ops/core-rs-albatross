use hash::{Blake2bHash, Sha256Hash};
use keys::Address;
use transaction::account::htlc_contract::{AnyHash, HashAlgorithm};
use transaction::account::htlc_contract::CreationTransactionData as HtlcCreationData;

use crate::recipient::Recipient;

#[derive(Default)]
pub struct HtlcRecipientBuilder {
    contract_creation_data: HtlcCreationData,
}

impl HtlcRecipientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_single_sha256(sender: Address, recipient: Address, timeout: u32, hashed_secret: Sha256Hash) -> Self {
        let mut builder = Self::new();
        builder.with_sender(sender)
            .with_recipient(recipient)
            .with_sha256_hash(hashed_secret, 1)
            .with_timeout(timeout);
        builder
    }

    pub fn with_sender(&mut self, sender: Address) -> &mut Self {
        self.contract_creation_data.sender = sender;
        self
    }

    pub fn with_recipient(&mut self, recipient: Address) -> &mut Self {
        self.contract_creation_data.recipient = recipient;
        self
    }

    pub fn with_hash(&mut self, hash_root: AnyHash, hash_count: u8, hash_algorithm: HashAlgorithm) -> &mut Self {
        self.contract_creation_data.hash_root = hash_root;
        self.contract_creation_data.hash_count = hash_count;
        self.contract_creation_data.hash_algorithm = hash_algorithm;
        self
    }

    pub fn with_sha256_hash(&mut self, hash_root: Sha256Hash, hash_count: u8) -> &mut Self {
        let hash: [u8; 32] = hash_root.into();
        self.contract_creation_data.hash_root = AnyHash::from(hash);
        self.contract_creation_data.hash_count = hash_count;
        self.contract_creation_data.hash_algorithm = HashAlgorithm::Sha256;
        self
    }

    pub fn with_blake2b_hash(&mut self, hash_root: Blake2bHash, hash_count: u8) -> &mut Self {
        let hash: [u8; 32] = hash_root.into();
        self.contract_creation_data.hash_root = AnyHash::from(hash);
        self.contract_creation_data.hash_count = hash_count;
        self.contract_creation_data.hash_algorithm = HashAlgorithm::Blake2b;
        self
    }

    pub fn with_timeout(&mut self, timeout: u32) -> &mut Self {
        self.contract_creation_data.timeout = timeout;
        self
    }

    pub fn generate(self) -> Recipient {
        Recipient::HtlcCreation {
            data: self.into()
        }
    }
}

impl From<HtlcRecipientBuilder> for HtlcCreationData {
    fn from(builder: HtlcRecipientBuilder) -> Self {
        builder.contract_creation_data
    }
}

impl From<HtlcRecipientBuilder> for Recipient {
    fn from(builder: HtlcRecipientBuilder) -> Self {
        builder.generate()
    }
}
