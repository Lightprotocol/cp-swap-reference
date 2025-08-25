use anchor_spl::{associated_token::spl_associated_token_account, token::spl_token};
use anyhow::{format_err, Result};
use arrayref::array_ref;
use clap::Parser;
use configparser::ini::Ini;
use light_client::constants::LOOKUP_TABLE_ADDRESS;
use light_client::rpc::{load_lookup_table, LightClient, LightClientConfig, Rpc};

use light_compressible_client::account_fetcher::get_compressible_account;
use light_token_client::actions::create_token_pool;
use light_token_client::compressed_token;
use light_token_client::instructions::create_token_pool;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
};
use solana_transaction_status::UiTransactionEncoding;
use std::str::FromStr;

mod instructions;
use instructions::amm_instructions::*;
use instructions::compression_instructions::*;
use instructions::events_instructions_parse::*;
use instructions::rpc::*;
use instructions::token_instructions::*;
use instructions::utils::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    http_url: String,
    ws_url: String,
    payer_path: String,
    admin_path: String,
    raydium_cp_program: Pubkey,
    slippage: f64,
}

fn load_cfg(client_config: &String) -> Result<ClientConfig> {
    let mut config = Ini::new();
    let _map = config.load(client_config).unwrap();
    let http_url = config.get("Global", "http_url").unwrap();
    if http_url.is_empty() {
        panic!("http_url must not be empty");
    }
    let ws_url = config.get("Global", "ws_url").unwrap();
    if ws_url.is_empty() {
        panic!("ws_url must not be empty");
    }
    let payer_path = config.get("Global", "payer_path").unwrap();
    if payer_path.is_empty() {
        panic!("payer_path must not be empty");
    }
    let admin_path = config.get("Global", "admin_path").unwrap();
    if admin_path.is_empty() {
        panic!("admin_path must not be empty");
    }

    let raydium_cp_program_str = config.get("Global", "raydium_cp_program").unwrap();
    if raydium_cp_program_str.is_empty() {
        panic!("raydium_cp_program must not be empty");
    }
    let raydium_cp_program = Pubkey::from_str(&raydium_cp_program_str).unwrap();
    let slippage = config.getfloat("Global", "slippage").unwrap().unwrap();

    Ok(ClientConfig {
        http_url,
        ws_url,
        payer_path,
        admin_path,
        raydium_cp_program,
        slippage,
    })
}

fn read_keypair_file(s: &str) -> Result<Keypair> {
    solana_sdk::signature::read_keypair_file(s)
        .map_err(|_| format_err!("failed to read keypair from {}", s))
}

#[derive(Debug, Parser)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: RaydiumCpCommands,
}

#[derive(Debug, Parser)]
pub enum RaydiumCpCommands {
    InitializePool {
        mint0: Pubkey,
        mint1: Pubkey,
        init_amount_0: u64,
        init_amount_1: u64,
        #[arg(short, long, default_value_t = 0)]
        open_time: u64,
        #[clap(short, long, action)]
        random_pool: bool,
    },
    Deposit {
        pool_id: Pubkey,
        user_token_0: Pubkey,
        user_token_1: Pubkey,
        lp_token_amount: u64,
    },
    Withdraw {
        pool_id: Pubkey,
        lp_token_amount: u64,
    },
    SwapBaseIn {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        user_input_amount: u64,
    },
    SwapBaseOut {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        amount_out_less_fee: u64,
    },
    DecodeInstruction {
        instr_hex_data: String,
    },
    DecodeEvent {
        log_event: String,
    },
    DecodeTxLog {
        tx_id: String,
    },
    InitCompressionConfig {},
    CreateMint {
        #[clap(help = "Number of decimal places for the token")]
        decimals: u8,
        #[clap(short, long, help = "Path to save the mint keypair (optional)")]
        save_keypair: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let client_config = "client_config.ini";
    let pool_config = load_cfg(&client_config.to_string()).unwrap();
    let payer = read_keypair_file(&pool_config.payer_path)?;
    let rpc_client = RpcClient::new(pool_config.http_url.to_string());

    // 1. Extend RpcClient with ZK Compression RPC endpoints.
    let mut light_client = LightClient::new(LightClientConfig::local()).await?;

    let opts = Opts::parse();
    match opts.command {
        RaydiumCpCommands::InitializePool {
            mint0,
            mint1,
            init_amount_0,
            init_amount_1,
            open_time,
            random_pool,
        } => {
            let admin = read_keypair_file(&pool_config.admin_path)?;

            // Initialize compression config once globally.
            initialize_compression_config(&rpc_client, &pool_config, &admin).await?;

            let (mint0, mint1, init_amount_0, init_amount_1) = if mint0 > mint1 {
                (mint1, mint0, init_amount_1, init_amount_0)
            } else {
                (mint0, mint1, init_amount_0, init_amount_1)
            };

            let load_pubkeys = vec![mint0, mint1];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let token_0_program = rsps[0].clone().unwrap().owner;
            let token_1_program = rsps[1].clone().unwrap().owner;

            let mut signers = vec![&payer];

            let random_pool_keypair = Keypair::new();
            let random_pool_id = if random_pool {
                let random_pool_id = random_pool_keypair.pubkey();
                signers.push(&random_pool_keypair);
                Some(random_pool_id)
            } else {
                None
            };

            let initialize_pool_instr = initialize_pool_instr(
                &mut light_client,
                &pool_config,
                mint0,
                mint1,
                token_0_program,
                token_1_program,
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &payer.pubkey(),
                    &mint0,
                    &token_0_program,
                ),
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &payer.pubkey(),
                    &mint1,
                    &token_1_program,
                ),
                raydium_cp_swap::create_pool_fee_receiver::ID,
                random_pool_id,
                init_amount_0,
                init_amount_1,
                open_time,
            )
            .await?;

            let lookup_table = load_lookup_table(&rpc_client, &LOOKUP_TABLE_ADDRESS)?;

            send_versioned_txn(
                &rpc_client,
                &initialize_pool_instr,
                &signers,
                &payer.pubkey(),
                &[lookup_table],
                true,
            )?;
        }
        RaydiumCpCommands::Deposit {
            pool_id,
            user_token_0,
            user_token_1,
            lp_token_amount,
        } => {
            let pool_state =
                get_compressible_account::<raydium_cp_swap::states::PoolState, LightClient>(
                    &pool_id,
                    &pool_config.raydium_cp_program,
                    &light_client.get_address_tree_v2(),
                    &mut light_client,
                )
                .await?;

            decompress_pool_and_observation_idempotent(
                &mut light_client,
                &rpc_client,
                &pool_config,
                pool_id,
                pool_state.observation_key,
                pool_state.amm_config,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
            )
            .await?;

            let load_pubkeys = vec![pool_state.token_0_vault, pool_state.token_1_vault];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let [token_0_vault_account, token_1_vault_account] = array_ref![rsps, 0, 2];
            let token_0_vault_info = unpack_token(&token_0_vault_account.as_ref().unwrap().data)?;
            let token_1_vault_info = unpack_token(&token_1_vault_account.as_ref().unwrap().data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                u64::from(token_0_vault_info.base.amount),
                u64::from(token_1_vault_info.base.amount),
            );

            let results = raydium_cp_swap::curve::CurveCalculator::lp_tokens_to_trading_tokens(
                u128::from(lp_token_amount),
                u128::from(pool_state.lp_supply),
                u128::from(total_token_0_amount),
                u128::from(total_token_1_amount),
                raydium_cp_swap::curve::RoundDirection::Ceiling,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();

            let amount_0_with_slippage =
                amount_with_slippage(results.token_0_amount as u64, pool_config.slippage, true);
            let amount_1_with_slippage =
                amount_with_slippage(results.token_1_amount as u64, pool_config.slippage, true);
            // calc with transfer_fee
            let transfer_fee = get_pool_mints_inverse_fee(
                &rpc_client,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                amount_0_with_slippage,
                amount_1_with_slippage,
            );

            let amount_0_max = (amount_0_with_slippage as u64)
                .checked_add(transfer_fee.0.transfer_fee)
                .unwrap();
            let amount_1_max = (amount_1_with_slippage as u64)
                .checked_add(transfer_fee.1.transfer_fee)
                .unwrap();

            let mut instructions = Vec::new();
            let owner_lp_token = compressed_token::get_associated_ctoken_address(
                &payer.pubkey(),
                &pool_state.lp_mint,
            );

            let deposit_instr = deposit_instr(
                &pool_config,
                pool_id,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                pool_state.lp_mint,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                user_token_0,
                user_token_1,
                owner_lp_token,
                lp_token_amount,
                amount_0_max,
                amount_1_max,
            )?;
            instructions.extend(deposit_instr);
            let signers = vec![&payer];

            // Load lookup table
            let lookup_table = load_lookup_table(&rpc_client, &LOOKUP_TABLE_ADDRESS)?;
            let lookup_tables = vec![lookup_table];

            send_versioned_txn(
                &rpc_client,
                &instructions,
                &signers,
                &payer.pubkey(),
                &lookup_tables,
                true,
            )?;
        }
        RaydiumCpCommands::Withdraw {
            pool_id,
            lp_token_amount,
        } => {
            // Fetch pool state irrespective of whether it's currently
            // compressed or decompressed.
            let pool_state =
                get_compressible_account::<raydium_cp_swap::states::PoolState, LightClient>(
                    &pool_id,
                    &pool_config.raydium_cp_program,
                    &light_client.get_address_tree_v2(),
                    &mut light_client,
                )
                .await?;

            decompress_pool_and_observation_idempotent(
                &mut light_client,
                &rpc_client,
                &pool_config,
                pool_id,
                pool_state.observation_key,
                pool_state.amm_config,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
            )
            .await?;

            let load_pubkeys = vec![pool_state.token_0_vault, pool_state.token_1_vault];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let [token_0_vault_account, token_1_vault_account] = array_ref![rsps, 0, 2];
            let token_0_vault_info = unpack_token(&token_0_vault_account.as_ref().unwrap().data)?;
            let token_1_vault_info = unpack_token(&token_1_vault_account.as_ref().unwrap().data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                u64::from(token_0_vault_info.base.amount),
                u64::from(token_1_vault_info.base.amount),
            );

            let results = raydium_cp_swap::curve::CurveCalculator::lp_tokens_to_trading_tokens(
                u128::from(lp_token_amount),
                u128::from(pool_state.lp_supply),
                u128::from(total_token_0_amount),
                u128::from(total_token_1_amount),
                raydium_cp_swap::curve::RoundDirection::Ceiling,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();

            let amount_0_with_slippage =
                amount_with_slippage(results.token_0_amount as u64, pool_config.slippage, false);
            let amount_1_with_slippage =
                amount_with_slippage(results.token_1_amount as u64, pool_config.slippage, false);

            let transfer_fee = get_pool_mints_transfer_fee(
                &rpc_client,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                amount_0_with_slippage,
                amount_1_with_slippage,
            );

            let amount_0_min = amount_0_with_slippage
                .checked_sub(transfer_fee.0.transfer_fee)
                .unwrap();
            let amount_1_min = amount_1_with_slippage
                .checked_sub(transfer_fee.1.transfer_fee)
                .unwrap();

            let mut instructions = Vec::new();
            let create_user_token_0_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &pool_state.token_0_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_token_0_instr);
            let create_user_token_1_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &pool_state.token_1_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_token_1_instr);
            let owner_lp_token = compressed_token::get_associated_ctoken_address(
                &payer.pubkey(),
                &pool_state.lp_mint,
            );

            let withdraw_instr = withdraw_instr(
                &pool_config,
                pool_id,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                pool_state.lp_mint,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_0_mint,
                ),
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_1_mint,
                ),
                owner_lp_token, // Use derived LP token account instead of user_lp_token parameter
                lp_token_amount,
                amount_0_min,
                amount_1_min,
            )?;
            instructions.extend(withdraw_instr);
            let signers = vec![&payer];

            // Load lookup table
            let lookup_table = load_lookup_table(&rpc_client, &LOOKUP_TABLE_ADDRESS)?;
            let lookup_tables = vec![lookup_table];

            send_versioned_txn(
                &rpc_client,
                &instructions,
                &signers,
                &payer.pubkey(),
                &lookup_tables,
                true,
            )?;
        }
        RaydiumCpCommands::SwapBaseIn {
            pool_id,
            user_input_token,
            user_input_amount,
        } => {
            // Fetch pool state irrespective of whether it's currently
            // compressed or decompressed.
            let pool_state =
                get_compressible_account::<raydium_cp_swap::states::PoolState, LightClient>(
                    &pool_id,
                    &pool_config.raydium_cp_program,
                    &light_client.get_address_tree_v2(),
                    &mut light_client,
                )
                .await?;

            decompress_pool_and_observation_idempotent(
                &mut light_client,
                &rpc_client,
                &pool_config,
                pool_id,
                pool_state.observation_key,
                pool_state.amm_config,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
            )
            .await?;

            let load_pubkeys = vec![
                pool_id,
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                user_input_token,
            ];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let epoch = rpc_client.get_epoch_info().unwrap().epoch;
            let [pool_account, amm_config_account, token_0_vault_account, token_1_vault_account, token_0_mint_account, token_1_mint_account, user_input_token_account] =
                array_ref![rsps, 0, 7];

            let pool_state = deserialize_anchor_account::<raydium_cp_swap::states::PoolState>(
                pool_account.as_ref().unwrap(),
            )
            .unwrap();
            let amm_config_state = deserialize_anchor_account::<raydium_cp_swap::states::AmmConfig>(
                amm_config_account.as_ref().unwrap(),
            )?;
            let token_0_vault_info = unpack_token(&token_0_vault_account.as_ref().unwrap().data)?;
            let token_1_vault_info = unpack_token(&token_1_vault_account.as_ref().unwrap().data)?;
            let token_0_mint_info = unpack_mint(&token_0_mint_account.as_ref().unwrap().data)?;
            let token_1_mint_info = unpack_mint(&token_1_mint_account.as_ref().unwrap().data)?;
            let user_input_token_info =
                unpack_token(&user_input_token_account.as_ref().unwrap().data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                u64::from(token_0_vault_info.base.amount),
                u64::from(token_1_vault_info.base.amount),
            );

            let (
                trade_direction,
                total_input_token_amount,
                total_output_token_amount,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                transfer_fee,
            ) = if user_input_token_info.base.mint == token_0_vault_info.base.mint {
                (
                    raydium_cp_swap::curve::TradeDirection::ZeroForOne,
                    total_token_0_amount,
                    total_token_1_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_1_mint,
                    ),
                    pool_state.token_0_vault,
                    pool_state.token_1_vault,
                    pool_state.token_0_mint,
                    pool_state.token_1_mint,
                    pool_state.token_0_program,
                    pool_state.token_1_program,
                    get_transfer_fee(&token_0_mint_info, epoch, user_input_amount),
                )
            } else {
                (
                    raydium_cp_swap::curve::TradeDirection::OneForZero,
                    total_token_1_amount,
                    total_token_0_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_0_mint,
                    ),
                    pool_state.token_1_vault,
                    pool_state.token_0_vault,
                    pool_state.token_1_mint,
                    pool_state.token_0_mint,
                    pool_state.token_1_program,
                    pool_state.token_0_program,
                    get_transfer_fee(&token_1_mint_info, epoch, user_input_amount),
                )
            };

            let actual_amount_in = user_input_amount.saturating_sub(transfer_fee);
            let result = raydium_cp_swap::curve::CurveCalculator::swap_base_input(
                u128::from(actual_amount_in),
                u128::from(total_input_token_amount),
                u128::from(total_output_token_amount),
                amm_config_state.trade_fee_rate,
                amm_config_state.protocol_fee_rate,
                amm_config_state.fund_fee_rate,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();
            let amount_out = u64::try_from(result.destination_amount_swapped).unwrap();
            let transfer_fee = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => {
                    get_transfer_fee(&token_1_mint_info, epoch, amount_out)
                }
                raydium_cp_swap::curve::TradeDirection::OneForZero => {
                    get_transfer_fee(&token_0_mint_info, epoch, amount_out)
                }
            };
            let amount_received = amount_out.checked_sub(transfer_fee).unwrap();

            let minimum_amount_out =
                amount_with_slippage(amount_received, pool_config.slippage, false);

            let mut instructions = Vec::new();
            let create_user_output_token_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &output_token_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_output_token_instr);
            let swap_base_in_instr = swap_base_input_instr(
                &pool_config,
                pool_id,
                pool_state.amm_config,
                pool_state.observation_key,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                user_input_amount,
                minimum_amount_out,
            )?;
            instructions.extend(swap_base_in_instr);
            let signers = vec![&payer];

            // Load lookup table
            let lookup_table = load_lookup_table(&rpc_client, &LOOKUP_TABLE_ADDRESS)?;

            send_versioned_txn(
                &rpc_client,
                &instructions,
                &signers,
                &payer.pubkey(),
                &[lookup_table],
                true,
            )?;
        }
        RaydiumCpCommands::SwapBaseOut {
            pool_id,
            user_input_token,
            amount_out_less_fee,
        } => {
            // Fetch pool state irrespective of whether it's currently
            // compressed or decompressed.
            let pool_state =
                get_compressible_account::<raydium_cp_swap::states::PoolState, LightClient>(
                    &pool_id,
                    &pool_config.raydium_cp_program,
                    &light_client.get_address_tree_v2(),
                    &mut light_client,
                )
                .await?;

            decompress_pool_and_observation_idempotent(
                &mut light_client,
                &rpc_client,
                &pool_config,
                pool_id,
                pool_state.observation_key,
                pool_state.amm_config,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
            )
            .await?;

            let load_pubkeys = vec![
                pool_id,
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                user_input_token,
            ];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let epoch = rpc_client.get_epoch_info().unwrap().epoch;
            let [pool_account, amm_config_account, token_0_vault_account, token_1_vault_account, token_0_mint_account, token_1_mint_account, user_input_token_account] =
                array_ref![rsps, 0, 7];

            let pool_state = deserialize_anchor_account::<raydium_cp_swap::states::PoolState>(
                pool_account.as_ref().unwrap(),
            )
            .unwrap();
            let amm_config_state = deserialize_anchor_account::<raydium_cp_swap::states::AmmConfig>(
                amm_config_account.as_ref().unwrap(),
            )?;
            let token_0_vault_info = unpack_token(&token_0_vault_account.as_ref().unwrap().data)?;
            let token_1_vault_info = unpack_token(&token_1_vault_account.as_ref().unwrap().data)?;
            let token_0_mint_info = unpack_mint(&token_0_mint_account.as_ref().unwrap().data)?;
            let token_1_mint_info = unpack_mint(&token_1_mint_account.as_ref().unwrap().data)?;
            let user_input_token_info =
                unpack_token(&user_input_token_account.as_ref().unwrap().data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                u64::from(token_0_vault_info.base.amount),
                u64::from(token_1_vault_info.base.amount),
            );

            let (
                trade_direction,
                total_input_token_amount,
                total_output_token_amount,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                out_transfer_fee,
            ) = if user_input_token_info.base.mint == token_0_vault_info.base.mint {
                (
                    raydium_cp_swap::curve::TradeDirection::ZeroForOne,
                    total_token_0_amount,
                    total_token_1_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_1_mint,
                    ),
                    pool_state.token_0_vault,
                    pool_state.token_1_vault,
                    pool_state.token_0_mint,
                    pool_state.token_1_mint,
                    pool_state.token_0_program,
                    pool_state.token_1_program,
                    get_transfer_inverse_fee(&token_1_mint_info, epoch, amount_out_less_fee),
                )
            } else {
                (
                    raydium_cp_swap::curve::TradeDirection::OneForZero,
                    total_token_1_amount,
                    total_token_0_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_0_mint,
                    ),
                    pool_state.token_1_vault,
                    pool_state.token_0_vault,
                    pool_state.token_1_mint,
                    pool_state.token_0_mint,
                    pool_state.token_1_program,
                    pool_state.token_0_program,
                    get_transfer_inverse_fee(&token_0_mint_info, epoch, amount_out_less_fee),
                )
            };
            let actual_amount_out = amount_out_less_fee.checked_add(out_transfer_fee).unwrap();

            let result = raydium_cp_swap::curve::CurveCalculator::swap_base_output(
                u128::from(actual_amount_out),
                u128::from(total_input_token_amount),
                u128::from(total_output_token_amount),
                amm_config_state.trade_fee_rate,
                amm_config_state.protocol_fee_rate,
                amm_config_state.fund_fee_rate,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();

            let source_amount_swapped = u64::try_from(result.source_amount_swapped).unwrap();
            let amount_in_transfer_fee = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => {
                    get_transfer_inverse_fee(&token_0_mint_info, epoch, source_amount_swapped)
                }
                raydium_cp_swap::curve::TradeDirection::OneForZero => {
                    get_transfer_inverse_fee(&token_1_mint_info, epoch, source_amount_swapped)
                }
            };

            let input_transfer_amount = source_amount_swapped
                .checked_add(amount_in_transfer_fee)
                .unwrap();

            let max_amount_in =
                amount_with_slippage(input_transfer_amount, pool_config.slippage, true);
            let mut instructions = Vec::new();
            let create_user_output_token_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &output_token_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_output_token_instr);
            let swap_base_in_instr = swap_base_output_instr(
                &pool_config,
                pool_id,
                pool_state.amm_config,
                pool_state.observation_key,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                max_amount_in,
                amount_out_less_fee,
            )?;
            instructions.extend(swap_base_in_instr);
            let signers = vec![&payer];

            // Load lookup table
            let lookup_table = load_lookup_table(&rpc_client, &LOOKUP_TABLE_ADDRESS)?;

            send_versioned_txn(
                &rpc_client,
                &instructions,
                &signers,
                &payer.pubkey(),
                &[lookup_table],
                true,
            )?;
        }
        RaydiumCpCommands::DecodeInstruction { instr_hex_data } => {
            handle_program_instruction(&instr_hex_data, InstructionDecodeType::BaseHex)?;
        }
        RaydiumCpCommands::DecodeEvent { log_event } => {
            handle_program_log(
                &pool_config.raydium_cp_program.to_string(),
                &log_event,
                false,
            )?;
        }
        RaydiumCpCommands::DecodeTxLog { tx_id } => {
            let signature = Signature::from_str(&tx_id)?;
            let tx = rpc_client.get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Json),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            )?;
            let transaction = tx.transaction;
            // get meta
            let meta = if transaction.meta.is_some() {
                transaction.meta
            } else {
                None
            };
            // get encoded_transaction
            let encoded_transaction = transaction.transaction;
            // decode instruction data
            parse_program_instruction(
                &pool_config.raydium_cp_program.to_string(),
                encoded_transaction,
                meta.clone(),
            )?;
            // decode logs
            parse_program_event(&pool_config.raydium_cp_program.to_string(), meta.clone())?;
        }
        RaydiumCpCommands::InitCompressionConfig {} => {
            let authority = read_keypair_file(&pool_config.admin_path)?;
            initialize_compression_config(&rpc_client, &pool_config, &authority).await?;
        }
        RaydiumCpCommands::CreateMint {
            decimals,
            save_keypair,
        } => {
            use crate::instructions::rpc::send_txn;
            use crate::instructions::token_instructions::*;

            let mint_keypair = Keypair::new();
            let token_program = spl_token::id();
            let mint_authority = payer.pubkey();
            let freeze_authority = Some(&mint_authority);

            let mut light_client = LightClient::new(LightClientConfig::local()).await?;

            let create_mint_ixs = create_and_init_mint_instr(
                &pool_config,
                token_program,
                &mint_keypair.pubkey(),
                &mint_authority,
                freeze_authority,
                decimals,
            )
            .await?;

            let create_ata_ixs = create_ata_token_account_instr(
                &pool_config,
                token_program,
                &mint_keypair.pubkey(),
                &payer.pubkey(),
            )?;

            let mint_amount = 1_000_000 * 10_u64.pow(decimals as u32);
            let ata_address = spl_associated_token_account::get_associated_token_address(
                &payer.pubkey(),
                &mint_keypair.pubkey(),
            );

            let mint_to_ixs = spl_token_mint_to_instr(
                &pool_config,
                token_program,
                &mint_keypair.pubkey(),
                &ata_address,
                mint_amount,
                &payer,
            )?;

            let mut all_instructions = Vec::new();
            all_instructions.extend(create_mint_ixs);
            all_instructions.extend(create_ata_ixs);
            all_instructions.extend(mint_to_ixs);

            let recent_blockhash = rpc_client.get_latest_blockhash()?;
            let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
                &all_instructions,
                Some(&payer.pubkey()),
                &[&payer, &mint_keypair],
                recent_blockhash,
            );

            let signature = send_txn(&rpc_client, &transaction, true)?;

            /// create the compressed token pool pdas
            create_token_pool(&mut light_client, &mint_keypair.pubkey(), false, &payer).await?;

            println!("Mint Address: {}", mint_keypair.pubkey().to_string());
            println!("Your Token Account: {}", ata_address.to_string());
            println!(
                "Minted Amount: {} tokens",
                mint_amount / 10_u64.pow(decimals as u32)
            );
            println!("Transaction Signature: {}", signature.to_string());

            if let Some(path) = save_keypair {
                std::fs::write(
                    &path,
                    serde_json::to_string(&mint_keypair.to_bytes().to_vec())?,
                )?;
                println!("Mint keypair saved to: {}", path);
            }
        }
    }
    Ok(())
}
