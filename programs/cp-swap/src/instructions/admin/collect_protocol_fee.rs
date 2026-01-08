use crate::error::ErrorCode;
use crate::state::*;
use crate::utils::ctoken::get_bumps;
use crate::utils::*;
use anchor_lang::prelude::*;
use anchor_spl::token::Token;
use anchor_spl::token_interface::Mint;
use anchor_spl::token_interface::Token2022;
use anchor_spl::token_interface::TokenAccount;

#[derive(Accounts)]
pub struct CollectProtocolFee<'info> {
    /// Only admin or owner can collect fee now
    #[account(constraint = (owner.key() == amm_config.protocol_owner || owner.key() == crate::admin::ID) @ ErrorCode::InvalidOwner)]
    pub owner: Signer<'info>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// Pool state stores accumulated protocol fee amount
    #[account(mut)]
    pub pool_state: Account<'info, PoolState>,

    /// Amm config account stores owner
    #[account(address = pool_state.amm_config)]
    pub amm_config: Account<'info, AmmConfig>,

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

    /// The address that receives the collected token_0 protocol fees
    #[account(mut)]
    pub recipient_token_0_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that receives the collected token_1 protocol fees
    #[account(mut)]
    pub recipient_token_1_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The SPL program to perform token transfers
    pub token_program: Program<'info, Token>,

    /// The SPL program 2022 to perform token transfers
    pub token_program_2022: Program<'info, Token2022>,

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

pub fn collect_protocol_fee(
    ctx: Context<CollectProtocolFee>,
    amount_0_requested: u64,
    amount_1_requested: u64,
) -> Result<()> {
    let amount_0: u64;
    let amount_1: u64;
    let auth_bump: u8;
    {
        let pool_state = &mut ctx.accounts.pool_state;

        amount_0 = amount_0_requested.min(pool_state.protocol_fees_token_0);
        amount_1 = amount_1_requested.min(pool_state.protocol_fees_token_1);

        pool_state.protocol_fees_token_0 = pool_state
            .protocol_fees_token_0
            .checked_sub(amount_0)
            .unwrap();
        pool_state.protocol_fees_token_1 = pool_state
            .protocol_fees_token_1
            .checked_sub(amount_1)
            .unwrap();

        auth_bump = pool_state.auth_bump;
        pool_state.recent_epoch = Clock::get()?.epoch;
    }

    let (spl_interface_0_bump, spl_interface_1_bump) = get_bumps(
        ctx.accounts.vault_0_mint.key(),
        ctx.accounts.vault_1_mint.key(),
        ctx.accounts.light_token_program.key(),
    );

    transfer_from_pool_vault_to_user(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.recipient_token_0_account.to_account_info(),
        Some(ctx.accounts.vault_0_mint.to_account_info()),
        Some(if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        }),
        Some(ctx.accounts.spl_interface_0_pda.to_account_info()),
        Some(spl_interface_0_bump),
        ctx.accounts.light_token_program_cpi_authority.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        amount_0,
        ctx.accounts.vault_0_mint.decimals,
        &[&[crate::AUTH_SEED.as_bytes(), &[auth_bump]]],
    )?;

    transfer_from_pool_vault_to_user(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.recipient_token_1_account.to_account_info(),
        Some(ctx.accounts.vault_1_mint.to_account_info()),
        Some(if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        }),
        Some(ctx.accounts.spl_interface_1_pda.to_account_info()),
        Some(spl_interface_1_bump),
        ctx.accounts.light_token_program_cpi_authority.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
        amount_1,
        ctx.accounts.vault_1_mint.decimals,
        &[&[crate::AUTH_SEED.as_bytes(), &[auth_bump]]],
    )?;

    Ok(())
}
