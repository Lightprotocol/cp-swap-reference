use crate::curve::CurveCalculator;
use crate::curve::RoundDirection;
use crate::error::ErrorCode;
use crate::utils::ctoken::get_bumps;
use crate::states::*;
use crate::utils::token::*;
use crate::utils::transfer_ctoken_from_user_to_pool_vault;
use anchor_lang::prelude::*;
use anchor_spl::{
    memo::spl_memo,
    token::Token,
    token_interface::{Mint, Token2022, TokenAccount},
};
use light_sdk::compressible::HasCompressionInfo;

#[derive(Accounts)]
pub struct Withdraw<'info> {
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

    /// Pool state account
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut, 
        token::authority = owner
    )]
    pub owner_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The token account for receive token_0,
    #[account(
        mut,
        token::mint = token_0_vault.mint,
    )]
    pub token_0_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The token account for receive token_1
    #[account(
        mut,
        token::mint = token_1_vault.mint,
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

    /// Program lp token vault
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
    pub lp_vault: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: checked by protocol.
    pub compressed_token_program_cpi_authority: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    pub compressed_token_program: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    ///
    /// Every mint must be registered in the compression protocol via a
    /// compression_token_pool_pda.
    #[account(mut)]
    pub compressed_token_0_pool_pda: AccountInfo<'info>,
    /// CHECK: checked by protocol.
    #[account(mut)]
    pub compressed_token_1_pool_pda: AccountInfo<'info>,

    /// memo program
    /// CHECK:
    #[account(
        address = spl_memo::id()
    )]
    pub memo_program: UncheckedAccount<'info>,
}

pub fn withdraw(
    ctx: Context<Withdraw>,
    lp_token_amount: u64,
    minimum_token_0_amount: u64,
    minimum_token_1_amount: u64,
) -> Result<()> {
    require_gt!(lp_token_amount, 0);
    require_gt!(ctx.accounts.lp_vault.amount, 0);
    let pool_id = ctx.accounts.pool_state.key();
    let pool_state = &mut ctx.accounts.pool_state;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw) {
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
        RoundDirection::Floor,
    )
    .ok_or(ErrorCode::ZeroTradingTokens)?;
    if results.token_0_amount == 0 || results.token_1_amount == 0 {
        return err!(ErrorCode::ZeroTradingTokens);
    }
    let token_0_amount = u64::try_from(results.token_0_amount).unwrap();
    let token_0_amount = std::cmp::min(total_token_0_amount, token_0_amount);
    let (receive_token_0_amount, token_0_transfer_fee) = {
        let transfer_fee =
            get_transfer_fee(&ctx.accounts.vault_0_mint.to_account_info(), token_0_amount)?;
        (
            token_0_amount.checked_sub(transfer_fee).unwrap(),
            transfer_fee,
        )
    };

    let token_1_amount = u64::try_from(results.token_1_amount).unwrap();
    let token_1_amount = std::cmp::min(total_token_1_amount, token_1_amount);
    let (receive_token_1_amount, token_1_transfer_fee) = {
        let transfer_fee =
            get_transfer_fee(&ctx.accounts.vault_1_mint.to_account_info(), token_1_amount)?;
        (
            token_1_amount.checked_sub(transfer_fee).unwrap(),
            transfer_fee,
        )
    };

    #[cfg(feature = "enable-log")]
    msg!(
        "results.token_0_amount;{}, results.token_1_amount:{},receive_token_0_amount:{},token_0_transfer_fee:{},
            receive_token_1_amount:{},token_1_transfer_fee:{}",
        results.token_0_amount,
        results.token_1_amount,
        receive_token_0_amount,
        token_0_transfer_fee,
        receive_token_1_amount,
        token_1_transfer_fee
    );
    emit!(LpChangeEvent {
        pool_id,
        lp_amount_before: pool_state.lp_supply,
        token_0_vault_before: total_token_0_amount,
        token_1_vault_before: total_token_1_amount,
        token_0_amount: receive_token_0_amount,
        token_1_amount: receive_token_1_amount,
        token_0_transfer_fee,
        token_1_transfer_fee,
        change_type: 1
    });

    if receive_token_0_amount < minimum_token_0_amount
        || receive_token_1_amount < minimum_token_1_amount
    {
        return Err(ErrorCode::ExceededSlippage.into());
    }

    transfer_ctoken_from_user_to_pool_vault(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.owner_lp_token.to_account_info(),
        ctx.accounts.lp_vault.to_account_info(),
        lp_token_amount,
    )?;

    pool_state.lp_supply = pool_state.lp_supply.checked_sub(lp_token_amount).unwrap();

    let (compressed_token_0_pool_bump, compressed_token_1_pool_bump) = get_bumps(
        ctx.accounts.vault_0_mint.key(),
        ctx.accounts.vault_1_mint.key(),
        ctx.accounts.compressed_token_program.key(),
    );

    transfer_from_pool_vault_to_user(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.token_0_account.to_account_info(),
        Some(ctx.accounts.vault_0_mint.to_account_info()),
        Some(if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        }),
        Some(ctx.accounts.compressed_token_0_pool_pda.to_account_info()),
        Some(compressed_token_0_pool_bump),
        ctx.accounts
            .compressed_token_program_cpi_authority
            .to_account_info(),
        token_0_amount,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
    )?;

    transfer_from_pool_vault_to_user(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_account.to_account_info(),
        Some(ctx.accounts.vault_1_mint.to_account_info()),
        Some(if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        }),
        Some(ctx.accounts.compressed_token_1_pool_pda.to_account_info()),
        Some(compressed_token_1_pool_bump),
        ctx.accounts
            .compressed_token_program_cpi_authority
            .to_account_info(),
        token_1_amount,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
    )?;
    pool_state.recent_epoch = Clock::get()?.epoch;

    // The account was written to, so we must update CompressionInfo.
    pool_state.compression_info_mut().bump_last_written_slot().unwrap();

    Ok(())
}
