use anchor_lang::prelude::*;

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
