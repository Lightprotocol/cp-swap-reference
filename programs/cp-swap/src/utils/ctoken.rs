use anchor_lang::prelude::*;
use light_token_sdk::token::TransferCpi;

pub fn transfer_ctoken_from_user_to_pool_vault<'a>(
    authority: AccountInfo<'a>,
    source: AccountInfo<'a>,
    destination: AccountInfo<'a>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    TransferCpi {
        source,
        destination,
        authority,
        amount,
        max_top_up: None,
    }
    .invoke()
    .map_err(Into::into)
}

pub fn transfer_ctoken_from_pool_vault_to_user<'a>(
    authority: AccountInfo<'a>,
    source: AccountInfo<'a>,
    destination: AccountInfo<'a>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    TransferCpi {
        source,
        destination,
        authority,
        amount,
        max_top_up: None,
    }
    .invoke_signed(signer_seeds)
    .map_err(Into::into)
}
