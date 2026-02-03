use crate::states::*;
use anchor_lang::{accounts::account_loader::AccountLoader, prelude::*};

#[derive(Accounts)]
pub struct UpdatePoolStatus<'info> {
    #[account(
        address = crate::admin::ID
    )]
    pub authority: Signer<'info>,

    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,
}

pub fn update_pool_status(ctx: Context<UpdatePoolStatus>, status: u8) -> Result<()> {
    require_gte!(255, status);
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    pool_state.set_status(status);
    pool_state.recent_epoch = Clock::get()?.epoch;
    Ok(())
}
