use anchor_lang::prelude::*;
use light_token_sdk::token::{TransferCpi, TransferSignedCpi};

pub fn transfer_ctoken_from_user_to_pool_vault<'a>(
    authority: AccountInfo<'a>,
    from: AccountInfo<'a>,
    to_vault: AccountInfo<'a>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    TransferCpi {
        from,
        to: to_vault,
        authority,
        amount,
    }
    .invoke()?;
    Ok(())
}

pub fn transfer_ctoken_from_pool_vault_to_user<'a>(
    authority: AccountInfo<'a>,
    from_vault: AccountInfo<'a>,
    to: AccountInfo<'a>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    TransferSignedCpi {
        from: from_vault,
        to,
        authority,
        amount,
    }
    .invoke_signed(signer_seeds)?;
    Ok(())
}
