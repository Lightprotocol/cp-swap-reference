use anchor_client::{Client, Cluster};
use anchor_spl::{
    associated_token::spl_associated_token_account, memo::spl_memo, token::spl_token,
    token_2022::spl_token_2022,
};
use anyhow::Result;
use light_client::{
    indexer::{AddressWithTree, Indexer},
    rpc::{LightClient, Rpc},
};
use light_compressed_account::address::derive_address;
use light_sdk::{
    compressible::CompressibleConfig,
    instruction::{PackedAccounts, SystemAccountMetaConfig},
};
use light_token_client::compressed_token::{self, derive_compressed_mint_address};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, system_program, sysvar};
use std::rc::Rc;

use raydium_cp_swap::{accounts as raydium_cp_accounts, utils::POOL_STATE_CREATION_INDEX};
use raydium_cp_swap::{
    instruction as raydium_cp_instructions,
    utils::{LP_MINT_CREATION_INDEX, OBSERVATION_STATE_CREATION_INDEX},
};
use raydium_cp_swap::{
    states::{AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_LP_MINT_SEED, POOL_SEED, POOL_VAULT_SEED},
    AUTH_SEED,
};

use super::super::{read_keypair_file, ClientConfig};

pub async fn initialize_pool_instr(
    light_client: &mut LightClient,
    config: &ClientConfig,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_0_program: Pubkey,
    token_1_program: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    create_pool_fee: Pubkey,
    random_pool_id: Option<Pubkey>,
    init_amount_0: u64,
    init_amount_1: u64,
    open_time: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path)?;
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());

    let client = Client::new(url, Rc::new(payer));
    let program = client.program(config.raydium_cp_program)?;

    let amm_config_index = 0u16;
    let (amm_config_key, __bump) = Pubkey::find_program_address(
        &[AMM_CONFIG_SEED.as_bytes(), &amm_config_index.to_be_bytes()],
        &program.id(),
    );

    let pool_account_key = if random_pool_id.is_some() {
        random_pool_id.unwrap()
    } else {
        Pubkey::find_program_address(
            &[
                POOL_SEED.as_bytes(),
                amm_config_key.to_bytes().as_ref(),
                token_0_mint.to_bytes().as_ref(),
                token_1_mint.to_bytes().as_ref(),
            ],
            &program.id(),
        )
        .0
    };

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let (token_0_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            token_0_mint.to_bytes().as_ref(),
        ],
        &program.id(),
    );
    let (token_1_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            token_1_mint.to_bytes().as_ref(),
        ],
        &program.id(),
    );

    let (lp_mint_signer, _lp_mint_signer_bump) = Pubkey::find_program_address(
        &[
            POOL_LP_MINT_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
        ],
        &program.id(),
    );

    let (lp_mint_key, lp_mint_bump) = compressed_token::find_mint_address(lp_mint_signer);

    let (lp_vault, __bump) = Pubkey::find_program_address(
        &[POOL_VAULT_SEED.as_bytes(), lp_mint_key.to_bytes().as_ref()],
        &program.id(),
    );
    let (observation_key, __bump) = Pubkey::find_program_address(
        &[
            OBSERVATION_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
        ],
        &program.id(),
    );

    let compression_config_key = CompressibleConfig::derive_default_pda(&program.id()).0;
    let mut remaining_accounts = PackedAccounts::default();
    let address_tree_info = light_client.get_address_tree_v2();
    let state_tree_info = light_client.get_state_tree_infos()[0];
    remaining_accounts.add_system_accounts_small(SystemAccountMetaConfig::new_with_cpi_context(
        program.id(),
        state_tree_info.cpi_context.unwrap(),
    ))?;

    // Derive compressed addresses of all to-be-initialized compressible accounts.
    let pool_compressed_address = derive_address(
        &pool_account_key.to_bytes(),
        &address_tree_info.tree.to_bytes(),
        &program.id().to_bytes(),
    );
    let observation_compressed_address = derive_address(
        &observation_key.to_bytes(),
        &address_tree_info.tree.to_bytes(),
        &program.id().to_bytes(),
    );
    let lp_mint_compressed_address =
        derive_compressed_mint_address(lp_mint_key, &address_tree_info.tree);

    // Fetch validity proof for all new compressed addresses. Proves that the
    // accounts don't exist yet. Must match the ordering used by the program
    // when invoking the cpi.
    let rpc_result = light_client
        .get_validity_proof(
            vec![],
            vec![
                AddressWithTree {
                    address: pool_compressed_address,
                    tree: address_tree_info.tree,
                },
                AddressWithTree {
                    address: observation_compressed_address,
                    tree: address_tree_info.tree,
                },
                AddressWithTree {
                    address: lp_mint_compressed_address,
                    tree: address_tree_info.tree,
                },
            ],
            None,
        )
        .await
        .unwrap()
        .value;

    let output_state_tree_index = remaining_accounts.insert_or_get(state_tree_info.queue);
    let packed_tree_infos = rpc_result.pack_tree_infos(&mut remaining_accounts);
    let pool_address_tree_info =
        packed_tree_infos.address_trees[POOL_STATE_CREATION_INDEX as usize];
    let observation_address_tree_info =
        packed_tree_infos.address_trees[OBSERVATION_STATE_CREATION_INDEX as usize];
    let lp_mint_address_tree_info =
        packed_tree_infos.address_trees[LP_MINT_CREATION_INDEX as usize];

    let (system_accounts, _, _) = remaining_accounts.to_account_metas();

    let (creator_lp_token, creator_lp_token_bump) =
        compressed_token::get_associated_ctoken_address_and_bump(&program.payer(), &lp_mint_key);

    let compression_params =
        raydium_cp_swap::instructions::initialize::InitializeCompressionParams {
            pool_address_tree_info,
            observation_address_tree_info,
            lp_mint_address_tree_info,
            lp_mint_bump,
            proof: rpc_result.proof.into(),
            output_state_tree_index,
            creator_lp_token_bump,
        };

    let (compressed_token_0_pool_pda, token_0_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_0_mint);
    let (compressed_token_1_pool_pda, token_1_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_1_mint);

    let mut instructions = program
        .request()
        .accounts(raydium_cp_accounts::Initialize {
            creator: program.payer(),
            amm_config: amm_config_key,
            authority,
            pool_state: pool_account_key,
            token_0_mint,
            token_1_mint,
            lp_mint: lp_mint_key,
            lp_vault,
            creator_token_0: user_token_0_account,
            creator_token_1: user_token_1_account,
            creator_lp_token,
            token_0_vault,
            token_1_vault,
            create_pool_fee,
            observation_state: observation_key,
            token_program: spl_token::id(),
            token_0_program,
            token_1_program,
            associated_token_program: spl_associated_token_account::id(),
            system_program: system_program::id(),
            rent: sysvar::rent::id(),
            compression_config: compression_config_key,
            rent_recipient: program.payer(),
            lp_mint_signer,
            compressed_token_program_cpi_authority: compressed_token::cpi_authority(),
            compressed_token_program: compressed_token::id(),
            compressed_token_0_pool_pda,
            compressed_token_1_pool_pda,
        })
        .accounts(system_accounts)
        .args(raydium_cp_instructions::Initialize {
            init_amount_0,
            init_amount_1,
            open_time,
            compression_params,
        })
        .instructions()?;

    if random_pool_id.is_some() {
        // update account signer as true for random pool
        for account in instructions[0].accounts.iter_mut() {
            if account.pubkey == random_pool_id.unwrap() {
                account.is_signer = true;
                break;
            }
        }
    }

    Ok(instructions)
}

pub fn deposit_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_lp_mint: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    user_token_lp_account: Pubkey,
    lp_token_amount: u64,
    maximum_token_0_amount: u64,
    maximum_token_1_amount: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path)?;
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let (lp_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            token_lp_mint.to_bytes().as_ref(),
        ],
        &program.id(),
    );

    let (compressed_token_0_pool_pda, _token_0_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_0_mint);
    let (compressed_token_1_pool_pda, _token_1_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_1_mint);

    let instructions = program
        .request()
        .accounts(raydium_cp_accounts::Deposit {
            owner: program.payer(),
            authority,
            pool_state: pool_id,
            owner_lp_token: user_token_lp_account,
            token_0_account: user_token_0_account,
            token_1_account: user_token_1_account,
            token_0_vault,
            token_1_vault,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            vault_0_mint: token_0_mint,
            vault_1_mint: token_1_mint,
            lp_vault,
            compressed_token_program: compressed_token::id(),
            compressed_token_program_cpi_authority: compressed_token::cpi_authority(),
            compressed_token_0_pool_pda,
            compressed_token_1_pool_pda,
        })
        .args(raydium_cp_instructions::Deposit {
            lp_token_amount,
            maximum_token_0_amount,
            maximum_token_1_amount,
        })
        .instructions()?;
    Ok(instructions)
}

pub fn withdraw_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_lp_mint: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    user_token_lp_account: Pubkey,
    lp_token_amount: u64,
    minimum_token_0_amount: u64,
    minimum_token_1_amount: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path)?;
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let (lp_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            token_lp_mint.to_bytes().as_ref(),
        ],
        &program.id(),
    );
    let (compressed_token_0_pool_pda, _token_0_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_0_mint);
    let (compressed_token_1_pool_pda, _token_1_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&token_1_mint);

    let instructions = program
        .request()
        .accounts(raydium_cp_accounts::Withdraw {
            owner: program.payer(),
            authority,
            pool_state: pool_id,
            owner_lp_token: user_token_lp_account,
            token_0_account: user_token_0_account,
            token_1_account: user_token_1_account,
            token_0_vault,
            token_1_vault,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            vault_0_mint: token_0_mint,
            vault_1_mint: token_1_mint,
            lp_vault,
            compressed_token_program: compressed_token::id(),
            memo_program: spl_memo::id(),
            compressed_token_program_cpi_authority: compressed_token::cpi_authority(),
            compressed_token_0_pool_pda,
            compressed_token_1_pool_pda,
        })
        .args(raydium_cp_instructions::Withdraw {
            lp_token_amount,
            minimum_token_0_amount,
            minimum_token_1_amount,
        })
        .instructions()?;
    Ok(instructions)
}

pub fn swap_base_input_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    amm_config: Pubkey,
    observation_account: Pubkey,
    input_token_account: Pubkey,
    output_token_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    input_token_mint: Pubkey,
    output_token_mint: Pubkey,
    input_token_program: Pubkey,
    output_token_program: Pubkey,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path)?;
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let (compressed_token_0_pool_pda, _token_0_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&input_token_mint);
    let (compressed_token_1_pool_pda, _token_1_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&output_token_mint);

    let instructions = program
        .request()
        .accounts(raydium_cp_accounts::Swap {
            payer: program.payer(),
            authority,
            amm_config,
            pool_state: pool_id,
            input_token_account,
            output_token_account,
            input_vault,
            output_vault,
            input_token_program,
            output_token_program,
            input_token_mint,
            output_token_mint,
            observation_state: observation_account,
            compressed_token_program_cpi_authority: compressed_token::cpi_authority(),
            compressed_token_program: compressed_token::id(),
            compressed_token_0_pool_pda,
            compressed_token_1_pool_pda,
        })
        .args(raydium_cp_instructions::SwapBaseInput {
            amount_in,
            minimum_amount_out,
        })
        .instructions()?;
    Ok(instructions)
}

pub fn swap_base_output_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    amm_config: Pubkey,
    observation_account: Pubkey,
    input_token_account: Pubkey,
    output_token_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    input_token_mint: Pubkey,
    output_token_mint: Pubkey,
    input_token_program: Pubkey,
    output_token_program: Pubkey,
    max_amount_in: u64,
    amount_out: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path)?;
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let (compressed_token_0_pool_pda, _token_0_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&input_token_mint);
    let (compressed_token_1_pool_pda, _token_1_pool_pda_bump) =
        compressed_token::get_token_pool_address_and_bump(&output_token_mint);

    let instructions = program
        .request()
        .accounts(raydium_cp_accounts::Swap {
            payer: program.payer(),
            authority,
            amm_config,
            pool_state: pool_id,
            input_token_account,
            output_token_account,
            input_vault,
            output_vault,
            input_token_program,
            output_token_program,
            input_token_mint,
            output_token_mint,
            observation_state: observation_account,
            compressed_token_program_cpi_authority: compressed_token::cpi_authority(),
            compressed_token_program: compressed_token::id(),
            compressed_token_0_pool_pda,
            compressed_token_1_pool_pda,
        })
        .args(raydium_cp_instructions::SwapBaseOutput {
            max_amount_in,
            amount_out,
        })
        .instructions()?;
    Ok(instructions)
}
