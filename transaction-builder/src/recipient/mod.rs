use beserial::Serialize;
use keys::Address;
use nimiq_account::AccountType;
use transaction::account::htlc_contract::CreationTransactionData as HtlcCreationData;
use transaction::account::vesting_contract::CreationTransactionData as VestingCreationData;

use crate::recipient::htlc_contract::HtlcRecipientBuilder;
use crate::recipient::staking_contract::{StakingRecipientBuilder, StakingTransaction};
use crate::recipient::vesting_contract::VestingRecipientBuilder;

pub mod vesting_contract;
pub mod htlc_contract;
pub mod staking_contract;

pub enum Recipient {
    Basic {
        address: Address,
    },
    HtlcCreation {
        data: HtlcCreationData,
    },
    VestingCreation {
        data: VestingCreationData,
    },
    Staking {
        address: Address,
        data: StakingTransaction,
    },
}

impl Recipient {
    pub fn new_basic(address: Address) -> Self {
        Recipient::Basic {
            address
        }
    }

    pub fn new_htlc_builder() -> HtlcRecipientBuilder {
        HtlcRecipientBuilder::new()
    }

    pub fn new_vesting_builder(owner: Address) -> VestingRecipientBuilder {
        VestingRecipientBuilder::new(owner)
    }

    pub fn new_staking_builder(staking_contract: Address) -> StakingRecipientBuilder {
        StakingRecipientBuilder::new(staking_contract)
    }

    pub fn is_creation(&self) -> bool {
        match self {
            Recipient::Basic { .. } | Recipient::Staking { .. } => false,
            _ => true,
        }
    }

    pub fn account_type(&self) -> AccountType {
        match self {
            Recipient::Basic { .. } => AccountType::Basic,
            Recipient::HtlcCreation { .. } => AccountType::HTLC,
            Recipient::VestingCreation { .. } => AccountType::Vesting,
            Recipient::Staking { .. } => AccountType::Staking,
        }
    }

    pub fn address(&self) -> Option<&Address> {
        match self {
            Recipient::Basic { address } |
            Recipient::Staking { address, .. } => Some(address),
            _ => None,
        }
    }

    pub fn data(&self) -> Vec<u8> {
        match self {
            Recipient::Basic { .. } => Vec::new(),
            Recipient::HtlcCreation { data } => data.serialize_to_vec(),
            Recipient::VestingCreation { data } => data.serialize_to_vec(),
            Recipient::Staking { data, .. } => data.serialize_to_vec(),
        }
    }

    pub fn is_valid_sender(&self, sender: &Address, sender_type: Option<AccountType>) -> bool {
        match self {
            Recipient::Staking { address, data } => {
                if data.is_self_transaction() {
                    address == sender && sender_type == Some(AccountType::Staking)
                } else {
                    true
                }
            },
            _ => true,
        }
    }
}
