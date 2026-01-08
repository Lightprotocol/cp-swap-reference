use anchor_lang::prelude::*;
use light_ctoken_sdk::ctoken::TransferInterfaceCpi;

pub fn transfer_ctoken_from_user_to_pool_vault<'a>(
    authority: AccountInfo<'a>,
    from: AccountInfo<'a>,
    to_vault: AccountInfo<'a>,
    amount: u64,
    decimals: u8,
    ctoken_program_authority: AccountInfo<'a>,
    system_program: AccountInfo<'a>,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }

    TransferInterfaceCpi::new(
        amount,
        decimals,
        from,
        to_vault,
        authority.clone(),
        authority.clone(),
        ctoken_program_authority,
        system_program,
    )
    .invoke()
    .map_err(|e| anchor_lang::error::Error::from(e))?;
    Ok(())
}

pub fn transfer_ctoken_from_pool_vault_to_user<'a>(
    authority: AccountInfo<'a>,
    from_vault: AccountInfo<'a>,
    to: AccountInfo<'a>,
    amount: u64,
    decimals: u8,
    ctoken_program_authority: AccountInfo<'a>,
    system_program: AccountInfo<'a>,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    TransferInterfaceCpi::new(
        amount,
        decimals,
        from_vault,
        to,
        authority.clone(),
        authority.clone(),
        ctoken_program_authority,
        system_program,
    )
    .invoke_signed(signer_seeds)
    .map_err(|e| anchor_lang::error::Error::from(e))?;
    Ok(())
}

// To reduce CU usage, you can instead also pass the bumps as instruction data.
pub fn get_bumps(
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    light_token_program: Pubkey,
) -> (u8, u8) {
    let spl_interface_0_bump = Pubkey::find_program_address(
        &[b"pool".as_ref(), token_0_mint.as_ref()],
        &light_token_program,
    )
    .1;
    let spl_interface_1_bump = Pubkey::find_program_address(
        &[b"pool".as_ref(), token_1_mint.as_ref()],
        &light_token_program,
    )
    .1;
    (spl_interface_0_bump, spl_interface_1_bump)
}
