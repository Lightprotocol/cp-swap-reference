use crate::curve::CurveCalculator;
use crate::curve::RoundDirection;
use crate::error::ErrorCode;
use crate::state::*;
use crate::utils::ctoken::get_bumps;
use crate::utils::token::*;
use crate::utils::transfer_ctoken_from_pool_vault_to_user;
use anchor_lang::prelude::*;
use anchor_spl::token::Token;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
use light_sdk::compressible::HasCompressionInfo;

#[derive(Accounts)]
pub struct Deposit<'info> {
    /// Pays to mint the position
    pub owner: Signer<'info>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub pool_state: Account<'info, PoolState>,

    /// Owner lp token account
    #[account(mut, token::authority = owner)]
    pub owner_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The payer's token account for token_0
    #[account(
        mut,
        token::mint = token_0_vault.mint,
        token::authority = owner
    )]
    pub token_0_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The payer's token account for token_1
    #[account(
        mut,
        token::mint = token_1_vault.mint,
        token::authority = owner
    )]
    pub token_1_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that holds pool tokens for token_0
    #[account(
        mut,
        constraint = token_0_vault.key() == pool_state.token_0_vault
    )]
    pub token_0_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that holds pool tokens for token_1
    #[account(
        mut,
        constraint = token_1_vault.key() == pool_state.token_1_vault
    )]
    pub token_1_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// token Program
    pub token_program: Program<'info, Token>,

    /// Token program 2022
    pub token_program_2022: Program<'info, Token2022>,

    /// The mint of token_0 vault
    #[account(
        address = token_0_vault.mint,
        mint::token_program = pool_state.token_0_program,
    )]
    pub vault_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of token_1 vault
    #[account(
        address = token_1_vault.mint,
        mint::token_program = pool_state.token_1_program,
    )]
    pub vault_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Lp token vault
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.lp_mint.as_ref()
        ],
        bump,
        token::mint = lp_vault.mint,
        token::authority = authority
    )]
    pub lp_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// System program for lamport transfers
    pub system_program: Program<'info, System>,

    /// CHECK: checked by protocol.
    pub light_token_program_cpi_authority: AccountInfo<'info>,

    /// CHECK: checked by protocol.
    pub light_token_program: AccountInfo<'info>,

    /// CHECK: checked by protocol.
    #[account(mut)]
    pub spl_interface_0_pda: AccountInfo<'info>,

    /// CHECK: checked by protocol.
    #[account(mut)]
    pub spl_interface_1_pda: AccountInfo<'info>,
}

pub fn deposit(
    ctx: Context<Deposit>,
    lp_token_amount: u64,
    maximum_token_0_amount: u64,
    maximum_token_1_amount: u64,
) -> Result<()> {
    require_gt!(lp_token_amount, 0);
    let pool_id = ctx.accounts.pool_state.key();
    let pool_state = &mut ctx.accounts.pool_state;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit) {
        return err!(ErrorCode::NotApproved);
    }
    let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
        ctx.accounts.token_0_vault.amount,
        ctx.accounts.token_1_vault.amount,
    );
    let results = CurveCalculator::lp_tokens_to_trading_tokens(
        u128::from(lp_token_amount),
        u128::from(pool_state.lp_supply),
        u128::from(total_token_0_amount),
        u128::from(total_token_1_amount),
        RoundDirection::Ceiling,
    )
    .ok_or(ErrorCode::ZeroTradingTokens)?;
    if results.token_0_amount == 0 || results.token_1_amount == 0 {
        return err!(ErrorCode::ZeroTradingTokens);
    }
    let token_0_amount = u64::try_from(results.token_0_amount).unwrap();
    let (transfer_token_0_amount, transfer_token_0_fee) = {
        let transfer_fee =
            get_transfer_inverse_fee(&ctx.accounts.vault_0_mint.to_account_info(), token_0_amount)?;
        (
            token_0_amount.checked_add(transfer_fee).unwrap(),
            transfer_fee,
        )
    };

    let token_1_amount = u64::try_from(results.token_1_amount).unwrap();
    let (transfer_token_1_amount, transfer_token_1_fee) = {
        let transfer_fee =
            get_transfer_inverse_fee(&ctx.accounts.vault_1_mint.to_account_info(), token_1_amount)?;
        (
            token_1_amount.checked_add(transfer_fee).unwrap(),
            transfer_fee,
        )
    };

    emit!(LpChangeEvent {
        pool_id,
        lp_amount_before: pool_state.lp_supply,
        token_0_vault_before: total_token_0_amount,
        token_1_vault_before: total_token_1_amount,
        token_0_amount,
        token_1_amount,
        token_0_transfer_fee: transfer_token_0_fee,
        token_1_transfer_fee: transfer_token_1_fee,
        change_type: 0
    });

    if transfer_token_0_amount > maximum_token_0_amount
        || transfer_token_1_amount > maximum_token_1_amount
    {
        return Err(ErrorCode::ExceededSlippage.into());
    }
    let (spl_interface_0_bump, spl_interface_1_bump) = get_bumps(
        ctx.accounts.vault_0_mint.key(),
        ctx.accounts.vault_1_mint.key(),
        ctx.accounts.light_token_program.key(),
    );

    transfer_from_user_to_pool_vault(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_0_account.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        Some(ctx.accounts.vault_0_mint.to_account_info()),
        Some(
            if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
        ),
        Some(ctx.accounts.spl_interface_0_pda.to_account_info()),
        Some(spl_interface_0_bump),
        ctx.accounts.light_token_program_cpi_authority.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        transfer_token_0_amount,
        ctx.accounts.vault_0_mint.decimals,
    )?;

    transfer_from_user_to_pool_vault(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_1_account.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        Some(ctx.accounts.vault_1_mint.to_account_info()),
        Some(
            if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
        ),
        Some(ctx.accounts.spl_interface_1_pda.to_account_info()),
        Some(spl_interface_1_bump),
        ctx.accounts.light_token_program_cpi_authority.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        transfer_token_1_amount,
        ctx.accounts.vault_1_mint.decimals,
    )?;

    pool_state.lp_supply = pool_state.lp_supply.checked_add(lp_token_amount).unwrap();

    // LP tokens use 9 decimals
    transfer_ctoken_from_pool_vault_to_user(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.lp_vault.to_account_info(),
        ctx.accounts.owner_lp_token.to_account_info(),
        lp_token_amount,
        9,
        ctx.accounts.light_token_program_cpi_authority.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
    )?;
    pool_state.recent_epoch = Clock::get()?.epoch;

    // The account was written to, so we must update CompressionInfo.
    pool_state.compression_info_mut().bump_last_claimed_slot()?;

    Ok(())
}
