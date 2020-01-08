use keys::Address;
use primitives::coin::Coin;
use transaction::account::vesting_contract::CreationTransactionData as VestingCreationData;

use crate::recipient::Recipient;

pub struct VestingRecipientBuilder {
    contract_creation_data: VestingCreationData,
}

impl VestingRecipientBuilder {
    pub fn new(owner: Address) -> Self {
        VestingRecipientBuilder {
            contract_creation_data: VestingCreationData {
                owner,
                start: 0,
                step_blocks: 0,
                step_amount: Default::default(),
                total_amount: Default::default()
            },
        }
    }

    pub fn new_single_step(owner: Address, start_block: u32, amount: Coin) -> Self {
        let mut builder = Self::new(owner);
        builder.with_step_blocks(start_block)
            .with_total_amount(amount)
            .with_step_amount(amount);
        builder
    }

    pub fn with_owner(&mut self, owner: Address) -> &mut Self {
        self.contract_creation_data.owner = owner;
        self
    }

    pub fn with_start_block(&mut self, start_block: u32) -> &mut Self {
        self.contract_creation_data.start = start_block;
        self
    }

    pub fn with_total_amount(&mut self, amount: Coin) -> &mut Self {
        self.contract_creation_data.total_amount = amount;
        self
    }

    pub fn with_step_blocks(&mut self, step_blocks: u32) -> &mut Self {
        self.contract_creation_data.step_blocks = step_blocks;
        self
    }

    pub fn with_step_amount(&mut self, step_amount: Coin) -> &mut Self {
        self.contract_creation_data.step_amount = step_amount;
        self
    }

    pub fn generate(self) -> Recipient {
        Recipient::VestingCreation {
            data: self.into()
        }
    }
}

impl From<VestingRecipientBuilder> for VestingCreationData {
    fn from(builder: VestingRecipientBuilder) -> Self {
        builder.contract_creation_data
    }
}

impl From<VestingRecipientBuilder> for Recipient {
    fn from(builder: VestingRecipientBuilder) -> Self {
        builder.generate()
    }
}
