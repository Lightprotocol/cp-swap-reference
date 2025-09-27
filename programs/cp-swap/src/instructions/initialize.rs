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
use light_compressed_token_sdk::instructions::create_associated_ctoken_account;
use light_compressed_token_sdk::instructions::create_token_account::create_ctoken_account_signed;
use light_sdk::{
    compressible::CompressibleConfig,
    instruction::{borsh_compat::ValidityProof, PackedAddressTreeInfo},
};
use spl_token_2022;
use std::ops::Deref;

#[derive(Accounts)]
#[instruction(init_amount_0: u64, init_amount_1: u64, open_time: u64, compression_params: InitializeCompressionParams)]
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

    /// CHECK: Initialize an account to store the pool state
    #[account(
        init,
        compress_on_init,
        cpda::address_tree_info = compression_params.pool_address_tree_info,
        cpda::proof = compression_params.proof,
        cpda::output_state_tree_index = compression_params.output_state_tree_index,
        seeds = [
            POOL_SEED.as_bytes(),
            amm_config.key().as_ref(),
            token_0_mint.key().as_ref(),
            token_1_mint.key().as_ref(),
        ],
        bump,
        payer = creator,
        space = PoolState::INIT_SPACE
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

    /// Signer pda used to derive lp_mint and its compressed address.
    /// CHECK: checked by protocol.
    #[account(
        seeds = [
            POOL_LP_MINT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            ],
        bump,
    )]
    pub lp_mint_signer: UncheckedAccount<'info>,

    /// Compressed mint for LP tokens
    #[account(
        cmint::authority = authority,
        cmint::decimals = 9,
        cmint::payer = creator,
        cmint::mint_signer = lp_mint_signer,
        cmint::address_tree_info = compression_params.lp_mint_address_tree_info,
        cmint::proof = compression_params.proof,
        cmint::output_state_tree_index = compression_params.output_state_tree_index,
    )]
    pub lp_mint: CMint<'info>,
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

    /// CHECK:
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            lp_mint.key().as_ref()
        ],
        bump,
    )]
    pub lp_vault: UncheckedAccount<'info>,

    /// CHECK: Token_0 vault for the pool, created by contract
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_0_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_0_vault: UncheckedAccount<'info>,

    /// CHECK: Token_1 vault for the pool, created by contract
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_1_mint.key().as_ref()
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

    /// an account to store oracle observations
    #[account(
        init,
        compress_on_init,
        cpda::address_tree_info = compression_params.observation_address_tree_info,
        cpda::proof = compression_params.proof,
        cpda::output_state_tree_index = compression_params.output_state_tree_index,
        seeds = [
            OBSERVATION_SEED.as_bytes(),
            pool_state.key().as_ref(),
        ],
        bump,
        payer = creator,
        space = ObservationState::INIT_SPACE
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
    pub rent_recipient: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub compressed_token_program_cpi_authority: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub compressed_token_program: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub ctoken_config_account: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub ctoken_rent_recipient: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub compressed_token_0_pool_pda: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub compressed_token_1_pool_pda: AccountInfo<'info>,
}

// This instruction:
// 0. Runs checks and loads compression config.
// 1. Creates token vault accounts for pool tokens as compressible.
// 2. Creates user token accounts as compressible.
// 3. Initializes PoolState and ObservationState as compressible.
// 4. Creates compressed token mint for LP tokens.
// 5. Distributes initial liquidity to user and vault.
// 6. Compresses PoolState and ObservationState.
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

    // ZK Compression Step 1: Load compression config and check rent recipient
    let compression_config =
        CompressibleConfig::load_checked(&ctx.accounts.compression_config, &crate::ID)?;
    let rent_recipient = &ctx.accounts.rent_recipient;
    if rent_recipient.key() != compression_config.rent_recipient {
        return err!(ErrorCode::InvalidRentRecipient);
    }

    let block_timestamp = clock::Clock::get()?.unix_timestamp as u64;
    if open_time <= block_timestamp {
        open_time = block_timestamp + 1;
    }

    create_ctoken_account_signed(
        crate::ID,
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.token_0_mint.to_account_info(),
        *ctx.accounts.authority.to_account_info().key,
        &[
            POOL_VAULT_SEED.as_bytes(),
            ctx.accounts.pool_state.key().as_ref(),
            ctx.accounts.token_0_mint.key().as_ref(),
            &[ctx.bumps.token_0_vault][..],
        ],
        ctx.accounts.ctoken_rent_recipient.to_account_info(),
        ctx.accounts.ctoken_config_account.to_account_info(),
        Some(1),
        None,
    )?;

    create_ctoken_account_signed(
        crate::ID,
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_mint.to_account_info(),
        *ctx.accounts.authority.to_account_info().key,
        &[
            POOL_VAULT_SEED.as_bytes(),
            ctx.accounts.pool_state.key().as_ref(),
            ctx.accounts.token_1_mint.key().as_ref(),
            &[ctx.bumps.token_1_vault][..],
        ],
        ctx.accounts.ctoken_rent_recipient.to_account_info(),
        ctx.accounts.ctoken_config_account.to_account_info(),
        Some(1),
        None,
    )?;

    let (compressed_token_0_pool_bump, compressed_token_1_pool_bump) = get_bumps(
        ctx.accounts.token_0_mint.key(),
        ctx.accounts.token_1_mint.key(),
        ctx.accounts.compressed_token_program.key(),
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
        ctx.accounts.token_0_mint.to_account_info(),
        ctx.accounts.token_0_program.to_account_info(),
        ctx.accounts.compressed_token_0_pool_pda.to_account_info(),
        compressed_token_0_pool_bump,
        ctx.accounts
            .compressed_token_program_cpi_authority
            .to_account_info(),
        init_amount_0,
    )?;

    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_1.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_mint.to_account_info(),
        ctx.accounts.token_1_program.to_account_info(),
        ctx.accounts.compressed_token_1_pool_pda.to_account_info(),
        compressed_token_1_pool_bump,
        ctx.accounts
            .compressed_token_program_cpi_authority
            .to_account_info(),
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
        &ctx.accounts.lp_mint.to_account_info(),
        observation_state_key,
    );
    let _pool_auth_bump = pool_state.auth_bump;

    // ZK Compression Step 2: Setup CPI accounts. We compress PDAs **and**
    // create a cMint (lp_mint), so we need to use 'with_cpi_context'.
    // let cpi_accounts = CpiAccountsSmall::new_with_config(
    //     &ctx.accounts.creator,
    //     ctx.remaining_accounts,
    //     CpiAccountsConfig::new_with_cpi_context(LIGHT_CPI_SIGNER),
    // );

    // // ZK Compression Step 3: Compress the PDAs.
    // compress_pool_and_observation_pdas(
    //     &cpi_accounts,
    //     &pool_state,
    //     &observation_state,
    //     &compression_params,
    //     &rent_recipient,
    //     &compression_config.address_space,
    // )?;

    // ZK Compression Step 4: Create ctoken accounts. These match regular
    // SPL token accounts but are compressible.
    create_ctoken_account_signed(
        crate::ID,
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.lp_vault.to_account_info(),
        ctx.accounts.lp_mint.to_account_info(),
        *ctx.accounts.authority.to_account_info().key,
        &[
            POOL_VAULT_SEED.as_bytes(),
            ctx.accounts.lp_mint.key().as_ref(),
            &[ctx.bumps.lp_vault][..],
        ],
        ctx.accounts.ctoken_rent_recipient.to_account_info(),
        ctx.accounts.ctoken_config_account.to_account_info(),
        Some(1),
        None,
    )?;
    create_associated_ctoken_account(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_lp_token.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        ctx.accounts.ctoken_config_account.to_account_info(),
        ctx.accounts.ctoken_rent_recipient.to_account_info(),
        ctx.accounts.creator.to_account_info(),
        *ctx.accounts.lp_mint.to_account_info().key,
        compression_params.creator_lp_token_bump,
        Some(1),
        None,
    )?;

    ctx.accounts
        .lp_mint
        .mint_to(&ctx.accounts.creator_lp_token.key(), user_lp_amount)?;

    ctx.accounts
        .lp_mint
        .mint_to(&ctx.accounts.lp_vault.key(), vault_lp_amount)?;

    // ZK Compression Step 5: We create the lp cMint and distribute the lp tokens
    // to the lp_vault and user based on the regular LP math.
    // create_and_mint_lp(
    //     ctx.accounts.creator.to_account_info(),
    //     ctx.accounts.authority.to_account_info(),
    //     &ctx.accounts.lp_mint.key(),
    //     ctx.accounts.lp_vault.to_account_info(),
    //     ctx.accounts.creator_lp_token.to_account_info(),
    //     ctx.accounts.lp_mint_signer.to_account_info(),
    //     &pool_state_key,
    //     ctx.accounts
    //         .compressed_token_program_cpi_authority
    //         .to_account_info(),
    //     ctx.accounts.compressed_token_program.to_account_info(),
    //     ctx.bumps.lp_mint_signer,
    //     &compression_params,
    //     &cpi_accounts,
    //     user_lp_amount,
    //     vault_lp_amount,
    //     pool_auth_bump,
    // )?;

    // ZK Compression Step 6: Clean up compressed onchain PDAs. Always do this
    // at the end of your instruction. Only PoolState and ObservationState are
    // being compressed right away. All other accounts only initialized as
    // compressible - for async compression once they're inactive. PoolState and
    // ObservationState are compressed atomically for demo purposes. You can
    // choose whether to compress_at_init or only after they've become inactive.
    // If you compress_at_init, you pay 0 upfront rent, but the first
    // transaction to use the account must include a
    // decompress_accounts_idempotent instruction in their transaction which
    // fronts then rent. Only the first touch will actually decompress the
    // account; swap n+1 will still succeed.
    // pool_state.close(rent_recipient.clone())?;
    // observation_state.close(rent_recipient.clone())?;

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

    // Shared compression params
    pub proof: ValidityProof,
    pub output_state_tree_index: u8,
}
