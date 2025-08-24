use crate::{
    instructions::InitializeCompressionParams,
    states::POOL_LP_MINT_SEED,
    utils::{create_or_allocate_account, LP_MINT_CREATION_INDEX},
};
use anchor_lang::{
    prelude::*,
    solana_program::program::{invoke, invoke_signed},
};
use light_compressed_token_sdk::{
    instructions::{
        create_compressible_associated_token_account_with_bump as initialize_compressible_associated_token_account_with_bump,
        create_compressible_token_account as initialize_compressible_token_account,
        create_mint_action_cpi, derive_compressed_mint_address, transfer, transfer_signed,
        CreateCompressibleAssociatedTokenAccountInputs, CreateCompressibleTokenAccount,
        MintActionInputs, MintActionType,
    },
    CompressedProof,
};
use light_ctoken_types::{
    instructions::mint_action::CompressedMintWithContext,
    instructions::mint_action::CpiContext as CompressedCpiContext, COMPRESSIBLE_TOKEN_ACCOUNT_SIZE,
};
use light_sdk::cpi::CpiAccountsSmall;

pub fn transfer_ctoken_from_user_to_pool_vault<'a>(
    authority: AccountInfo<'a>,
    from: AccountInfo<'a>,
    to_vault: AccountInfo<'a>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }

    transfer(&from, &to_vault, &authority, amount)?;
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
    transfer_signed(&from_vault, &to, &authority, amount, signer_seeds)?;
    Ok(())
}

pub fn create_compressible_token_account<'a>(
    authority: &AccountInfo<'a>,
    payer: &AccountInfo<'a>,
    token_account: &AccountInfo<'a>,
    mint_account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    signer_seeds: &[&[u8]],
    rent_authority: &AccountInfo<'a>,
    rent_recipient: &AccountInfo<'a>,
    slots_until_compression: u64,
) -> Result<()> {
    // Note this does not support token account extensions.
    let space = COMPRESSIBLE_TOKEN_ACCOUNT_SIZE as usize;

    create_or_allocate_account(
        token_program.key,
        payer.to_account_info(),
        system_program.to_account_info(),
        token_account.to_account_info(),
        signer_seeds,
        space,
    )?;

    let init_ix = initialize_compressible_token_account(CreateCompressibleTokenAccount {
        account_pubkey: *token_account.key,
        mint_pubkey: *mint_account.key,
        owner_pubkey: *authority.key,
        rent_authority: *rent_authority.key,
        rent_recipient: *rent_recipient.key,
        slots_until_compression,
    })
    .map_err(|e| ProgramError::from(e))?;

    invoke(
        &init_ix,
        &[
            token_account.to_account_info(),
            mint_account.to_account_info(),
            authority.to_account_info(),
            rent_authority.to_account_info(),
            rent_recipient.to_account_info(),
        ],
    )?;

    Ok(())
}

pub fn create_compressible_associated_token_account<'a>(
    owner: &AccountInfo<'a>,
    payer: &AccountInfo<'a>,
    associated_token_account: &AccountInfo<'a>,
    mint_account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    rent_authority: &AccountInfo<'a>,
    rent_recipient: &AccountInfo<'a>,
    slots_until_compression: u64,
    bump: u8,
) -> Result<()> {
    let init_ix = initialize_compressible_associated_token_account_with_bump(
        CreateCompressibleAssociatedTokenAccountInputs {
            payer: *payer.key,
            mint: *mint_account.key,
            owner: *owner.key,
            rent_authority: *rent_authority.key,
            rent_recipient: *rent_recipient.key,
            slots_until_compression,
        },
        *associated_token_account.key,
        bump,
    )
    .map_err(|e| ProgramError::from(e))?;

    invoke(
        &init_ix,
        &[
            payer.to_account_info(),
            associated_token_account.to_account_info(),
            mint_account.to_account_info(),
            owner.to_account_info(),
            system_program.to_account_info(),
        ],
    )?;

    Ok(())
}

pub fn create_and_mint_lp<'a, 'b, 'info>(
    creator: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    lp_mint_key: &Pubkey,
    lp_vault: AccountInfo<'info>,
    creator_lp_token: AccountInfo<'info>,
    lp_mint_signer: AccountInfo<'info>,
    pool_state_key: &Pubkey,
    compressed_token_program_cpi_authority: AccountInfo<'info>,
    compressed_token_program: AccountInfo<'info>,
    lp_mint_signer_bump: u8,
    compression_params: &InitializeCompressionParams,
    cpi_accounts: &CpiAccountsSmall<'b, 'info>,
    user_lp_amount: u64,
    vault_lp_amount: u64,
    pool_auth_bump: u8,
) -> Result<()> {
    // Get tree accounts
    let output_state_queue_idx: u8 = 0;
    let address_tree_idx: u8 = 1;
    let output_state_queue =
        *cpi_accounts.tree_accounts().unwrap()[output_state_queue_idx as usize].key;
    let address_tree_pubkey = *cpi_accounts.tree_accounts().unwrap()[address_tree_idx as usize].key;

    let compressed_mint_with_context = CompressedMintWithContext::new(
        derive_compressed_mint_address(lp_mint_key, &address_tree_pubkey),
        compression_params.lp_mint_address_tree_info.root_index,
        9, // Our Lp mints always have 9 decimals.
        Some(authority.key().into()),
        Some(authority.key().into()),
        lp_mint_key.into(),
    );

    // The cmint creation is implicit. Here we additionally
    // mint to the creator and the pool vault.
    let actions = vec![
        MintActionType::MintToDecompressed {
            account: creator_lp_token.key(),
            amount: user_lp_amount,
        },
        MintActionType::MintToDecompressed {
            account: lp_vault.key(),
            amount: vault_lp_amount,
        },
    ];

    let mint_action_instruction: anchor_lang::solana_program::instruction::Instruction =
        create_mint_action_cpi(
            MintActionInputs::new_for_create_mint(
                compressed_mint_with_context,
                actions,
                output_state_queue,
                address_tree_pubkey,
                lp_mint_signer.key(),
                Some(compression_params.lp_mint_bump),
                authority.key().into(),
                creator.key(),
                compression_params.proof.0.map(|p| CompressedProof::from(p)),
            ),
            Some(CompressedCpiContext::last_cpi_create_mint(
                address_tree_idx,
                output_state_queue_idx,
                LP_MINT_CREATION_INDEX,
            )),
            Some(cpi_accounts.cpi_context().unwrap().key()),
        )
        .map_err(|e| ProgramError::from(e))?;

    // Extend the account infos with the accounts needed for the cmint creation.
    let mut account_infos = cpi_accounts.to_account_infos();
    account_infos.extend([
        compressed_token_program_cpi_authority,
        compressed_token_program,
        authority,
        lp_mint_signer,
        creator,
        // accounts used by the additional mint actions:
        lp_vault,
        creator_lp_token,
    ]);

    // Invoke. We batch settle all compression related CPIs here, hence the
    // signer_seeds for the PDAs.
    invoke_signed(
        &mint_action_instruction,
        &account_infos,
        &[
            // The mint creation is checked before the PDA actions, so its
            // signer_seeds come first.
            &[
                POOL_LP_MINT_SEED.as_bytes(),
                pool_state_key.as_ref(),
                &[lp_mint_signer_bump],
            ],
            // Since we also create 2 compressed PDAs in our instruction cia
            // cpi_context, now we need to settle via the pda authority's
            // signer_seeds.
            &[crate::AUTH_SEED.as_bytes(), &[pool_auth_bump]],
        ],
    )?;

    Ok(())
}
