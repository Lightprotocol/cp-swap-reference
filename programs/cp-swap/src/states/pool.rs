use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;
use std::ops::{BitAnd, BitOr, BitXor, Deref};
/// Seed to derive account address and signature
pub const POOL_SEED: &str = "pool";
pub const POOL_LP_MINT_SEED: &str = "pool_lp_mint";
pub const POOL_VAULT_SEED: &str = "pool_vault";

pub const Q32: u128 = (u32::MAX as u128) + 1; // 2^32

pub enum PoolStatusBitIndex {
    Deposit,
    Withdraw,
    Swap,
}

#[derive(PartialEq, Eq)]
pub enum PoolStatusBitFlag {
    Enable,
    Disable,
}

#[derive(Default, Debug, AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct PoolAddresses {
    pub amm_config: Pubkey,
    pub pool_creator: Pubkey,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    pub observation_key: Pubkey,
}

impl Space for PoolAddresses {
    const INIT_SPACE: usize = 32 * 10; // 10 Pubkeys
}

#[derive(Default, Debug, AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct PoolMetadata {
    pub lp_mint_decimals: u8,
    pub mint_0_decimals: u8,
    pub mint_1_decimals: u8,
    pub open_time: u64,
}

impl Space for PoolMetadata {
    const INIT_SPACE: usize = 1 * 3 + 8 * 1; // 3 u8s + 1 u64
}

#[account(zero_copy(unsafe))]
#[repr(C, packed)]
#[derive(Default, Debug, InitSpace)]
pub struct PoolState {
    // TOP LEVEL - Active fields (8 fields)
    pub protocol_fees_token_0: u64,
    pub protocol_fees_token_1: u64,
    pub fund_fees_token_0: u64,
    pub fund_fees_token_1: u64,
    pub lp_supply: u64,
    pub recent_epoch: u64,
    pub status: u8,
    pub auth_bump: u8,

    // NESTED structures
    pub addresses: PoolAddresses, // 10 pubkeys
    pub metadata: PoolMetadata,   // decimals + open_time

    pub padding: [u64; 1],
}

impl Deref for PoolState {
    type Target = PoolAddresses;

    fn deref(&self) -> &Self::Target {
        &self.addresses
    }
}

impl PoolState {
    pub const LEN: usize = 8 + 8 * 6 + 1 * 2 + (32 * 10) + (1 * 3 + 8 * 1) + 1 + 9 + 8 * 1;

    pub fn initialize(
        &mut self,
        auth_bump: u8,
        lp_supply: u64,
        open_time: u64,
        pool_creator: Pubkey,
        amm_config: Pubkey,
        token_0_vault: Pubkey,
        token_1_vault: Pubkey,
        token_0_mint: &InterfaceAccount<Mint>,
        token_1_mint: &InterfaceAccount<Mint>,
        lp_mint: &InterfaceAccount<Mint>,
        observation_key: Pubkey,
    ) {
        self.addresses.amm_config = amm_config.key();
        self.addresses.pool_creator = pool_creator.key();
        self.addresses.token_0_vault = token_0_vault;
        self.addresses.token_1_vault = token_1_vault;
        self.addresses.lp_mint = lp_mint.key();
        self.addresses.token_0_mint = token_0_mint.key();
        self.addresses.token_1_mint = token_1_mint.key();
        self.addresses.token_0_program = *token_0_mint.to_account_info().owner;
        self.addresses.token_1_program = *token_1_mint.to_account_info().owner;
        self.addresses.observation_key = observation_key;
        self.auth_bump = auth_bump;
        self.metadata.lp_mint_decimals = lp_mint.decimals;
        self.metadata.mint_0_decimals = token_0_mint.decimals;
        self.metadata.mint_1_decimals = token_1_mint.decimals;
        self.lp_supply = lp_supply;
        self.protocol_fees_token_0 = 0;
        self.protocol_fees_token_1 = 0;
        self.fund_fees_token_0 = 0;
        self.fund_fees_token_1 = 0;
        self.metadata.open_time = open_time;
        self.recent_epoch = Clock::get().unwrap().epoch;
        self.padding = [0u64; 1];
    }

    pub fn set_status(&mut self, status: u8) {
        self.status = status
    }

    pub fn set_status_by_bit(&mut self, bit: PoolStatusBitIndex, flag: PoolStatusBitFlag) {
        let s = u8::from(1) << (bit as u8);
        if flag == PoolStatusBitFlag::Disable {
            self.status = self.status.bitor(s);
        } else {
            let m = u8::from(255).bitxor(s);
            self.status = self.status.bitand(m);
        }
    }

    /// Get status by bit, if it is `noraml` status, return true
    pub fn get_status_by_bit(&self, bit: PoolStatusBitIndex) -> bool {
        let status = u8::from(1) << (bit as u8);
        self.status.bitand(status) == 0
    }

    pub fn vault_amount_without_fee(&self, vault_0: u64, vault_1: u64) -> (u64, u64) {
        (
            vault_0
                .checked_sub(self.protocol_fees_token_0 + self.fund_fees_token_0)
                .unwrap(),
            vault_1
                .checked_sub(self.protocol_fees_token_1 + self.fund_fees_token_1)
                .unwrap(),
        )
    }

    pub fn token_price_x32(&self, vault_0: u64, vault_1: u64) -> (u128, u128) {
        let (token_0_amount, token_1_amount) = self.vault_amount_without_fee(vault_0, vault_1);
        (
            token_1_amount as u128 * Q32 as u128 / token_0_amount as u128,
            token_0_amount as u128 * Q32 as u128 / token_1_amount as u128,
        )
    }
}

#[cfg(test)]
pub mod pool_test {
    use super::*;

    #[test]
    fn pool_state_size_test() {
        assert_eq!(std::mem::size_of::<PoolState>(), PoolState::LEN - 8)
    }

    mod pool_status_test {
        use super::*;

        #[test]
        fn get_set_status_by_bit() {
            let mut pool_state = PoolState::default();
            pool_state.set_status(4); // 0000100
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Swap),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit),
                true
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw),
                true
            );

            // disable -> disable, nothing to change
            pool_state.set_status_by_bit(PoolStatusBitIndex::Swap, PoolStatusBitFlag::Disable);
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Swap),
                false
            );

            // disable -> enable
            pool_state.set_status_by_bit(PoolStatusBitIndex::Swap, PoolStatusBitFlag::Enable);
            assert_eq!(pool_state.get_status_by_bit(PoolStatusBitIndex::Swap), true);

            // enable -> enable, nothing to change
            pool_state.set_status_by_bit(PoolStatusBitIndex::Swap, PoolStatusBitFlag::Enable);
            assert_eq!(pool_state.get_status_by_bit(PoolStatusBitIndex::Swap), true);
            // enable -> disable
            pool_state.set_status_by_bit(PoolStatusBitIndex::Swap, PoolStatusBitFlag::Disable);
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Swap),
                false
            );

            pool_state.set_status(5); // 0000101
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Swap),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw),
                true
            );

            pool_state.set_status(7); // 0000111
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Swap),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw),
                false
            );

            pool_state.set_status(3); // 0000011
            assert_eq!(pool_state.get_status_by_bit(PoolStatusBitIndex::Swap), true);
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit),
                false
            );
            assert_eq!(
                pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw),
                false
            );
        }
    }
}
