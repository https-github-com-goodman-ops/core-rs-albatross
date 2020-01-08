use beserial::{Serialize, SerializingError, WriteBytesExt};
use hash::{Blake2bHash, Sha256Hash};
use keys::KeyPair;
use transaction::{SignatureProof, Transaction};
use transaction::account::htlc_contract::{AnyHash, HashAlgorithm, ProofType};

pub enum HtlcProof {
    RegularTransfer {
        hash_algorithm: HashAlgorithm,
        hash_depth: u8,
        hash_root: AnyHash,
        pre_image: AnyHash,
        recipient_signature: SignatureProof,
    },
    EarlyResolve {
        recipient_signature: SignatureProof,
        sender_signature: SignatureProof,
    },
    TimeoutResolve {
        signature: SignatureProof,
    },
}

impl Serialize for HtlcProof {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<usize, SerializingError> {
        let mut size = 0;
        match self {
            HtlcProof::RegularTransfer { hash_algorithm, hash_depth, hash_root, pre_image, recipient_signature } => {
                size += ProofType::RegularTransfer.serialize(writer)?;
                size += hash_algorithm.serialize(writer)?;
                size += hash_depth.serialize(writer)?;
                size += hash_root.serialize(writer)?;
                size += pre_image.serialize(writer)?;
                size += recipient_signature.serialize(writer)?;
            },
            HtlcProof::EarlyResolve { recipient_signature, sender_signature } => {
                size += ProofType::EarlyResolve.serialize(writer)?;
                size += recipient_signature.serialize(writer)?;
                size += sender_signature.serialize(writer)?;
            },
            HtlcProof::TimeoutResolve { signature } => {
                size += ProofType::TimeoutResolve.serialize(writer)?;
                size += signature.serialize(writer)?;
            },
        }
        Ok(size)
    }

    fn serialized_size(&self) -> usize {
        let mut size = 0;
        match self {
            HtlcProof::RegularTransfer { hash_algorithm, hash_depth, hash_root, pre_image, recipient_signature } => {
                size += ProofType::RegularTransfer.serialized_size();
                size += hash_algorithm.serialized_size();
                size += hash_depth.serialized_size();
                size += hash_root.serialized_size();
                size += pre_image.serialized_size();
                size += recipient_signature.serialized_size();
            },
            HtlcProof::EarlyResolve { recipient_signature, sender_signature } => {
                size += ProofType::EarlyResolve.serialized_size();
                size += recipient_signature.serialized_size();
                size += sender_signature.serialized_size();
            },
            HtlcProof::TimeoutResolve { signature } => {
                size += ProofType::TimeoutResolve.serialized_size();
                size += signature.serialized_size();
            },
        }
        size
    }
}

pub struct HtlcProofBuilder {
    pub transaction: Transaction,
    proof: Option<HtlcProof>,
}

impl HtlcProofBuilder {
    pub fn new(transaction: Transaction) -> Self {
        HtlcProofBuilder {
            transaction,
            proof: None,
        }
    }

    pub fn signature_with_key_pair(&self, key_pair: &KeyPair) -> SignatureProof {
        let signature = key_pair.sign(self.transaction.serialize_content().as_slice());
        SignatureProof::from(key_pair.public, signature)
    }

    pub fn timeout_resolve(&mut self, sender_signature: SignatureProof) -> &mut Self {
        self.proof = Some(HtlcProof::TimeoutResolve {
            signature: sender_signature,
        });
        self
    }

    pub fn early_resolve(&mut self, sender_signature: SignatureProof, recipient_signature: SignatureProof) -> &mut Self {
        self.proof = Some(HtlcProof::EarlyResolve {
            sender_signature,
            recipient_signature,
        });
        self
    }

    pub fn regular_transfer(&mut self, hash_algorithm: HashAlgorithm, pre_image: AnyHash, hash_count: u8, hash_root: AnyHash, recipient_signature: SignatureProof) -> &mut Self {
        self.proof = Some(HtlcProof::RegularTransfer {
            hash_algorithm,
            hash_depth: hash_count,
            hash_root,
            pre_image,
            recipient_signature
        });
        self
    }

    pub fn regular_transfer_sha256(&mut self, pre_image: Sha256Hash, hash_count: u8, hash_root: Sha256Hash, recipient_signature: SignatureProof) -> &mut Self {
        let pre_image: [u8; 32] = pre_image.into();
        let hash_root: [u8; 32] = hash_root.into();
        self.regular_transfer(HashAlgorithm::Sha256, pre_image.into(), hash_count, hash_root.into(), recipient_signature)
    }

    pub fn regular_transfer_blake2b(&mut self, pre_image: Blake2bHash, hash_count: u8, hash_root: Blake2bHash, recipient_signature: SignatureProof) -> &mut Self {
        let pre_image: [u8; 32] = pre_image.into();
        let hash_root: [u8; 32] = hash_root.into();
        self.regular_transfer(HashAlgorithm::Blake2b, pre_image.into(), hash_count, hash_root.into(), recipient_signature)
    }

    pub fn generate(self) -> Option<Transaction> {
        let mut tx = self.transaction;
        tx.proof = self.proof?.serialize_to_vec();
        Some(tx)
    }
}
