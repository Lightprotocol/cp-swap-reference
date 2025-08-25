use anchor_lang::AnchorDeserialize;
use anyhow::Result;
use light_client::{
    indexer::Indexer,
    rpc::{LightClient, Rpc},
};
use light_compressible_client::CompressibleInstruction;
use light_sdk::compressible::CompressibleConfig;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::str::FromStr;

use super::super::{read_keypair_file, ClientConfig};
use super::rpc::send_versioned_txn;
use light_client::rpc::load_lookup_table;

pub const COMPRESSION_DELAY: u32 = 100;
pub const ADDRESS_SPACE: [Pubkey; 1] = [solana_sdk::pubkey!(
    "EzKE84aVTkCUhDHLELqyJaq1Y7UVVmqxXqZjVHwHY3rK"
)];

pub async fn initialize_compression_config(
    rpc_client: &RpcClient,
    config: &ClientConfig,
    authority: &Keypair,
) -> Result<()> {
    let payer = read_keypair_file(&config.payer_path)?;
    let program_id = config.raydium_cp_program;

    let (config_pda, _) = CompressibleConfig::derive_default_pda(&program_id);
    if rpc_client.get_account(&config_pda).is_ok() {
        return Ok(());
    }

    let instruction = CompressibleInstruction::initialize_compression_config(
        &program_id,
        &CompressibleInstruction::INITIALIZE_COMPRESSION_CONFIG_DISCRIMINATOR,
        &payer.pubkey(),
        &authority.pubkey(),
        COMPRESSION_DELAY,
        payer.pubkey(), // rent_recipient
        ADDRESS_SPACE.to_vec(),
        None,
    );

    let lookup_table_address = Pubkey::from_str("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")?;
    let lookup_table = load_lookup_table(&rpc_client, &lookup_table_address)?;
    let lookup_tables = vec![lookup_table];

    let signers = vec![&payer, authority];
    let _signature = send_versioned_txn(
        &rpc_client,
        &[instruction],
        &signers,
        &payer.pubkey(),
        &lookup_tables,
        true,
    )?;

    Ok(())
}

pub async fn decompress_pool_and_observation_idempotent(
    light_client: &mut LightClient,
    rpc_client: &RpcClient,
    config: &ClientConfig,
    pool_address: Pubkey,
    observation_address: Pubkey,
    amm_config: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
) -> Result<()> {
    let payer = read_keypair_file(&config.payer_path)?;
    let program_id = config.raydium_cp_program;

    let pool_exists = rpc_client.get_account(&pool_address).is_ok();
    let observation_exists = rpc_client.get_account(&observation_address).is_ok();

    if pool_exists && observation_exists {
        return Ok(());
    }

    let address_tree_info = light_client.get_address_tree_v2();

    let pool_compressed_address = light_compressed_account::address::derive_address(
        &pool_address.to_bytes(),
        &address_tree_info.tree.to_bytes(),
        &program_id.to_bytes(),
    );

    let observation_compressed_address = light_compressed_account::address::derive_address(
        &observation_address.to_bytes(),
        &address_tree_info.tree.to_bytes(),
        &program_id.to_bytes(),
    );

    let pool_compressed_accounts = light_client
        .get_compressed_accounts_by_owner(&program_id, None, None)
        .await?;

    let pool_compressed = pool_compressed_accounts
        .value
        .items
        .iter()
        .find(|acc| acc.address == Some(pool_compressed_address))
        .cloned();

    let observation_compressed = pool_compressed_accounts
        .value
        .items
        .iter()
        .find(|acc| acc.address == Some(observation_compressed_address))
        .cloned();

    if pool_compressed.is_none() && observation_compressed.is_none() {
        if rpc_client.get_account(&pool_address).is_err() {
            return Err(anyhow::anyhow!("Onchain pool account does not exist"));
        }
        if rpc_client.get_account(&observation_address).is_err() {
            return Err(anyhow::anyhow!(
                "Onchain observation account does not exist"
            ));
        }

        return Ok(());
    }

    let pool_compressed =
        pool_compressed.ok_or_else(|| anyhow::anyhow!("Pool compressed account not found"))?;
    let observation_compressed = observation_compressed
        .ok_or_else(|| anyhow::anyhow!("Observation compressed account not found"))?;

    let validity_proof_result = light_client
        .get_validity_proof(
            vec![
                pool_compressed.hash.clone(),
                observation_compressed.hash.clone(),
            ],
            vec![],
            None,
        )
        .await?
        .value;

    let (_, pool_bump) = Pubkey::find_program_address(
        &[
            raydium_cp_swap::states::POOL_SEED.as_bytes(),
            amm_config.as_ref(),
            token_0_mint.as_ref(),
            token_1_mint.as_ref(),
        ],
        &program_id,
    );

    let (_, observation_bump) = Pubkey::find_program_address(
        &[
            raydium_cp_swap::states::OBSERVATION_SEED.as_bytes(),
            pool_address.as_ref(),
        ],
        &program_id,
    );

    let state_tree_info = light_client.get_state_tree_infos()[0];

    let pool_data = pool_compressed
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Pool compressed account has no data"))?;
    let pool_state = raydium_cp_swap::states::PoolState::deserialize(&mut &pool_data.data[..])?;

    let observation_data = observation_compressed
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Observation compressed account has no data"))?;
    let observation_state =
        raydium_cp_swap::states::ObservationState::deserialize(&mut &observation_data.data[..])?;

    let decompress_instr =
        light_compressible_client::CompressibleInstruction::decompress_accounts_idempotent(
            &program_id,
            &CompressibleInstruction::DECOMPRESS_ACCOUNTS_IDEMPOTENT_DISCRIMINATOR,
            &payer.pubkey(),
            &payer.pubkey(),
            &[pool_address, observation_address],
            &[
                (
                    pool_compressed.clone(),
                    raydium_cp_swap::raydium_cp_swap::CompressedAccountVariant::PoolState(
                        pool_state,
                    ),
                    vec![
                        raydium_cp_swap::states::POOL_SEED.as_bytes().to_vec(),
                        amm_config.to_bytes().to_vec(),
                        token_0_mint.to_bytes().to_vec(),
                        token_1_mint.to_bytes().to_vec(),
                    ],
                ),
                (
                    observation_compressed.clone(),
                    raydium_cp_swap::raydium_cp_swap::CompressedAccountVariant::ObservationState(
                        observation_state,
                    ),
                    vec![
                        raydium_cp_swap::states::OBSERVATION_SEED
                            .as_bytes()
                            .to_vec(),
                        pool_address.to_bytes().to_vec(),
                    ],
                ),
            ],
            &[pool_bump, observation_bump],
            validity_proof_result,
            state_tree_info,
        )
        .map_err(|e| anyhow::anyhow!("Failed to build decompress instruction: {}", e))?;

    let lookup_table_address = Pubkey::from_str("9NYFyEqPkyXUhkerbGHXUXkvb4qpzeEdHuGpgbgpH1NJ")?;
    let lookup_table = load_lookup_table(&rpc_client, &lookup_table_address)?;
    let lookup_tables = vec![lookup_table];

    let signers = vec![&payer];
    send_versioned_txn(
        &rpc_client,
        &[decompress_instr],
        &signers,
        &payer.pubkey(),
        &lookup_tables,
        true,
    )?;

    Ok(())
}
