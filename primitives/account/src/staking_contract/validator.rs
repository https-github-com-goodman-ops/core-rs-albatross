use std::cmp::Ordering;
use std::collections::BTreeMap;

use beserial::{Deserialize, Serialize};
use bls::bls12_381::CompressedPublicKey as BlsPublicKey;
use keys::Address;
use primitives::coin::Coin;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Validator {
    pub balance: Coin,
    pub reward_address: Address,
    pub validator_key: BlsPublicKey,
    pub active_stake_by_address: BTreeMap<Address, Coin>,
}

impl PartialEq for Validator {
    fn eq(&self, other: &Validator) -> bool {
        self.validator_key == other.validator_key
            && self.balance == other.balance
    }
}

impl Eq for Validator {}

impl PartialOrd for Validator {
    fn partial_cmp(&self, other: &Validator) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Validator {
    // Highest to low balances
    fn cmp(&self, other: &Self) -> Ordering {
        other.balance.cmp(&self.balance)
            .then_with(|| self.validator_key.cmp(&other.validator_key))
    }
}
