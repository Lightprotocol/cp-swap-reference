use crate::curve::CurveCalculator;
use crate::error::ErrorCode;
use crate::states::*;
use crate::utils::*;
use anchor_lang::{
    accounts::interface_account::InterfaceAccount,
    prelude::*,
    solana_program::{clock, program::invoke, system_instruction},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::spl_token,
    token::Token,
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use light_compressible::CreateAccountsProof;
use light_sdk_macros::RentFree;
use light_token_sdk::token::{
    CreateTokenAccountCpi, CreateTokenAtaCpi, MintToCpi, COMPRESSIBLE_CONFIG_V1,
    RENT_SPONSOR as CTOKEN_RENT_SPONSOR,
};

use spl_token_2022;
use std::ops::Deref;

pub const LP_MINT_SIGNER_SEED: &[u8] = b"pool_lp_mint";

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitializeParams {
    pub init_amount_0: u64,
    pub init_amount_1: u64,
    pub open_time: u64,
    pub create_accounts_proof: CreateAccountsProof,
    pub lp_mint_signer_bump: u8,
    pub lp_vault_bump: u8,
    pub creator_lp_token_bump: u8,
}

#[derive(Accounts, RentFree)]
#[instruction(params: InitializeParams)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,

    pub amm_config: Box<Account<'info, AmmConfig>>,

    #[account(
        seeds = [crate::AUTH_SEED.as_bytes()],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    #[account(
        init,
        seeds = [
            POOL_SEED.as_bytes(),
            amm_config.key().as_ref(),
            token_0_mint.key().as_ref(),
            token_1_mint.key().as_ref(),
        ],
        bump,
        payer = creator,
        space = 8 + PoolState::INIT_SPACE
    )]
    #[rentfree]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        constraint = token_0_mint.key() < token_1_mint.key(),
        mint::token_program = token_0_program,
    )]
    pub token_0_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mint::token_program = token_1_program)]
    pub token_1_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [LP_MINT_SIGNER_SEED, pool_state.key().as_ref()],
        bump,
    )]
    pub lp_mint_signer: UncheckedAccount<'info>,

    #[account(mut)]
    #[light_mint(
        mint_signer = lp_mint_signer,
        authority = authority,
        decimals = 9,
        signer_seeds = &[LP_MINT_SIGNER_SEED, self.pool_state.to_account_info().key.as_ref(), &[params.lp_mint_signer_bump]]
    )]
    pub lp_mint: UncheckedAccount<'info>,

    #[account(
        mut,
        token::mint = token_0_mint,
        token::authority = creator,
    )]
    pub creator_token_0: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = token_1_mint,
        token::authority = creator,
    )]
    pub creator_token_1: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub creator_lp_token: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [POOL_VAULT_SEED.as_bytes(), lp_mint.key().as_ref()],
        bump,
    )]
    #[rentfree_token(authority = [crate::AUTH_SEED.as_bytes()])]
    pub lp_vault: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_0_mint.key().as_ref()
        ],
        bump,
    )]
    #[rentfree_token(authority = [crate::AUTH_SEED.as_bytes()])]
    pub token_0_vault: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_1_mint.key().as_ref()
        ],
        bump,
    )]
    #[rentfree_token(authority = [crate::AUTH_SEED.as_bytes()])]
    pub token_1_vault: UncheckedAccount<'info>,

    #[account(
        init,
        seeds = [OBSERVATION_SEED.as_bytes(), pool_state.key().as_ref()],
        bump,
        payer = creator,
        space = 8 + ObservationState::INIT_SPACE
    )]
    pub observation_state: Box<Account<'info, ObservationState>>,

    #[account(mut, address = crate::create_pool_fee_receiver::ID)]
    pub create_pool_fee: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub token_0_program: Interface<'info, TokenInterface>,
    pub token_1_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,

    pub compression_config: AccountInfo<'info>,

    #[account(address = COMPRESSIBLE_CONFIG_V1)]
    pub ctoken_compressible_config: AccountInfo<'info>,

    #[account(mut, address = CTOKEN_RENT_SPONSOR)]
    pub ctoken_rent_sponsor: AccountInfo<'info>,

    pub light_token_program: AccountInfo<'info>,

    /// CHECK: CToken CPI authority.
    pub ctoken_cpi_authority: AccountInfo<'info>,
}

pub fn initialize<'info>(
    ctx: Context<'_, '_, '_, 'info, Initialize<'info>>,
    params: InitializeParams,
) -> Result<()> {
    let init_amount_0 = params.init_amount_0;
    let init_amount_1 = params.init_amount_1;
    let mut open_time = params.open_time;
    if !(is_supported_mint(&ctx.accounts.token_0_mint).unwrap()
        && is_supported_mint(&ctx.accounts.token_1_mint).unwrap())
    {
        return err!(ErrorCode::NotSupportMint);
    }

    if ctx.accounts.amm_config.disable_create_pool {
        return err!(ErrorCode::NotApproved);
    }

    let block_timestamp = clock::Clock::get()?.unix_timestamp as u64;
    if open_time <= block_timestamp {
        open_time = block_timestamp + 1;
    }

    let pool_state_key = ctx.accounts.pool_state.key();

    // Create token_0 vault
    CreateTokenAccountCpi {
        payer: ctx.accounts.creator.to_account_info(),
        account: ctx.accounts.token_0_vault.to_account_info(),
        mint: ctx.accounts.token_0_mint.to_account_info(),
        owner: ctx.accounts.authority.key(),
    }
    .rent_free(
        ctx.accounts.ctoken_compressible_config.to_account_info(),
        ctx.accounts.ctoken_rent_sponsor.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        &crate::ID,
    )
    .invoke_signed(&[
        POOL_VAULT_SEED.as_bytes(),
        pool_state_key.as_ref(),
        ctx.accounts.token_0_mint.key().as_ref(),
        &[ctx.bumps.token_0_vault],
    ])?;

    // Create token_1 vault
    CreateTokenAccountCpi {
        payer: ctx.accounts.creator.to_account_info(),
        account: ctx.accounts.token_1_vault.to_account_info(),
        mint: ctx.accounts.token_1_mint.to_account_info(),
        owner: ctx.accounts.authority.key(),
    }
    .rent_free(
        ctx.accounts.ctoken_compressible_config.to_account_info(),
        ctx.accounts.ctoken_rent_sponsor.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        &crate::ID,
    )
    .invoke_signed(&[
        POOL_VAULT_SEED.as_bytes(),
        pool_state_key.as_ref(),
        ctx.accounts.token_1_mint.key().as_ref(),
        &[ctx.bumps.token_1_vault],
    ])?;

    // Transfer tokens from creator to vaults
    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_0.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.token_0_mint.to_account_info(),
        ctx.accounts.token_0_program.to_account_info(),
        init_amount_0,
    )?;

    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_1.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_mint.to_account_info(),
        ctx.accounts.token_1_program.to_account_info(),
        init_amount_1,
    )?;

    let token_0_vault =
        spl_token_2022::extension::StateWithExtensions::<spl_token_2022::state::Account>::unpack(
            ctx.accounts
                .token_0_vault
                .to_account_info()
                .try_borrow_data()?
                .deref(),
        )?
        .base;
    let token_1_vault =
        spl_token_2022::extension::StateWithExtensions::<spl_token_2022::state::Account>::unpack(
            ctx.accounts
                .token_1_vault
                .to_account_info()
                .try_borrow_data()?
                .deref(),
        )?
        .base;

    CurveCalculator::validate_supply(token_0_vault.amount, token_1_vault.amount)?;

    // Charge the fee to create a pool
    if ctx.accounts.amm_config.create_pool_fee != 0 {
        invoke(
            &system_instruction::transfer(
                ctx.accounts.creator.key,
                &ctx.accounts.create_pool_fee.key(),
                u64::from(ctx.accounts.amm_config.create_pool_fee),
            ),
            &[
                ctx.accounts.creator.to_account_info(),
                ctx.accounts.create_pool_fee.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        invoke(
            &spl_token::instruction::sync_native(
                ctx.accounts.token_program.key,
                &ctx.accounts.create_pool_fee.key(),
            )?,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.create_pool_fee.to_account_info(),
            ],
        )?;
    }

    let liquidity = U128::from(token_0_vault.amount)
        .checked_mul(token_1_vault.amount.into())
        .unwrap()
        .integer_sqrt()
        .as_u64();
    let lock_lp_amount = 100;

    let user_lp_amount = liquidity
        .checked_sub(lock_lp_amount)
        .ok_or(ErrorCode::InitLpAmountTooLess)?;
    let vault_lp_amount = u64::MAX
        .checked_sub(user_lp_amount)
        .ok_or(ErrorCode::InitLpAmountTooLess)?;

    let pool_state = &mut ctx.accounts.pool_state;
    let observation_state = &mut ctx.accounts.observation_state;
    let observation_state_key = observation_state.key();
    observation_state.pool_id = pool_state_key;

    pool_state.initialize(
        ctx.bumps.authority,
        liquidity,
        open_time,
        ctx.accounts.creator.key(),
        ctx.accounts.amm_config.key(),
        ctx.accounts.token_0_vault.key(),
        ctx.accounts.token_1_vault.key(),
        &ctx.accounts.token_0_mint,
        &ctx.accounts.token_1_mint,
        &ctx.accounts.lp_vault,
        &ctx.accounts.lp_mint,
        observation_state_key,
    );

    let lp_mint_key = ctx.accounts.lp_mint.key();

    // Create LP vault
    CreateTokenAccountCpi {
        payer: ctx.accounts.creator.to_account_info(),
        account: ctx.accounts.lp_vault.to_account_info(),
        mint: ctx.accounts.lp_mint.to_account_info(),
        owner: ctx.accounts.authority.key(),
    }
    .rent_free(
        ctx.accounts.ctoken_compressible_config.to_account_info(),
        ctx.accounts.ctoken_rent_sponsor.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        &crate::ID,
    )
    .invoke_signed(&[
        POOL_VAULT_SEED.as_bytes(),
        lp_mint_key.as_ref(),
        &[params.lp_vault_bump],
    ])?;

    // Create creator LP token ATA
    CreateTokenAtaCpi {
        payer: ctx.accounts.creator.to_account_info(),
        owner: ctx.accounts.creator.to_account_info(),
        mint: ctx.accounts.lp_mint.to_account_info(),
        ata: ctx.accounts.creator_lp_token.to_account_info(),
        bump: params.creator_lp_token_bump,
    }
    .idempotent()
    .rent_free(
        ctx.accounts.ctoken_compressible_config.to_account_info(),
        ctx.accounts.ctoken_rent_sponsor.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
    )
    .invoke()?;

    // Mint LP tokens to vault
    MintToCpi {
        mint: ctx.accounts.lp_mint.to_account_info(),
        destination: ctx.accounts.lp_vault.to_account_info(),
        amount: vault_lp_amount,
        authority: ctx.accounts.authority.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        max_top_up: None,
    }
    .invoke_signed(&[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]])?;

    // Mint LP tokens to creator
    MintToCpi {
        mint: ctx.accounts.lp_mint.to_account_info(),
        destination: ctx.accounts.creator_lp_token.to_account_info(),
        amount: user_lp_amount,
        authority: ctx.accounts.authority.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        max_top_up: None,
    }
    .invoke_signed(&[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]])?;

    Ok(())
}
