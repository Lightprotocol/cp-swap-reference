use crate::curve::CurveCalculator;
use crate::error::ErrorCode;
use crate::state::*;
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
use light_ctoken_sdk::ctoken::{CTokenMintToCpi, CompressibleParamsCpi, CreateCTokenAccountCpi};
use light_ctoken_sdk::ValidityProof;
use light_sdk::instruction::PackedAddressTreeInfo;
use light_sdk_macros::Compressible;
use light_sdk_macros::{light_instruction, LightFinalize};
use spl_token_2022;
use std::ops::Deref;

pub const LOCK_LP_AMOUNT: u64 = 100;

#[derive(Accounts, LightFinalize)]
#[instruction(compression_params: InitializeCompressionParams)]
pub struct Initialize<'info> {
    /// Address paying to create the pool. Can be anyone
    #[account(mut)]
    pub creator: Signer<'info>,

    /// Which config the pool belongs to.
    pub amm_config: Box<Account<'info, AmmConfig>>,

    /// CHECK:
    /// pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// Initialize a rent-free account to store the pool state (compressible)
    #[account(
        init,
        seeds = [
            POOL_SEED.as_bytes(),
            amm_config.to_account_info().key.as_ref(),
            token_0_mint.to_account_info().key.as_ref(),
            token_1_mint.to_account_info().key.as_ref(),
        ],
        bump,
        payer = creator,
        space = 8 + PoolState::INIT_SPACE,
    )]
    #[compressible(
        address_tree_info = compression_params.pool_address_tree_info,
        output_tree = compression_params.output_state_tree_index
    )]
    pub pool_state: Box<Account<'info, PoolState>>,

    /// Token_0 mint, the key must smaller than token_1 mint.
    #[account(
        constraint = token_0_mint.key() < token_1_mint.key(),
        mint::token_program = token_0_program,
    )]
    pub token_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Token_1 mint, the key must grater then token_0 mint.
    #[account(
        mint::token_program = token_1_program,
    )]
    pub token_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Signer pda used to derive lp_mint and address.
    /// CHECK: checked by protocol
    #[account(
        seeds = [
            POOL_LP_MINT_SEED.as_bytes(),
            pool_state.to_account_info().key.as_ref(),
            ],
        bump,
    )]
    pub lp_mint_signer: UncheckedAccount<'info>,

    /// Light mint for LP tokens (created via #[light_mint] at instruction START)
    /// CHECK: Created via light_pre_init before instruction body runs
    #[account(mut)]
    #[light_mint(
        mint_signer = lp_mint_signer,
        authority = authority,
        decimals = 9,
        address_tree_info = compression_params.lp_mint_address_tree_info,
        signer_seeds = &[POOL_LP_MINT_SEED.as_bytes(), self.pool_state.to_account_info().key.as_ref(), &[compression_params.lp_mint_bump]]
    )]
    pub lp_mint: UncheckedAccount<'info>,
    /// payer token0 account
    #[account(
        mut,
        token::mint = token_0_mint,
        token::authority = creator,
    )]
    pub creator_token_0: Box<InterfaceAccount<'info, TokenAccount>>,

    /// creator token1 account
    #[account(
        mut,
        token::mint = token_1_mint,
        token::authority = creator,
    )]
    pub creator_token_1: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK:
    #[account(mut)]
    pub creator_lp_token: UncheckedAccount<'info>,

    /// CHECK: Token_0 vault for the pool, created via CTokenAccountCpi
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.to_account_info().key.as_ref(),
            token_0_mint.to_account_info().key.as_ref()
        ],
        bump,
    )]
    pub token_0_vault: UncheckedAccount<'info>,

    /// CHECK: Token_1 vault for the pool, created via CTokenAccountCpi
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.to_account_info().key.as_ref(),
            token_1_mint.to_account_info().key.as_ref()
        ],
        bump,
    )]
    pub token_1_vault: UncheckedAccount<'info>,

    /// create pool fee account
    #[account(
        mut,
        address= crate::create_pool_fee_receiver::ID,
    )]
    pub create_pool_fee: Box<InterfaceAccount<'info, TokenAccount>>,

    /// an account to store oracle observations (compressible)
    #[account(
        init,
        seeds = [
            OBSERVATION_SEED.as_bytes(),
            pool_state.to_account_info().key.as_ref(),
        ],
        bump,
        payer = creator,
        space = 8 + ObservationState::INIT_SPACE
    )]
    #[compressible(
        address_tree_info = compression_params.observation_address_tree_info,
        output_tree = compression_params.output_state_tree_index
    )]
    pub observation_state: Box<Account<'info, ObservationState>>,

    /// Program to create mint account and mint tokens
    pub token_program: Program<'info, Token>,
    /// Spl token program or token program 2022
    pub token_0_program: Interface<'info, TokenInterface>,
    /// Spl token program or token program 2022
    pub token_1_program: Interface<'info, TokenInterface>,
    /// Program to create an ATA for receiving position NFT
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// To create a new program account
    pub system_program: Program<'info, System>,
    /// Sysvar for program account
    pub rent: Sysvar<'info, Rent>,

    /// CHECK: checked via load_checked.
    pub compression_config: AccountInfo<'info>,
    /// CHECK: checked in instruction.
    #[account(mut)]
    pub rent_sponsor: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub light_token_program_cpi_authority: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub light_token_program: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub light_token_config_account: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub light_token_rent_sponsor: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub spl_interface_0_pda: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub spl_interface_1_pda: AccountInfo<'info>,
}

// This instruction:
// 0. Runs checks and loads compression config.
// 1. Creates token vault accounts for pool tokens as compressible.
// 2. Creates user token accounts as compressible.
// 3. Initializes PoolState and ObservationState as compressible.
// 4. Creates compressed token mint for LP tokens.
// 5. Distributes initial liquidity to user and vault.
// 6. Compresses PoolState via light_finalize (auto-called at end).
#[light_instruction(compression_params)]
pub fn initialize<'info>(
    ctx: Context<'_, '_, '_, 'info, Initialize<'info>>,
    init_amount_0: u64,
    init_amount_1: u64,
    mut open_time: u64,
    compression_params: InitializeCompressionParams,
) -> Result<()> {
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

    CreateCTokenAccountCpi {
        payer: ctx.accounts.creator.to_account_info(),
        account: ctx.accounts.token_0_vault.to_account_info(),
        mint: ctx.accounts.token_0_mint.to_account_info(),
        owner: *ctx.accounts.authority.to_account_info().key,
        compressible: CompressibleParamsCpi::new(
            ctx.accounts.light_token_config_account.to_account_info(),
            ctx.accounts.light_token_rent_sponsor.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ),
    }
    .invoke_signed(&[&[
        POOL_VAULT_SEED.as_bytes(),
        ctx.accounts.pool_state.to_account_info().key.as_ref(),
        ctx.accounts.token_0_mint.to_account_info().key.as_ref(),
        &[ctx.bumps.token_0_vault],
    ]])?;

    CreateCTokenAccountCpi {
        payer: ctx.accounts.creator.to_account_info(),
        account: ctx.accounts.token_1_vault.to_account_info(),
        mint: ctx.accounts.token_1_mint.to_account_info(),
        owner: *ctx.accounts.authority.to_account_info().key,
        compressible: CompressibleParamsCpi::new(
            ctx.accounts.light_token_config_account.to_account_info(),
            ctx.accounts.light_token_rent_sponsor.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ),
    }
    .invoke_signed(&[&[
        POOL_VAULT_SEED.as_bytes(),
        ctx.accounts.pool_state.to_account_info().key.as_ref(),
        ctx.accounts.token_1_mint.to_account_info().key.as_ref(),
        &[ctx.bumps.token_1_vault],
    ]])?;

    let (spl_interface_0_bump, spl_interface_1_bump) = get_bumps(
        ctx.accounts.token_0_mint.key(),
        ctx.accounts.token_1_mint.key(),
        ctx.accounts.light_token_program.key(),
    );

    let pool_state = &mut ctx.accounts.pool_state;
    let pool_state_key = pool_state.key();
    let observation_state = &mut ctx.accounts.observation_state;
    let observation_state_key = observation_state.key();
    observation_state.pool_id = pool_state_key;

    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_0.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        Some(ctx.accounts.token_0_mint.to_account_info()),
        Some(ctx.accounts.token_0_program.to_account_info()),
        Some(ctx.accounts.spl_interface_0_pda.to_account_info()),
        Some(spl_interface_0_bump),
        ctx.accounts
            .light_token_program_cpi_authority
            .to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        init_amount_0,
        ctx.accounts.token_0_mint.decimals,
    )?;

    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_1.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        Some(ctx.accounts.token_1_mint.to_account_info()),
        Some(ctx.accounts.token_1_program.to_account_info()),
        Some(ctx.accounts.spl_interface_1_pda.to_account_info()),
        Some(spl_interface_1_bump),
        ctx.accounts
            .light_token_program_cpi_authority
            .to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        init_amount_1,
        ctx.accounts.token_1_mint.decimals,
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

    let user_lp_amount = liquidity
        .checked_sub(LOCK_LP_AMOUNT)
        .ok_or(ErrorCode::InitLpAmountTooLess)?;

    pool_state.initialize(
        ctx.bumps.authority,
        user_lp_amount,
        open_time,
        ctx.accounts.creator.key(),
        ctx.accounts.amm_config.key(),
        ctx.accounts.token_0_vault.key(),
        ctx.accounts.token_1_vault.key(),
        &ctx.accounts.token_0_mint,
        &ctx.accounts.token_1_mint,
        ctx.accounts.token_0_program.key(),
        ctx.accounts.token_1_program.key(),
        &ctx.accounts.lp_mint.to_account_info(),
        observation_state_key,
    );

    // Mint LP tokens to creator
    CTokenMintToCpi {
        cmint: ctx.accounts.lp_mint.to_account_info(),
        destination: ctx.accounts.creator_lp_token.to_account_info(),
        amount: user_lp_amount,
        authority: ctx.accounts.authority.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        max_top_up: None,
    }
    .invoke_signed(&[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]])?;

    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug)]
pub struct InitializeCompressionParams {
    // CPDA compression params
    pub pool_address_tree_info: PackedAddressTreeInfo,
    pub observation_address_tree_info: PackedAddressTreeInfo,

    // CMint compression params
    pub lp_mint_address_tree_info: PackedAddressTreeInfo,
    pub lp_mint_bump: u8,
    pub creator_lp_token_bump: u8,

    // Metadata fields (optional)
    pub name: Vec<u8>,
    pub symbol: Vec<u8>,
    pub uri: Vec<u8>,

    // Shared compression params
    pub proof: ValidityProof,
    pub output_state_tree_index: u8,
}
