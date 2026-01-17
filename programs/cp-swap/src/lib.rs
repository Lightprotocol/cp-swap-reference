pub mod curve;
pub mod error;
pub mod instructions;
pub mod states;
pub mod utils;

use crate::curve::fees::FEE_RATE_DENOMINATOR_VALUE;
pub use crate::instructions::initialize::Initialize;
pub use crate::states::{ObservationState, PoolState};
use anchor_lang::prelude::*;
use instructions::*;
use light_sdk::{derive_light_cpi_signer, derive_light_rent_sponsor_pda};
use light_sdk_macros::rentfree_program;
use light_sdk_types::CpiSigner;

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "raydium-cp-swap",
    project_url: "https://raydium.io",
    contacts: "link:https://immunefi.com/bounty/raydium",
    policy: "https://immunefi.com/bounty/raydium",
    source_code: "https://github.com/raydium-io/raydium-cp-swap",
    preferred_languages: "en",
    auditors: "https://github.com/raydium-io/raydium-docs/blob/master/audit/MadShield%20Q1%202024/raydium-cp-swap-v-1.0.0.pdf"
}

#[cfg(feature = "devnet")]
declare_id!("CPMDWBwJDtYax9qW7AyRuVC19Cc4L4Vcy4n2BHAbHkCW");
#[cfg(not(feature = "devnet"))]
declare_id!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub const PROGRAM_RENT_SPONSOR_DATA: ([u8; 32], u8) =
    derive_light_rent_sponsor_pda!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C", 1);

#[inline]
pub fn program_rent_sponsor() -> Pubkey {
    Pubkey::from(PROGRAM_RENT_SPONSOR_DATA.0)
}

pub mod admin {
    use super::{pubkey, Pubkey};
    #[cfg(feature = "devnet")]
    pub const ID: Pubkey = pubkey!("adMCyoCgfkg7bQiJ9aBJ59H3BXLY3r5LNLfPpQfMzBe");
    #[cfg(not(feature = "devnet"))]
    pub const ID: Pubkey = pubkey!("GThUX1Atko4tqhN2NaiTazWSeFWMuiUvfFnyJyUghFMJ");
}

pub mod create_pool_fee_receiver {
    use super::{pubkey, Pubkey};
    #[cfg(feature = "devnet")]
    pub const ID: Pubkey = pubkey!("G11FKBRaAkHAKuLCgLM6K6NUc9rTjPAznRCjZifrTQe2");
    #[cfg(not(feature = "devnet"))]
    pub const ID: Pubkey = pubkey!("DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8");
}

pub const AUTH_SEED: &str = "vault_and_lp_mint_auth_seed";

#[rentfree_program]
#[program]
pub mod raydium_cp_swap {
    #![allow(clippy::too_many_arguments)]

    use super::*;

    pub fn create_amm_config(
        ctx: Context<CreateAmmConfig>,
        index: u16,
        trade_fee_rate: u64,
        protocol_fee_rate: u64,
        fund_fee_rate: u64,
        create_pool_fee: u64,
    ) -> Result<()> {
        assert!(trade_fee_rate < FEE_RATE_DENOMINATOR_VALUE);
        assert!(protocol_fee_rate <= FEE_RATE_DENOMINATOR_VALUE);
        assert!(fund_fee_rate <= FEE_RATE_DENOMINATOR_VALUE);
        assert!(fund_fee_rate + protocol_fee_rate <= FEE_RATE_DENOMINATOR_VALUE);
        instructions::create_amm_config(
            ctx,
            index,
            trade_fee_rate,
            protocol_fee_rate,
            fund_fee_rate,
            create_pool_fee,
        )
    }

    pub fn update_amm_config(ctx: Context<UpdateAmmConfig>, param: u8, value: u64) -> Result<()> {
        instructions::update_amm_config(ctx, param, value)
    }

    pub fn update_pool_status(ctx: Context<UpdatePoolStatus>, status: u8) -> Result<()> {
        instructions::update_pool_status(ctx, status)
    }

    pub fn collect_protocol_fee(
        ctx: Context<CollectProtocolFee>,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Result<()> {
        instructions::collect_protocol_fee(ctx, amount_0_requested, amount_1_requested)
    }

    pub fn collect_fund_fee(
        ctx: Context<CollectFundFee>,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Result<()> {
        instructions::collect_fund_fee(ctx, amount_0_requested, amount_1_requested)
    }

    pub fn initialize<'info>(
        ctx: Context<'_, '_, '_, 'info, Initialize<'info>>,
        init_amount_0: u64,
        init_amount_1: u64,
        open_time: u64,
        params: InitializeParams,
    ) -> Result<()> {
        instructions::initialize(ctx, init_amount_0, init_amount_1, open_time, params)
    }

    pub fn deposit(
        ctx: Context<Deposit>,
        lp_token_amount: u64,
        maximum_token_0_amount: u64,
        maximum_token_1_amount: u64,
    ) -> Result<()> {
        instructions::deposit(
            ctx,
            lp_token_amount,
            maximum_token_0_amount,
            maximum_token_1_amount,
        )
    }

    pub fn withdraw(
        ctx: Context<Withdraw>,
        lp_token_amount: u64,
        minimum_token_0_amount: u64,
        minimum_token_1_amount: u64,
    ) -> Result<()> {
        instructions::withdraw(
            ctx,
            lp_token_amount,
            minimum_token_0_amount,
            minimum_token_1_amount,
        )
    }

    pub fn swap_base_input(
        ctx: Context<Swap>,
        amount_in: u64,
        minimum_amount_out: u64,
    ) -> Result<()> {
        instructions::swap_base_input(ctx, amount_in, minimum_amount_out)
    }

    pub fn swap_base_output(ctx: Context<Swap>, max_amount_in: u64, amount_out: u64) -> Result<()> {
        instructions::swap_base_output(ctx, max_amount_in, amount_out)
    }
}
