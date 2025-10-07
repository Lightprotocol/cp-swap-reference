use crate::error::ErrorCode;
use anchor_lang::prelude::*;
use anchor_spl::{token::Token, token_interface::Mint};
use light_compressed_token_sdk::instructions::transfer2::{
    transfer_ctoken_to_spl_signed, transfer_spl_to_ctoken,
};
use spl_token_2022::{
    self,
    extension::{
        transfer_fee::{TransferFeeConfig, MAX_FEE_BASIS_POINTS},
        BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
};
use std::collections::HashSet;

const MINT_WHITELIST: [&'static str; 4] = [
    "HVbpJAQGNpkgBaYBZQBR1t7yFdvaYVp2vCQQfKKEN4tM",
    "Crn4x1Y2HUKko7ox2EZMT6N2t2ZyH7eKtwkBGVnhEq1g",
    "FrBfWJ4qE5sCzKm3k3JaAtqZcXUh4LvJygDeketsrsH4",
    "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo",
];

pub fn transfer_from_user_to_pool_vault<'a, 'b>(
    authority: AccountInfo<'a>,
    from: AccountInfo<'a>,
    to_vault: AccountInfo<'a>,
    mint: AccountInfo<'a>,
    spl_token_program: AccountInfo<'a>,
    compressed_token_pool_pda: AccountInfo<'a>,
    compressed_token_pool_pda_bump: u8,
    compressed_token_program_authority: AccountInfo<'a>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    transfer_spl_to_ctoken(
        authority.clone(),
        authority,
        from,
        to_vault,
        mint,
        spl_token_program,
        compressed_token_pool_pda,
        compressed_token_pool_pda_bump,
        compressed_token_program_authority,
        amount,
    )?;
    Ok(())
}

pub fn transfer_from_pool_vault_to_user<'a>(
    payer: AccountInfo<'a>,
    authority: AccountInfo<'a>,
    from_vault: AccountInfo<'a>,
    to: AccountInfo<'a>,
    mint: AccountInfo<'a>,
    spl_token_program: AccountInfo<'a>,
    compressed_token_pool_pda: AccountInfo<'a>,
    compressed_token_pool_pda_bump: u8,
    compressed_token_program_authority: AccountInfo<'a>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    transfer_ctoken_to_spl_signed(
        payer,
        authority,
        from_vault,
        to,
        mint,
        spl_token_program,
        compressed_token_pool_pda,
        compressed_token_pool_pda_bump,
        compressed_token_program_authority,
        amount,
        signer_seeds,
    )?;
    Ok(())
}

/// Calculate the fee for output amount
pub fn get_transfer_inverse_fee(mint_info: &AccountInfo, post_fee_amount: u64) -> Result<u64> {
    if *mint_info.owner == anchor_lang::solana_program::system_program::ID {
        return Ok(0);
    }
    if *mint_info.owner == Token::id() {
        return Ok(0);
    }
    if post_fee_amount == 0 {
        return err!(ErrorCode::InvalidInput);
    }
    let mint_data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    let fee = if let Ok(transfer_fee_config) = mint.get_extension::<TransferFeeConfig>() {
        let epoch = Clock::get()?.epoch;

        let transfer_fee = transfer_fee_config.get_epoch_fee(epoch);
        if u16::from(transfer_fee.transfer_fee_basis_points) == MAX_FEE_BASIS_POINTS {
            u64::from(transfer_fee.maximum_fee)
        } else {
            let transfer_fee = transfer_fee_config
                .calculate_inverse_epoch_fee(epoch, post_fee_amount)
                .unwrap();
            let transfer_fee_for_check = transfer_fee_config
                .calculate_epoch_fee(epoch, post_fee_amount.checked_add(transfer_fee).unwrap())
                .unwrap();
            if transfer_fee != transfer_fee_for_check {
                return err!(ErrorCode::TransferFeeCalculateNotMatch);
            }
            transfer_fee
        }
    } else {
        0
    };
    Ok(fee)
}

/// Calculate the fee for input amount
pub fn get_transfer_fee(mint_info: &AccountInfo, pre_fee_amount: u64) -> Result<u64> {
    if *mint_info.owner == anchor_lang::solana_program::system_program::ID {
        return Ok(0);
    }
    if *mint_info.owner == Token::id() {
        return Ok(0);
    }
    let mint_data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    let fee = if let Ok(transfer_fee_config) = mint.get_extension::<TransferFeeConfig>() {
        transfer_fee_config
            .calculate_epoch_fee(Clock::get()?.epoch, pre_fee_amount)
            .unwrap()
    } else {
        0
    };
    Ok(fee)
}

pub fn is_supported_mint(mint_account: &InterfaceAccount<Mint>) -> Result<bool> {
    let mint_info = mint_account.to_account_info();

    if *mint_info.owner == anchor_lang::solana_program::system_program::ID {
        return Ok(true);
    }

    if *mint_info.owner == Token::id() {
        return Ok(true);
    }
    let mint_whitelist: HashSet<&str> = MINT_WHITELIST.into_iter().collect();
    if mint_whitelist.contains(mint_account.key().to_string().as_str()) {
        return Ok(true);
    }
    let mint_data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;
    let extensions = mint.get_extension_types()?;
    for e in extensions {
        if e != ExtensionType::TokenMetadata {
            return Ok(false);
        }
    }
    Ok(true)
}
