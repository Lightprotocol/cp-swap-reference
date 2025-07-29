use crate::instructions::InitializeCompressionParams;
use crate::states::*;
use anchor_lang::prelude::*;
use light_sdk::compressible::prepare_accounts_for_compression_on_init;
use light_sdk::cpi::CpiAccountsSmall;
use light_sdk::cpi::CpiInputs;
use light_sdk_types::cpi_context_write::CpiContextWriteAccounts;

use crate::LIGHT_CPI_SIGNER;

// The order in which new compressed accounts are passed to the CPI.
pub const POOL_STATE_CREATION_INDEX: u8 = 0;
pub const OBSERVATION_STATE_CREATION_INDEX: u8 = 1;
pub const LP_MINT_CREATION_INDEX: u8 = 2;

pub fn compress_pool_and_observation_pdas<'a, 'b, 'info>(
    cpi_accounts: &CpiAccountsSmall<'b, 'info>,
    pool_state: &Account<'info, PoolState>,
    observation_state: &Account<'info, ObservationState>,
    compression_params: &InitializeCompressionParams,
    rent_recipient: &AccountInfo<'info>,
    address_space: &[Pubkey],
) -> Result<()> {
    // 1. Prepare new address params
    let pool_new_address_params = compression_params
        .pool_address_tree_info
        .into_new_address_params_assigned_packed(
            pool_state.key().to_bytes(),
            true,
            Some(POOL_STATE_CREATION_INDEX),
        );
    let observation_new_address_params = compression_params
        .observation_address_tree_info
        .into_new_address_params_assigned_packed(
            observation_state.key().to_bytes(),
            true,
            Some(OBSERVATION_STATE_CREATION_INDEX),
        );

    // 2. Prepare accounts for compression
    // prepare_accounts_for_compression_on_init is for direct compression. To
    // write to the accounts, as user then has to add a
    // decompress_accounts_idempotent instruction to the first transaction. Many
    // users can try to decompress the account concurrently. If you instead want
    // to start in a "decompressed" state, and only compress later, use
    // 'compress_empty_account_on_init'.
    let mut all_compressed_infos = Vec::with_capacity(2);

    let pool_state_compressed_info = prepare_accounts_for_compression_on_init::<PoolState>(
        &[pool_state],
        &[compression_params.pool_compressed_address],
        &[pool_new_address_params],
        &[compression_params.output_state_tree_index],
        &cpi_accounts,
        &address_space,
        rent_recipient,
    )?;
    all_compressed_infos.extend(pool_state_compressed_info);

    let observation_compressed_infos = prepare_accounts_for_compression_on_init::<ObservationState>(
        &[observation_state],
        &[compression_params.observation_compressed_address],
        &[observation_new_address_params],
        &[compression_params.output_state_tree_index],
        &cpi_accounts,
        &address_space,
        rent_recipient,
    )?;
    all_compressed_infos.extend(observation_compressed_infos);

    // 3. Compress. We invoke the cpi_context here to save CU, because we still
    // create a cmint later in the instruction. Only then will the state
    // transition be fully settled. Notice we're using 'new_first_cpi' here
    // instead of 'CompressedCpiContext::last_cpi_create_mint'.
    let cpi_inputs = CpiInputs::new_first_cpi(
        all_compressed_infos,
        vec![pool_new_address_params, observation_new_address_params],
    );
    let cpi_context = cpi_accounts.cpi_context().unwrap();
    let cpi_context_accounts = CpiContextWriteAccounts {
        fee_payer: cpi_accounts.fee_payer(),
        authority: cpi_accounts.authority().unwrap(),
        cpi_context,
        cpi_signer: LIGHT_CPI_SIGNER,
    };
    cpi_inputs.invoke_light_system_program_cpi_context(cpi_context_accounts)?;

    Ok(())
}
