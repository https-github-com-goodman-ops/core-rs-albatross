use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, BTreeMap};
use std::collections::btree_set::BTreeSet;
use std::mem;
use std::sync::Arc;

use beserial::{Deserialize, DeserializeWithLength, ReadBytesExt, Serialize, SerializeWithLength, SerializingError, WriteBytesExt};
use bls::bls12_381::CompressedPublicKey as BlsPublicKey;
use keys::Address;
use primitives::{policy, coin::Coin};
use primitives::slot::{Slots, SlotsBuilder};
use transaction::{SignatureProof, Transaction};
use transaction::account::staking_contract::{StakingTransactionData, StakingSelfTransactionType};
use vrf::{VrfSeed, VrfUseCase, AliasMethod};

use crate::{Account, AccountError, AccountTransactionInteraction, AccountType};
use crate::inherent::{AccountInherentInteraction, Inherent, InherentType};

pub mod actions;
pub mod validator;

pub use self::validator::Validator;
use parking_lot::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct InactiveStake {
    pub balance: Coin,
    pub retire_time: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct InactiveValidator {
    pub validator: Arc<Mutex<Validator>>,
    pub retire_time: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct SlashReceipt {
    newly_slashed: bool,
}

#[derive(Clone, Debug)]
pub struct StakingContract {
    pub balance: Coin,
    // Validators
    pub active_validators_sorted: BTreeSet<Arc<Mutex<Validator>>>,
    pub active_validators_by_key: HashMap<BlsPublicKey, Arc<Mutex<Validator>>>,
    pub inactive_validators_by_key: BTreeMap<BlsPublicKey, InactiveValidator>,
    pub current_epoch_parking: HashSet<BlsPublicKey>,
    pub previous_epoch_parking: HashSet<BlsPublicKey>,
    // Stake
    pub inactive_stake_by_address: HashMap<Address, InactiveStake>,
}

impl StakingContract {
    pub fn get_validator(&self, validator_key: &BlsPublicKey) -> Option<&Arc<Mutex<Validator>>> {
        self.active_validators_by_key.get(validator_key)
            .or_else(|| self.inactive_validators_by_key.get(validator_key)
                .map(|inactive_validator| &inactive_validator.validator))
    }

    pub fn get_balance(&self, staker_address: &Address) -> Coin {
        self.get_active_balance(staker_address) + self.get_inactive_balance(staker_address)
    }

    pub fn get_active_balance(&self, staker_address: &Address) -> Coin {
        self.active_stake_by_address.get(staker_address).map(|stake| stake.balance).unwrap_or(Coin::ZERO)
    }

    pub fn get_inactive_balance(&self, staker_address: &Address) -> Coin {
        self.inactive_stake_by_address.get(staker_address).map(|stake| stake.balance).unwrap_or(Coin::ZERO)
    }

    pub fn select_validators(&self, seed: &VrfSeed) -> Slots {
        // TODO: Depending on the circumstances and parameters, it might be more efficient to store active stake in an unsorted Vec.
        // Then, we would not need to create the Vec here. But then, removal of stake is a O(n) operation.
        // Assuming that validator selection happens less frequently than stake removal, the current implementation might be ok.
        let mut potential_validators = Vec::with_capacity(self.active_stake_sorted.len());
        let mut weights: Vec<u64> = Vec::with_capacity(self.active_stake_sorted.len());

        debug!("Select validators: num_slots = {}", policy::SLOTS);

        // NOTE: `active_stake_sorted` is sorted from highest to lowest stake. `LookupTable`
        // expects the reverse ordering.
        for validator in self.active_stake_sorted.iter() {
            potential_validators.push(Arc::clone(validator));
            weights.push(validator.balance.into());
        }

        let mut slots_builder = SlotsBuilder::default();
        let lookup = AliasMethod::new(weights);
        let mut rng = seed.rng(VrfUseCase::ValidatorSelection, 0);

        for _ in 0 .. policy::SLOTS {
            let index = lookup.sample(&mut rng);

            let active_stake = &potential_validators[index];

            slots_builder.push(
                active_stake.validator_key.clone(),
                active_stake.staker_address.clone(),
                active_stake.reward_address.clone()
            );
        }

        slots_builder.build()
    }

    fn get_signer(transaction: &Transaction) -> Result<Address, AccountError> {
        let signature_proof: SignatureProof = Deserialize::deserialize(&mut &transaction.proof[..])?;
        Ok(signature_proof.compute_signer())
    }
}

impl Serialize for StakingContract {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<usize, SerializingError> {
        let mut size = 0;
        size += Serialize::serialize(&self.balance, writer)?;

        size += Serialize::serialize(&(self.active_stake_sorted.len() as u32), writer)?;
        for active_stake in self.active_stake_sorted.iter() {
            let inactive_stake = self.inactive_stake_by_address.get(&active_stake.staker_address);
            size += Serialize::serialize(active_stake, writer)?;
            size += Serialize::serialize(&inactive_stake, writer)?;
        }

        // Collect remaining inactive stakes.
        let mut inactive_stakes = Vec::new();
        for (staker_address, inactive_stake) in self.inactive_stake_by_address.iter() {
            if !self.active_stake_by_address.contains_key(staker_address) {
                inactive_stakes.push((staker_address, inactive_stake));
            }
        }
        inactive_stakes.sort_by(|a, b|a.0.cmp(b.0)
            .then_with(|| a.1.balance.cmp(&b.1.balance))
            .then_with(|| a.1.retire_time.cmp(&b.1.retire_time)));

        size += Serialize::serialize(&(inactive_stakes.len() as u32), writer)?;
        for (staker_address, inactive_stake) in inactive_stakes {
            size += Serialize::serialize(staker_address, writer)?;
            size += Serialize::serialize(inactive_stake, writer)?;
        }

        size += SerializeWithLength::serialize::<u32, _>(&self.current_epoch_parking, writer)?;
        size += SerializeWithLength::serialize::<u32, _>(&self.previous_epoch_parking, writer)?;

        Ok(size)
    }

    fn serialized_size(&self) -> usize {
        let mut size = 0;
        size += Serialize::serialized_size(&self.balance);

        size += Serialize::serialized_size(&0u32);
        for active_stake in self.active_stake_sorted.iter() {
            let inactive_stake = self.inactive_stake_by_address.get(&active_stake.staker_address);
            size += Serialize::serialized_size(active_stake);
            size += Serialize::serialized_size(&inactive_stake);
        }

        size += Serialize::serialized_size(&0u32);
        for (staker_address, inactive_stake) in self.inactive_stake_by_address.iter() {
            if !self.active_stake_by_address.contains_key(staker_address) {
                size += Serialize::serialized_size(staker_address);
                size += Serialize::serialized_size(inactive_stake);
            }
        }

        size += SerializeWithLength::serialized_size::<u32>(&self.current_epoch_parking);
        size += SerializeWithLength::serialized_size::<u32>(&self.previous_epoch_parking);

        size
    }
}

impl Deserialize for StakingContract {
    fn deserialize<R: ReadBytesExt>(reader: &mut R) -> Result<Self, SerializingError> {
        let balance = Deserialize::deserialize(reader)?;

        let mut active_stake_sorted = BTreeSet::new();
        let mut active_stake_by_address = HashMap::new();
        let mut inactive_stake_by_address = HashMap::new();

        let num_active_stakes: u32 = Deserialize::deserialize(reader)?;
        for _ in 0..num_active_stakes {
            let active_stake: Arc<ActiveStake> = Deserialize::deserialize(reader)?;
            let inactive_stake: Option<InactiveStake> = Deserialize::deserialize(reader)?;

            active_stake_sorted.insert(Arc::clone(&active_stake));
            active_stake_by_address.insert(active_stake.staker_address.clone(), Arc::clone(&active_stake));

            if let Some(stake) = inactive_stake {
                inactive_stake_by_address.insert(active_stake.staker_address.clone(), stake);
            }
        }

        let num_inactive_stakes: u32 = Deserialize::deserialize(reader)?;
        for _ in 0..num_inactive_stakes {
            let staker_address = Deserialize::deserialize(reader)?;
            let inactive_stake = Deserialize::deserialize(reader)?;
            inactive_stake_by_address.insert(staker_address, inactive_stake);
        }

        let current_epoch_parking: HashSet<Address> = DeserializeWithLength::deserialize::<u32, _>(reader)?;
        let last_epoch_parking: HashSet<Address> = DeserializeWithLength::deserialize::<u32, _>(reader)?;

        Ok(StakingContract {
            balance,
            active_stake_sorted,
            active_stake_by_address,
            inactive_stake_by_address,
            current_epoch_parking,
            previous_epoch_parking: last_epoch_parking
        })
    }
}

// Not really useful traits for StakingContracts.
// FIXME Assume a single staking contract for now, i.e. all staking contracts are equal.
impl PartialEq for StakingContract {
    fn eq(&self, _other: &StakingContract) -> bool {
        true
    }
}

impl Eq for StakingContract {}

impl PartialOrd for StakingContract {
    fn partial_cmp(&self, other: &StakingContract) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StakingContract {
    fn cmp(&self, _other: &Self) -> Ordering {
        Ordering::Equal
    }
}

impl Default for StakingContract {
    fn default() -> Self {
        StakingContract {
            balance: Coin::ZERO,
            active_stake_sorted: BTreeSet::new(),
            active_stake_by_address: HashMap::new(),
            inactive_stake_by_address: HashMap::new(),
            current_epoch_parking: HashSet::new(),
            previous_epoch_parking: HashSet::new(),
        }
    }
}


#[test]
fn it_can_de_serialize_an_active_stake_receipt() {
    const ACTIVE_STAKE_RECEIPT: &str = "96b94e8a2fa79cb3d96bfde5ed2fa693aa6bec225e944b23c96b1c83dda67b34b62d105763bdf3cd378de9e4d8809fb00f815e309ec94126f22d77ef81fe00fa3a51a6c750349efda2133ca2f0e1b04094c4e2ce08b73c72fccedc33e127259f010303030303030303030303030303030303030303";
    const BLS_PUBLIC_KEY: &str = "96b94e8a2fa79cb3d96bfde5ed2fa693aa6bec225e944b23c96b1c83dda67b34b62d105763bdf3cd378de9e4d8809fb00f815e309ec94126f22d77ef81fe00fa3a51a6c750349efda2133ca2f0e1b04094c4e2ce08b73c72fccedc33e127259f";

    let bytes: Vec<u8> = hex::decode(ACTIVE_STAKE_RECEIPT).unwrap();
    let asr: ActiveStakeReceipt = Deserialize::deserialize(&mut &bytes[..]).unwrap();
    let bls_bytes: Vec<u8> = hex::decode(BLS_PUBLIC_KEY).unwrap();
    let bls_pubkey: BlsPublicKey = Deserialize::deserialize(&mut &bls_bytes[..]).unwrap();
    assert_eq!(asr.validator_key, bls_pubkey);

    assert_eq!(hex::encode(asr.serialize_to_vec()), ACTIVE_STAKE_RECEIPT);
}
