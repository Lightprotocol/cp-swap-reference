#![allow(dead_code, clippy::too_many_arguments, clippy::useless_vec)]

/// Functional integration test for cp-swap program.
/// Tests pool initialization with light test-validator and photon indexer.
use anchor_lang::{InstructionData, ToAccountMetas};
use forester_utils::forester_epoch::get_epoch_phases;
use light_anchor_spl::memo::spl_memo;
use light_client::{
    indexer::Indexer,
    interface::{
        get_create_accounts_proof, instructions::build_compress_accounts_idempotent,
        instructions::COMPRESS_ACCOUNTS_IDEMPOTENT_DISCRIMINATOR, CreateAccountsProofInput,
        CreateAccountsProofResult, InitializeRentFreeConfig, LightConfig,
    },
    rpc::{
        lut::{instruction as lut_instruction, load_lookup_table},
        LightClient, LightClientConfig, Rpc,
    },
};
use light_program_test::accounts::test_keypairs::PAYER_KEYPAIR;
use light_registry::{
    protocol_config::state::ProtocolConfigPda,
    sdk::{
        create_finalize_registration_instruction, create_register_forester_epoch_pda_instruction,
        create_register_forester_instruction,
    },
    utils::{get_forester_pda, get_protocol_config_pda_address},
    ForesterConfig as RegistryForesterConfig,
};
use light_sdk::compressed_account::derive_address;
use light_sdk::light_account_checks::discriminator::DISCRIMINATOR_LEN;
use light_sdk::LightDiscriminator;
use light_token::{
    constants::CPI_AUTHORITY_PDA,
    constants::LIGHT_TOKEN_PROGRAM_ID,
    instruction::{
        find_mint_address, get_associated_token_address_and_bump, CreateAssociatedTokenAccount,
        CreateMint, CreateMintParams, MintTo, LIGHT_TOKEN_CONFIG, LIGHT_TOKEN_RENT_SPONSOR,
    },
};
use raydium_cp_swap::{
    instructions::initialize::LP_MINT_SIGNER_SEED,
    program_rent_sponsor,
    states::{
        ObservationState, PoolState, AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_SEED, POOL_VAULT_SEED,
    },
    InitializeParams, AUTH_SEED,
};
use solana_commitment_config::CommitmentConfig;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::AddressLookupTableAccount;
use solana_pubkey::Pubkey;
use solana_sdk::{program_pack::Pack, signature::SeedDerivable};
use solana_signer::Signer;
use tokio::sync::OnceCell;

static VALIDATOR_INIT: OnceCell<()> = OnceCell::const_new();
static VALIDATOR_WITH_FORESTER_INIT: OnceCell<()> = OnceCell::const_new();

/// BPF Loader Upgradeable program ID
const BPF_LOADER_UPGRADEABLE: Pubkey =
    solana_pubkey::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");

// ============================================================================
// Constants
// ============================================================================

fn rent_sponsor() -> Pubkey {
    program_rent_sponsor()
}

pub fn light_token_program_id() -> Pubkey {
    LIGHT_TOKEN_PROGRAM_ID
}

// ============================================================================
// Types
// ============================================================================
/// PDAs for the AMM pool.
pub struct AmmPdas {
    pub pool_state: Pubkey,
    pub observation_state: Pubkey,
    pub authority: Pubkey,
    pub authority_bump: u8,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub lp_mint_signer: Pubkey,
    pub lp_mint_signer_bump: u8,
    pub lp_mint: Pubkey,
    pub creator_lp_token: Pubkey,
    pub creator_lp_token_bump: u8,
}

/// Test environment setup result.
pub struct TestEnv {
    pub rpc: LightClient,
    pub payer: Keypair,
    pub config_pda: Pubkey,
}

/// Token mints and creator accounts for the pool.
pub struct TokenSetup {
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_mint_signer: Pubkey,
    pub token_1_mint_signer: Pubkey,
    pub creator_token_0: Pubkey,
    pub creator_token_1: Pubkey,
}

// ============================================================================
// Setup Functions
// ============================================================================

/// Get the payer keypair used for test-validator deployment.
/// This must match the upgrade authority used when spawning the validator.
pub fn get_payer_keypair() -> Keypair {
    Keypair::try_from(PAYER_KEYPAIR.as_ref()).expect("Invalid PAYER_KEYPAIR")
}

/// Create the pool fee receiver account data file for preloading.
/// Returns the path to the JSON file.
fn create_pool_fee_receiver_account_file() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    let wsol_mint = spl_token::native_mint::id();
    let payer = get_payer_keypair();

    // Create a wrapped SOL token account structure
    // Token account layout: mint (32) + owner (32) + amount (8) + delegate (36) + state (1) + ...
    let mut data = vec![0u8; spl_token::state::Account::LEN];

    // Set mint (first 32 bytes)
    data[0..32].copy_from_slice(wsol_mint.as_ref());
    // Set owner (next 32 bytes)
    data[32..64].copy_from_slice(payer.pubkey().as_ref());
    // Set amount (next 8 bytes) - 1 SOL in lamports
    let amount: u64 = 1_000_000_000;
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    // Set delegate option to None (4 bytes = 0)
    data[72..76].copy_from_slice(&[0u8; 4]);
    // Skip delegate pubkey (32 bytes)
    // Set state to Initialized (1 byte = 1)
    data[108] = 1;
    // Rest is zeros (is_native, delegated_amount, close_authority)

    // Create JSON in the format expected by solana-test-validator --account
    let account_json = json!({
        "pubkey": raydium_cp_swap::create_pool_fee_receiver::ID.to_string(),
        "account": {
            "lamports": 2_000_000_000u64, // 2 SOL for rent + balance
            "data": [STANDARD.encode(&data), "base64"],
            "owner": spl_token::id().to_string(),
            "executable": false,
            "rentEpoch": 0
        }
    });

    // Write to temp file
    let tmp_dir = std::env::temp_dir();
    let file_path: PathBuf = tmp_dir.join("pool_fee_receiver.json");
    fs::write(
        &file_path,
        serde_json::to_string_pretty(&account_json).unwrap(),
    )
    .expect("Failed to write pool fee receiver account file");

    file_path.to_string_lossy().to_string()
}

/// Check if validator is already running by testing RPC endpoint.
async fn is_validator_running() -> bool {
    use reqwest::Client;
    let client = Client::new();
    match client
        .post("http://localhost:8899")
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"getHealth"}"#)
        .send()
        .await
    {
        Ok(resp) => resp
            .text()
            .await
            .map(|t| t.contains("\"ok\""))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Spawn the test-validator with cp-swap program deployed.
/// This is called once per test run via Once.
/// If validator is already running (e.g., started by justfile), skip spawning.
async fn ensure_validator_running(program_id: Pubkey) {
    use std::process::{Command, Stdio};

    // Check if validator is already running
    if is_validator_running().await {
        println!("Validator already running, skipping spawn");
        return;
    }

    // Stop any existing validator first
    println!("Stopping any existing validator...");
    let _ = Command::new("light")
        .args(["test-validator", "--stop"])
        .output();

    // Give it a moment to clean up
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Clean up old ledger to ensure fresh state
    let _ = std::fs::remove_dir_all("test-ledger");

    // Get the path to the compiled program (use absolute path)
    // Look up from cwd to find the target/deploy directory
    let program_path = std::env::var("SBF_OUT_DIR")
        .map(|dir| format!("{}/raydium_cp_swap.so", dir))
        .unwrap_or_else(|_| {
            let cwd = std::env::current_dir().expect("Failed to get current directory");
            // Try multiple possible locations
            let candidates = vec![
                cwd.join("target/deploy/raydium_cp_swap.so"),
                cwd.join("../../target/deploy/raydium_cp_swap.so"), // From programs/cp-swap
                cwd.parent()
                    .unwrap()
                    .join("target/deploy/raydium_cp_swap.so"),
                cwd.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("target/deploy/raydium_cp_swap.so"),
            ];
            candidates
                .iter()
                .find(|p| p.exists())
                .expect("Could not find raydium_cp_swap.so - run `cargo build-sbf` first")
                .canonicalize()
                .expect("Failed to canonicalize program path")
                .to_string_lossy()
                .to_string()
        });

    let payer = get_payer_keypair();

    // Create the pool fee receiver account file for preloading
    let fee_receiver_file = create_pool_fee_receiver_account_file();
    let fee_receiver_address = raydium_cp_swap::create_pool_fee_receiver::ID;

    // Build the command using the light CLI from PATH
    // Note: --account must be passed via --validator-args since it's a solana-test-validator flag
    let cmd = format!(
        "light test-validator \
         --limit-ledger-size 50000000 \
         --upgradeable-program {} {} {} \
         --validator-args '--account {} {}'",
        program_id,
        program_path,
        payer.pubkey(),
        fee_receiver_address,
        fee_receiver_file
    );

    println!("Starting validator with command: {}", cmd);

    let child = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start validator process");

    // Detach the process
    std::mem::drop(child);

    // Wait for validator to start
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
}

/// Spawn the test-validator with cp-swap program deployed AND forester for auto-compression.
/// This is for tests that need to verify compression behavior.
#[allow(dead_code)]
async fn ensure_validator_running_with_forester(program_id: Pubkey) {
    use std::process::{Command, Stdio};

    // Check if validator is already running
    if is_validator_running().await {
        println!("Validator already running, skipping spawn");
        return;
    }

    // Stop any existing validator first
    println!("Stopping any existing validator...");
    let _ = Command::new("light")
        .args(["test-validator", "--stop"])
        .output();

    // Give it a moment to clean up
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Clean up old ledger to ensure fresh state
    let _ = std::fs::remove_dir_all("test-ledger");

    // Get the path to the compiled program (use absolute path)
    let program_path = std::env::var("SBF_OUT_DIR")
        .map(|dir| format!("{}/raydium_cp_swap.so", dir))
        .unwrap_or_else(|_| {
            let cwd = std::env::current_dir().expect("Failed to get current directory");
            let candidates = [
                cwd.join("target/deploy/raydium_cp_swap.so"),
                cwd.join("../../target/deploy/raydium_cp_swap.so"),
                cwd.parent()
                    .unwrap()
                    .join("target/deploy/raydium_cp_swap.so"),
                cwd.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("target/deploy/raydium_cp_swap.so"),
            ];
            candidates
                .iter()
                .find(|p| p.exists())
                .expect("Could not find raydium_cp_swap.so - run `cargo build-sbf` first")
                .canonicalize()
                .expect("Failed to canonicalize program path")
                .to_string_lossy()
                .to_string()
        });

    let payer = get_payer_keypair();

    // Create the pool fee receiver account file for preloading
    let fee_receiver_file = create_pool_fee_receiver_account_file();
    let fee_receiver_address = raydium_cp_swap::create_pool_fee_receiver::ID;

    // Get discriminators from the actual types via LightDiscriminator trait
    let pool_state_disc_b58 = bs58::encode(PoolState::LIGHT_DISCRIMINATOR).into_string();
    let observation_state_disc_b58 =
        bs58::encode(ObservationState::LIGHT_DISCRIMINATOR).into_string();

    // Build the command with forester enabled
    // Format for compressible-pda-program: 'program_id:discriminator_base58'
    // Note: We use a very short --slots-per-epoch (32) so accounts become compressible quickly.
    // With max_funded_epochs=2 in RentConfig::default(), accounts become compressible after 2 epochs.
    // Using 32 slots/epoch means accounts become compressible after ~64 slots (~26 seconds at 400ms/slot).
    let slots_per_epoch = 32;
    let cmd = format!(
        "light test-validator \
         --limit-ledger-size 50000000 \
         --upgradeable-program {} {} {} \
         --forester \
         --compressible-pda-program {}:{} \
         --compressible-pda-program {}:{} \
         --validator-args '--account {} {} --slots-per-epoch {}'",
        program_id,
        program_path,
        payer.pubkey(),
        program_id,
        pool_state_disc_b58,
        program_id,
        observation_state_disc_b58,
        fee_receiver_address,
        fee_receiver_file,
        slots_per_epoch
    );

    println!("Starting validator with forester: {}", cmd);

    let child = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start validator process");

    // Detach the process
    std::mem::drop(child);

    // Wait for validator to start (forester takes longer to start)
    tokio::time::sleep(tokio::time::Duration::from_secs(90)).await;
}

/// Register the forester with the Light Registry protocol.
/// This is required for the forester to perform compression operations.
async fn register_forester(rpc: &mut LightClient) -> Result<Keypair, Box<dyn std::error::Error>> {
    use solana_sdk::transaction::Transaction;
    use std::time::Duration;

    let forester_keypair = get_payer_keypair();
    let forester_pubkey = forester_keypair.pubkey();

    // Governance authority is the same as PAYER_KEYPAIR in tests
    let governance_authority = get_payer_keypair();
    let governance_pubkey = governance_authority.pubkey();

    // Fund governance authority if needed
    let gov_balance = rpc.get_balance(&governance_pubkey).await.unwrap_or(0);
    if gov_balance < 10_000_000_000 {
        println!(
            "Funding governance authority {} with 10 SOL",
            governance_pubkey
        );
        rpc.airdrop_lamports(&governance_pubkey, 10_000_000_000 - gov_balance)
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Fund forester
    let forester_balance = rpc.get_balance(&forester_pubkey).await.unwrap_or(0);
    if forester_balance < 10_000_000_000 {
        println!("Funding forester {} with 10 SOL", forester_pubkey);
        rpc.airdrop_lamports(&forester_pubkey, 10_000_000_000 - forester_balance)
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Get protocol config
    let protocol_config_pda_address = get_protocol_config_pda_address().0;
    let protocol_config = rpc
        .get_anchor_account::<ProtocolConfigPda>(&protocol_config_pda_address)
        .await?
        .ok_or("Protocol config not found")?
        .config;

    // Check if forester is already registered
    let (forester_pda, _) = get_forester_pda(&forester_pubkey);
    let existing_forester = rpc.get_account(forester_pda).await.ok().flatten();

    if existing_forester.is_none() {
        // Register base forester
        let register_ix = create_register_forester_instruction(
            &governance_pubkey,
            &governance_pubkey,
            &forester_pubkey,
            RegistryForesterConfig::default(),
        );

        let (blockhash, _) = rpc.get_latest_blockhash().await?;
        let tx = Transaction::new_signed_with_payer(
            &[register_ix],
            Some(&governance_pubkey),
            &[&governance_authority],
            blockhash,
        );
        rpc.process_transaction(tx).await?;
        println!("Registered base forester: {}", forester_pda);
    } else {
        println!("Forester already registered: {}", forester_pda);
    }

    // Determine which epoch to register for
    let current_slot = rpc.get_slot().await?;
    let current_epoch = protocol_config.get_current_epoch(current_slot);
    let phases = get_epoch_phases(&protocol_config, current_epoch);

    println!(
        "Current slot: {}, current_epoch: {}, phases: {:?}",
        current_slot, current_epoch, phases
    );

    let (target_epoch, register_phase_start, active_phase_start) =
        if current_slot >= phases.active.start {
            let next_epoch = current_epoch + 1;
            let next_phases = get_epoch_phases(&protocol_config, next_epoch);
            println!(
                "Already in active phase, registering for next epoch {}, phases: {:?}",
                next_epoch, next_phases
            );
            (
                next_epoch,
                next_phases.registration.start,
                next_phases.active.start,
            )
        } else if current_slot >= phases.registration.start {
            println!("In registration phase for epoch {}", current_epoch);
            (
                current_epoch,
                phases.registration.start,
                phases.active.start,
            )
        } else {
            println!(
                "Waiting for registration phase (starts at slot {})",
                phases.registration.start
            );
            (
                current_epoch,
                phases.registration.start,
                phases.active.start,
            )
        };

    // Wait for registration phase
    while rpc.get_slot().await? < register_phase_start {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    // Register for the target epoch
    let register_epoch_ix = create_register_forester_epoch_pda_instruction(
        &forester_pubkey,
        &forester_pubkey,
        target_epoch,
    );

    let (blockhash, _) = rpc.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(
        &[register_epoch_ix],
        Some(&forester_pubkey),
        &[&forester_keypair],
        blockhash,
    );
    rpc.process_transaction(tx).await?;
    println!("Registered for epoch {}", target_epoch);

    // Wait for active phase
    while rpc.get_slot().await? < active_phase_start {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    println!("Active phase reached for epoch {}", target_epoch);

    // Finalize registration
    let finalize_ix =
        create_finalize_registration_instruction(&forester_pubkey, &forester_pubkey, target_epoch);

    let (blockhash, _) = rpc.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(
        &[finalize_ix],
        Some(&forester_pubkey),
        &[&forester_keypair],
        blockhash,
    );
    rpc.process_transaction(tx).await?;
    println!("Finalized forester registration for epoch {}", target_epoch);

    Ok(forester_keypair)
}

/// Wait for the indexer to be synced with the RPC.
pub async fn wait_for_indexer(rpc: &LightClient) -> Result<(), String> {
    let max_attempts = 120; // Increase max attempts for slower startup
    for attempt in 0..max_attempts {
        // First check if RPC is responding
        let rpc_slot = match rpc.get_slot().await {
            Ok(slot) => slot,
            Err(e) => {
                if attempt % 10 == 0 {
                    println!("Waiting for RPC... attempt {}: {}", attempt, e);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        // Then check indexer
        let indexer = match rpc.indexer() {
            Ok(i) => i,
            Err(e) => {
                if attempt % 10 == 0 {
                    println!(
                        "Waiting for indexer connection... attempt {}: {}",
                        attempt, e
                    );
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        let indexer_slot = match indexer.get_indexer_slot(None).await {
            Ok(slot) => slot,
            Err(e) => {
                if attempt % 10 == 0 {
                    println!("Waiting for indexer slot... attempt {}: {}", attempt, e);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        if indexer_slot >= rpc_slot.saturating_sub(5) {
            println!(
                "Indexer synced! RPC slot: {}, Indexer slot: {}",
                rpc_slot, indexer_slot
            );
            return Ok(());
        }

        if attempt % 10 == 0 {
            println!(
                "Waiting for indexer to sync... RPC slot: {}, Indexer slot: {}",
                rpc_slot, indexer_slot
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    Err("Indexer did not sync in time".to_string())
}

/// Initialize the test environment with test-validator and photon indexer.
pub async fn setup_test_environment(program_id: Pubkey) -> TestEnv {
    setup_test_environment_inner(program_id, false).await
}

/// Initialize the test environment with test-validator, photon indexer, AND forester.
/// Use this for tests that need auto-compression.
pub async fn setup_test_environment_with_forester(program_id: Pubkey) -> TestEnv {
    setup_test_environment_inner(program_id, true).await
}

/// Internal helper for test environment setup.
async fn setup_test_environment_inner(program_id: Pubkey, with_forester: bool) -> TestEnv {
    // Ensure validator is running (only spawns once per variant)
    if with_forester {
        VALIDATOR_WITH_FORESTER_INIT
            .get_or_init(|| async {
                ensure_validator_running_with_forester(program_id).await;
            })
            .await;
    } else {
        VALIDATOR_INIT
            .get_or_init(|| async {
                ensure_validator_running(program_id).await;
            })
            .await;
    }

    // Connect to the running validator
    let config = LightClientConfig::local();
    let mut rpc = <LightClient as Rpc>::new(config)
        .await
        .expect("Failed to connect to validator");

    // Wait for indexer to be synced before proceeding
    wait_for_indexer(&rpc).await.expect("Indexer should sync");

    // Fetch state trees from the validator (populates the internal cache)
    rpc.get_latest_active_state_trees()
        .await
        .expect("Failed to fetch state trees");

    // Use the payer keypair that was used as upgrade authority
    // Note: This is also the forester's payer keypair (from ~/.config/solana/id.json)
    let payer = get_payer_keypair();

    // Fund the payer with enough SOL for both tests AND forester operations
    // Forester needs SOL to call compress_accounts_idempotent
    rpc.airdrop_lamports(&payer.pubkey(), 100_000_000_000)
        .await
        .expect("Airdrop to payer should succeed");

    // Register forester with the Light Registry protocol
    if with_forester {
        println!("Forester logs available at: test-ledger/forester.log");
        println!("Forester payer (same as test payer): {}", payer.pubkey());

        // Register forester so it can perform compression operations
        register_forester(&mut rpc)
            .await
            .expect("Forester registration should succeed");
    }

    // Derive program_data_pda from BPF loader
    let (program_data_pda, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &BPF_LOADER_UPGRADEABLE);

    let (init_config_ix, config_pda) = InitializeRentFreeConfig::new(
        &program_id,
        &payer.pubkey(),
        &program_data_pda,
        rent_sponsor(),
        payer.pubkey(),
    )
    .build();

    // Check if config already exists (idempotent initialization for test reruns)
    let config_exists = rpc.get_account(config_pda).await.ok().flatten().is_some();
    if !config_exists {
        rpc.create_and_send_transaction(&[init_config_ix], &payer.pubkey(), &[&payer])
            .await
            .expect("Initialize config should succeed");
    } else {
        println!("Config already initialized at {}", config_pda);
    }

    // Fund the rent sponsor PDA so it can pay for rent reimbursements
    rpc.airdrop_lamports(&rent_sponsor(), 1_000_000_000)
        .await
        .expect("Airdrop to rent sponsor should succeed");

    // Wait for indexer to sync
    wait_for_indexer(&rpc)
        .await
        .expect("Failed to wait for indexer");

    TestEnv {
        rpc,
        payer,
        config_pda,
    }
}

/// Create a compressed mint with ATAs for recipients.
pub async fn setup_create_mint(
    rpc: &mut LightClient,
    payer: &Keypair,
    mint_authority: Pubkey,
    decimals: u8,
    recipients: Vec<(u64, Pubkey)>,
) -> (Pubkey, Vec<Pubkey>, Keypair) {
    let mint_seed = Keypair::new();
    let address_tree = rpc.get_address_tree_v2();
    let output_queue = rpc.get_random_state_tree_info().unwrap().queue;

    let compression_address = light_token::instruction::derive_mint_compressed_address(
        &mint_seed.pubkey(),
        &address_tree.tree,
    );

    let (mint, bump) = find_mint_address(&mint_seed.pubkey());

    let rpc_result = rpc
        .get_validity_proof(
            vec![],
            vec![light_client::indexer::AddressWithTree {
                address: compression_address,
                tree: address_tree.tree,
            }],
            None,
        )
        .await
        .unwrap()
        .value;

    let params = CreateMintParams {
        decimals,
        address_merkle_tree_root_index: rpc_result.addresses[0].root_index,
        mint_authority,
        proof: rpc_result.proof.0.unwrap(),
        compression_address,
        mint,
        bump,
        freeze_authority: None,
        extensions: None,
        rent_payment: 16,
        write_top_up: 766,
    };

    let create_mint_builder = CreateMint::new(
        params,
        mint_seed.pubkey(),
        payer.pubkey(),
        address_tree.tree,
        output_queue,
    );
    let instruction = create_mint_builder.instruction().unwrap();

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer, &mint_seed])
        .await
        .unwrap();

    if recipients.is_empty() {
        return (mint, vec![], mint_seed);
    }

    let mut ata_pubkeys = Vec::with_capacity(recipients.len());

    for (_amount, owner) in &recipients {
        let (ata_address, _bump) = get_associated_token_address_and_bump(owner, &mint);
        ata_pubkeys.push(ata_address);

        let create_ata = CreateAssociatedTokenAccount::new(payer.pubkey(), *owner, mint);
        let ata_instruction = create_ata.instruction().unwrap();

        rpc.create_and_send_transaction(&[ata_instruction], &payer.pubkey(), &[payer])
            .await
            .unwrap();
    }

    for (idx, (amount, _)) in recipients.iter().enumerate() {
        if *amount > 0 {
            let mint_instruction = MintTo {
                mint,
                destination: ata_pubkeys[idx],
                amount: *amount,
                authority: mint_authority,
                max_top_up: None,
                fee_payer: None,
            }
            .instruction()
            .unwrap();

            rpc.create_and_send_transaction(&[mint_instruction], &payer.pubkey(), &[payer])
                .await
                .unwrap();
        }
    }

    (mint, ata_pubkeys, mint_seed)
}

/// Create token mints and fund creator with initial balances.
pub async fn setup_token_mints(
    rpc: &mut LightClient,
    payer: &Keypair,
    creator: &Pubkey,
    initial_balance: u64,
) -> TokenSetup {
    let (mint_a, ata_pubkeys_a, mint_seed_a) = setup_create_mint(
        rpc,
        payer,
        payer.pubkey(),
        9,
        vec![(initial_balance, *creator)],
    )
    .await;

    let (mint_b, ata_pubkeys_b, mint_seed_b) = setup_create_mint(
        rpc,
        payer,
        payer.pubkey(),
        9,
        vec![(initial_balance, *creator)],
    )
    .await;

    // Ensure proper ordering: token_0_mint < token_1_mint
    if mint_a < mint_b {
        TokenSetup {
            token_0_mint: mint_a,
            token_1_mint: mint_b,
            token_0_mint_signer: mint_seed_a.pubkey(),
            token_1_mint_signer: mint_seed_b.pubkey(),
            creator_token_0: ata_pubkeys_a[0],
            creator_token_1: ata_pubkeys_b[0],
        }
    } else {
        TokenSetup {
            token_0_mint: mint_b,
            token_1_mint: mint_a,
            token_0_mint_signer: mint_seed_b.pubkey(),
            token_1_mint_signer: mint_seed_a.pubkey(),
            creator_token_0: ata_pubkeys_b[0],
            creator_token_1: ata_pubkeys_a[0],
        }
    }
}

// ============================================================================
// AMM Config Functions
// ============================================================================

/// Create and initialize the AMM config account.
/// Get the admin keypair for testing.
/// Must match the pubkey in lib.rs admin::ID when test-sbf feature is enabled.
pub fn get_admin_keypair() -> Keypair {
    // This generates pubkey: 4zvwRjXUKGfvwnParsHAS3HuSVzV5cA4McphgmoCtajS
    Keypair::from_seed(&[1u8; 32]).unwrap()
}

pub async fn create_amm_config(
    rpc: &mut LightClient,
    payer: &Keypair,
    admin: &Keypair,
    program_id: Pubkey,
    index: u16,
) -> Pubkey {
    let (amm_config_pda, _) = Pubkey::find_program_address(
        &[AMM_CONFIG_SEED.as_bytes(), &index.to_be_bytes()],
        &program_id,
    );

    // Check if already exists (idempotent for test reruns with persisted ledger)
    if rpc
        .get_account(amm_config_pda)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        println!("AmmConfig already exists at {}", amm_config_pda);
        return amm_config_pda;
    }

    let create_config_accounts = raydium_cp_swap::accounts::CreateAmmConfig {
        owner: admin.pubkey(),
        amm_config: amm_config_pda,
        system_program: solana_sdk::system_program::ID,
    };

    let create_config_data = raydium_cp_swap::instruction::CreateAmmConfig {
        index,
        trade_fee_rate: 2500,
        protocol_fee_rate: 1000,
        fund_fee_rate: 500,
        create_pool_fee: 0,
    };

    let create_config_ix = Instruction {
        program_id,
        accounts: create_config_accounts.to_account_metas(None),
        data: create_config_data.data(),
    };

    rpc.create_and_send_transaction(&[create_config_ix], &payer.pubkey(), &[payer, admin])
        .await
        .expect("Create AmmConfig should succeed");

    amm_config_pda
}

/// Setup the create_pool_fee account (wrapped SOL token account).
/// This account is preloaded via validator_args when spawning the validator.
/// This function now just verifies the account exists.
pub async fn setup_create_pool_fee_account(
    rpc: &mut LightClient,
    _payer: &Keypair,
    _owner: &Pubkey,
) {
    let create_pool_fee_receiver = raydium_cp_swap::create_pool_fee_receiver::ID;

    // Check if account already exists (should be preloaded by validator)
    if let Ok(Some(_)) = rpc.get_account(create_pool_fee_receiver).await {
        println!(
            "Pool fee receiver account exists at {}",
            create_pool_fee_receiver
        );
        return;
    }

    // Account was not preloaded - this is a problem
    panic!(
        "Pool fee receiver account at {} was not preloaded. Check validator_args configuration.",
        create_pool_fee_receiver
    );
}

// ============================================================================
// PDA Derivation
// ============================================================================

/// Derive all AMM PDAs for the pool.
pub fn derive_amm_pdas(
    program_id: &Pubkey,
    amm_config: &Pubkey,
    token_0_mint: &Pubkey,
    token_1_mint: &Pubkey,
    creator: &Pubkey,
) -> AmmPdas {
    let (pool_state, _) = Pubkey::find_program_address(
        &[
            POOL_SEED.as_bytes(),
            amm_config.as_ref(),
            token_0_mint.as_ref(),
            token_1_mint.as_ref(),
        ],
        program_id,
    );

    let (authority, authority_bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], program_id);

    let (observation_state, _) = Pubkey::find_program_address(
        &[OBSERVATION_SEED.as_bytes(), pool_state.as_ref()],
        program_id,
    );

    let (token_0_vault, _) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_state.as_ref(),
            token_0_mint.as_ref(),
        ],
        program_id,
    );

    let (token_1_vault, _) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_state.as_ref(),
            token_1_mint.as_ref(),
        ],
        program_id,
    );

    let (lp_mint_signer, lp_mint_signer_bump) =
        Pubkey::find_program_address(&[LP_MINT_SIGNER_SEED, pool_state.as_ref()], program_id);

    let (lp_mint, _) = find_mint_address(&lp_mint_signer);

    let (creator_lp_token, creator_lp_token_bump) =
        get_associated_token_address_and_bump(creator, &lp_mint);

    AmmPdas {
        pool_state,
        observation_state,
        authority,
        authority_bump,
        token_0_vault,
        token_1_vault,
        lp_mint_signer,
        lp_mint_signer_bump,
        lp_mint,
        creator_lp_token,
        creator_lp_token_bump,
    }
}

// ============================================================================
// Instruction Building
// ============================================================================

/// Get the create accounts proof for pool initialization.
pub async fn get_pool_create_accounts_proof(
    rpc: &LightClient,
    program_id: &Pubkey,
    pdas: &AmmPdas,
) -> CreateAccountsProofResult {
    get_create_accounts_proof(
        rpc,
        program_id,
        vec![
            CreateAccountsProofInput::pda(pdas.pool_state),
            CreateAccountsProofInput::pda(pdas.observation_state),
            CreateAccountsProofInput::mint(pdas.lp_mint_signer),
        ],
    )
    .await
    .unwrap()
}

// ============================================================================
// Address Lookup Table Helpers
// ============================================================================

/// Create an Address Lookup Table containing the given addresses.
/// Returns the LUT address and the AddressLookupTableAccount.
pub async fn create_address_lookup_table(
    rpc: &mut LightClient,
    payer: &Keypair,
    addresses: Vec<Pubkey>,
) -> AddressLookupTableAccount {
    // Wait a moment and then get a recent slot for LUT derivation
    // This helps ensure we get a slot that's actually in the SlotHashes sysvar
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Get slot with finalized commitment from the underlying client
    let recent_slot = rpc
        .client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .expect("Failed to get finalized slot");

    println!("Creating LUT with recent_slot: {}", recent_slot);

    // Create the lookup table
    let (create_ix, lut_address) =
        lut_instruction::create_lookup_table(payer.pubkey(), payer.pubkey(), recent_slot);

    rpc.create_and_send_transaction(&[create_ix], &payer.pubkey(), &[payer])
        .await
        .expect("Failed to create lookup table");

    // Wait a bit for the table to be created
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Extend the lookup table with addresses (max 30 addresses per extend)
    for chunk in addresses.chunks(30) {
        let extend_ix = lut_instruction::extend_lookup_table(
            lut_address,
            payer.pubkey(),
            Some(payer.pubkey()),
            chunk.to_vec(),
        );

        rpc.create_and_send_transaction(&[extend_ix], &payer.pubkey(), &[payer])
            .await
            .expect("Failed to extend lookup table");

        // Wait for extension to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Load and return the lookup table
    load_lookup_table(&rpc.client, &lut_address).expect("Failed to load lookup table")
}

/// Extract unique pubkeys from remaining accounts for LUT creation.
pub fn extract_lut_addresses(remaining_accounts: &[AccountMeta]) -> Vec<Pubkey> {
    remaining_accounts.iter().map(|acc| acc.pubkey).collect()
}

/// Build the Withdraw instruction.
pub fn build_withdraw_instruction(
    program_id: Pubkey,
    owner: Pubkey,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
    owner_token_0: Pubkey,
    owner_token_1: Pubkey,
    lp_token_amount: u64,
    minimum_token_0_amount: u64,
    minimum_token_1_amount: u64,
) -> Instruction {
    let accounts = raydium_cp_swap::accounts::Withdraw {
        owner,
        authority: pdas.authority,
        pool_state: pdas.pool_state,
        owner_lp_token: pdas.creator_lp_token,
        token_0_account: owner_token_0,
        token_1_account: owner_token_1,
        token_0_vault: pdas.token_0_vault,
        token_1_vault: pdas.token_1_vault,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        vault_0_mint: tokens.token_0_mint,
        vault_1_mint: tokens.token_1_mint,
        lp_mint: pdas.lp_mint,
        memo_program: spl_memo::id(),
        system_program: solana_sdk::system_program::ID,
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
        light_token_program: light_token_program_id(),
    };

    let instruction_data = raydium_cp_swap::instruction::Withdraw {
        lp_token_amount,
        minimum_token_0_amount,
        minimum_token_1_amount,
    };

    Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: instruction_data.data(),
    }
}

/// Build the Swap instruction.
pub fn build_swap_instruction(
    program_id: Pubkey,
    payer: Pubkey,
    amm_config: Pubkey,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
    input_token_account: Pubkey,
    output_token_account: Pubkey,
    is_token_0_input: bool, // true = swap 0->1, false = swap 1->0
    amount_in: u64,
    minimum_amount_out: u64,
) -> Instruction {
    let (input_vault, output_vault, input_mint, output_mint) = if is_token_0_input {
        (
            pdas.token_0_vault,
            pdas.token_1_vault,
            tokens.token_0_mint,
            tokens.token_1_mint,
        )
    } else {
        (
            pdas.token_1_vault,
            pdas.token_0_vault,
            tokens.token_1_mint,
            tokens.token_0_mint,
        )
    };

    let accounts = raydium_cp_swap::accounts::Swap {
        payer,
        authority: pdas.authority,
        amm_config,
        pool_state: pdas.pool_state,
        input_token_account,
        output_token_account,
        input_vault,
        output_vault,
        input_token_program: light_token_program_id(),
        output_token_program: light_token_program_id(),
        input_token_mint: input_mint,
        output_token_mint: output_mint,
        observation_state: pdas.observation_state,
        light_token_program: light_token_program_id(),
        system_program: solana_sdk::system_program::ID,
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
    };

    let instruction_data = raydium_cp_swap::instruction::SwapBaseInput {
        amount_in,
        minimum_amount_out,
    };

    Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: instruction_data.data(),
    }
}

/// Build the Deposit instruction.
pub fn build_deposit_instruction(
    program_id: Pubkey,
    owner: Pubkey,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
    owner_token_0: Pubkey,
    owner_token_1: Pubkey,
    lp_token_amount: u64,
    maximum_token_0_amount: u64,
    maximum_token_1_amount: u64,
) -> Instruction {
    let accounts = raydium_cp_swap::accounts::Deposit {
        owner,
        authority: pdas.authority,
        pool_state: pdas.pool_state,
        owner_lp_token: pdas.creator_lp_token,
        token_0_account: owner_token_0,
        token_1_account: owner_token_1,
        token_0_vault: pdas.token_0_vault,
        token_1_vault: pdas.token_1_vault,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        light_token_program: light_token_program_id(),
        vault_0_mint: tokens.token_0_mint,
        vault_1_mint: tokens.token_1_mint,
        lp_mint: pdas.lp_mint,
        system_program: solana_sdk::system_program::ID,
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
    };

    let instruction_data = raydium_cp_swap::instruction::Deposit {
        lp_token_amount,
        maximum_token_0_amount,
        maximum_token_1_amount,
    };

    Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: instruction_data.data(),
    }
}

/// Build the Initialize instruction.
pub fn build_initialize_instruction(
    program_id: Pubkey,
    creator: Pubkey,
    amm_config: Pubkey,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
    config_pda: Pubkey,
    proof_result: &CreateAccountsProofResult,
    init_amount_0: u64,
    init_amount_1: u64,
    open_time: u64,
) -> Instruction {
    let init_params = InitializeParams {
        init_amount_0,
        init_amount_1,
        open_time,
        create_accounts_proof: proof_result.create_accounts_proof.clone(),
        lp_mint_signer_bump: pdas.lp_mint_signer_bump,
        creator_lp_token_bump: pdas.creator_lp_token_bump,
        authority_bump: pdas.authority_bump,
    };

    let accounts = raydium_cp_swap::accounts::Initialize {
        creator,
        amm_config,
        authority: pdas.authority,
        pool_state: pdas.pool_state,
        token_0_mint: tokens.token_0_mint,
        token_1_mint: tokens.token_1_mint,
        lp_mint_signer: pdas.lp_mint_signer,
        lp_mint: pdas.lp_mint,
        creator_token_0: tokens.creator_token_0,
        creator_token_1: tokens.creator_token_1,
        creator_lp_token: pdas.creator_lp_token,
        token_0_vault: pdas.token_0_vault,
        token_1_vault: pdas.token_1_vault,
        observation_state: pdas.observation_state,
        create_pool_fee: raydium_cp_swap::create_pool_fee_receiver::ID,
        token_program: spl_token::id(),
        token_0_program: light_token_program_id(),
        token_1_program: light_token_program_id(),
        associated_token_program: light_anchor_spl::associated_token::ID,
        system_program: solana_sdk::system_program::ID,
        rent: solana_sdk::sysvar::rent::ID,
        compression_config: config_pda,
        light_token_config: LIGHT_TOKEN_CONFIG,
        pda_rent_sponsor: raydium_cp_swap::program_rent_sponsor(),
        light_token_rent_sponsor: LIGHT_TOKEN_RENT_SPONSOR,
        light_token_program: light_token_program_id(),
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
    };

    let instruction_data = raydium_cp_swap::instruction::Initialize {
        params: init_params,
    };

    Instruction {
        program_id,
        accounts: [
            accounts.to_account_metas(None),
            proof_result.remaining_accounts.clone(),
        ]
        .concat(),
        data: instruction_data.data(),
    }
}

// ============================================================================
// Assertions
// ============================================================================

/// Get the balance of a token account.
/// Supports both SPL Token and Light Token accounts.
pub async fn get_token_balance(rpc: &mut LightClient, account: Pubkey) -> u64 {
    let account_data = rpc.get_account(account).await.unwrap();
    if let Some(account) = account_data {
        // Token account layout: mint (32) + owner (32) + amount (8)
        // Works for both SPL tokens and Light tokens
        const AMOUNT_OFFSET: usize = 64;
        if account.data.len() >= AMOUNT_OFFSET + 8 {
            let amount_bytes = &account.data[AMOUNT_OFFSET..AMOUNT_OFFSET + 8];
            u64::from_le_bytes(amount_bytes.try_into().unwrap())
        } else {
            0
        }
    } else {
        0
    }
}

/// Assert that an account exists on-chain.
pub async fn assert_onchain_exists(rpc: &mut LightClient, pda: &Pubkey) {
    assert!(
        rpc.get_account(*pda).await.unwrap().is_some(),
        "Account {} should exist on-chain",
        pda
    );
}

/// Assert that an account is closed (doesn't exist or has 0 lamports).
pub async fn assert_onchain_closed(rpc: &mut LightClient, pda: &Pubkey) {
    let acc = rpc.get_account(*pda).await.unwrap();
    assert!(
        acc.is_none() || acc.unwrap().lamports == 0,
        "Account {} should be closed",
        pda
    );
}

/// Assert all pool accounts exist on-chain (hot or decompressed state).
pub async fn assert_pool_accounts_exist(
    rpc: &mut LightClient,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
) {
    assert_onchain_exists(rpc, &pdas.pool_state).await;
    assert_onchain_exists(rpc, &pdas.observation_state).await;
    assert_onchain_exists(rpc, &pdas.lp_mint).await;
    assert_onchain_exists(rpc, &pdas.token_0_vault).await;
    assert_onchain_exists(rpc, &pdas.token_1_vault).await;
    assert_onchain_exists(rpc, &pdas.creator_lp_token).await;
    assert_onchain_exists(rpc, &tokens.token_0_mint).await;
    assert_onchain_exists(rpc, &tokens.token_1_mint).await;
}

/// Assert all pool accounts are compressed (closed on-chain).
pub async fn assert_pool_accounts_compressed(
    rpc: &mut LightClient,
    pdas: &AmmPdas,
    tokens: &TokenSetup,
) {
    assert_onchain_closed(rpc, &pdas.pool_state).await;
    assert_onchain_closed(rpc, &pdas.observation_state).await;
    assert_onchain_closed(rpc, &pdas.lp_mint).await;
    assert_onchain_closed(rpc, &pdas.token_0_vault).await;
    assert_onchain_closed(rpc, &pdas.token_1_vault).await;
    assert_onchain_closed(rpc, &pdas.creator_lp_token).await;
    assert_onchain_closed(rpc, &tokens.token_0_mint).await;
    assert_onchain_closed(rpc, &tokens.token_1_mint).await;
}

/// Verify that the pool was initialized correctly.
pub async fn assert_pool_initialized(rpc: &mut LightClient, pdas: &AmmPdas) {
    let pool_account = rpc.get_account(pdas.pool_state).await.unwrap();
    assert!(pool_account.is_some(), "Pool state should exist");

    let observation_account = rpc.get_account(pdas.observation_state).await.unwrap();
    assert!(
        observation_account.is_some(),
        "Observation state should exist"
    );
}

/// Assert that deposit succeeded by checking LP token balance increased.
pub async fn assert_deposit_succeeded(
    rpc: &mut LightClient,
    owner_lp_token: Pubkey,
    lp_balance_before: u64,
    expected_lp_increase: u64,
) {
    let lp_balance_after = get_token_balance(rpc, owner_lp_token).await;
    let actual_increase = lp_balance_after.saturating_sub(lp_balance_before);
    assert!(
        actual_increase >= expected_lp_increase,
        "LP token balance should increase by at least {}. Before: {}, After: {}, Actual increase: {}",
        expected_lp_increase,
        lp_balance_before,
        lp_balance_after,
        actual_increase
    );
}

/// Assert that swap succeeded by checking balances changed correctly.
pub async fn assert_swap_succeeded(
    rpc: &mut LightClient,
    input_account: Pubkey,
    output_account: Pubkey,
    input_balance_before: u64,
    output_balance_before: u64,
    expected_input_decrease: u64,
    min_output_increase: u64,
) {
    let input_balance_after = get_token_balance(rpc, input_account).await;
    let output_balance_after = get_token_balance(rpc, output_account).await;

    let actual_input_decrease = input_balance_before.saturating_sub(input_balance_after);
    let actual_output_increase = output_balance_after.saturating_sub(output_balance_before);

    assert_eq!(
        actual_input_decrease, expected_input_decrease,
        "Input token balance should decrease by {}. Before: {}, After: {}",
        expected_input_decrease, input_balance_before, input_balance_after
    );

    assert!(
        actual_output_increase >= min_output_increase,
        "Output token balance should increase by at least {}. Before: {}, After: {}, Actual: {}",
        min_output_increase,
        output_balance_before,
        output_balance_after,
        actual_output_increase
    );
}

/// Assert that withdraw succeeded by checking LP token balance decreased.
pub async fn assert_withdraw_succeeded(
    rpc: &mut LightClient,
    owner_lp_token: Pubkey,
    lp_balance_before: u64,
    expected_lp_decrease: u64,
) {
    let lp_balance_after = get_token_balance(rpc, owner_lp_token).await;
    let actual_decrease = lp_balance_before.saturating_sub(lp_balance_after);
    assert_eq!(
        actual_decrease, expected_lp_decrease,
        "LP token balance should decrease by {}. Before: {}, After: {}",
        expected_lp_decrease, lp_balance_before, lp_balance_after
    );
}

/// Verify that the AMM config was created.
pub async fn assert_amm_config_created(rpc: &mut LightClient, amm_config: Pubkey) {
    let account = rpc.get_account(amm_config).await.unwrap();
    assert!(account.is_some(), "AmmConfig account should exist");
}

// ============================================================================
// Manual Compression Helpers
// ============================================================================

/// Manually compress a PDA account using compress_accounts_idempotent instruction.
/// This bypasses the forester timing check and compresses the account immediately.
/// Useful for testing when you can't wait for the forester's epoch-based timing.
pub async fn compress_pda_account(
    rpc: &mut LightClient,
    payer: &Keypair,
    program_id: &Pubkey,
    pda_pubkey: &Pubkey,
    config_pda: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    use anchor_lang::AnchorDeserialize;

    // Get the LightConfig to find rent_sponsor and address_tree
    let cfg_acc = rpc
        .get_account(*config_pda)
        .await?
        .ok_or("Config account not found")?;
    let cfg = LightConfig::deserialize(&mut &cfg_acc.data[DISCRIMINATOR_LEN..])
        .map_err(|e| format!("Failed to deserialize config: {:?}", e))?;

    let rent_sponsor = cfg.rent_sponsor;
    let compression_authority = payer.pubkey();
    let address_tree = cfg.address_space[0];

    // Derive the compressed address
    let compressed_address = derive_address(
        &pda_pubkey.to_bytes(),
        &address_tree.to_bytes(),
        &program_id.to_bytes(),
    );

    // Get the compressed account from indexer
    let compressed_account = rpc
        .get_compressed_account(compressed_address, None)
        .await?
        .value
        .ok_or_else(|| format!("Compressed account not found for PDA {}", pda_pubkey))?;

    // Get validity proof
    let proof_with_context = rpc
        .get_validity_proof(vec![compressed_account.hash], vec![], None)
        .await?
        .value;

    // Build program metas for compress_accounts_idempotent
    let program_metas = vec![
        AccountMeta::new(payer.pubkey(), true),        // fee_payer
        AccountMeta::new_readonly(*config_pda, false), // config
        AccountMeta::new(rent_sponsor, false),         // rent_sponsor
        AccountMeta::new_readonly(compression_authority, false), // compression_authority
    ];

    // Build compress instruction
    let ix = build_compress_accounts_idempotent(
        program_id,
        &COMPRESS_ACCOUNTS_IDEMPOTENT_DISCRIMINATOR,
        &[*pda_pubkey],
        &program_metas,
        proof_with_context,
    )?;

    // Send transaction
    rpc.create_and_send_transaction(&[ix], &payer.pubkey(), &[payer])
        .await?;

    println!("Compressed PDA {} successfully", pda_pubkey);
    Ok(())
}

/// Compress multiple PDA accounts for the cp-swap pool.
/// This manually compresses pool_state and observation_state.
pub async fn compress_pool_pda_accounts(
    rpc: &mut LightClient,
    payer: &Keypair,
    program_id: &Pubkey,
    pdas: &AmmPdas,
    config_pda: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    // Compress pool_state
    compress_pda_account(rpc, payer, program_id, &pdas.pool_state, config_pda).await?;

    // Wait for indexer to sync
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Compress observation_state
    compress_pda_account(rpc, payer, program_id, &pdas.observation_state, config_pda).await?;

    Ok(())
}

// ============================================================================
// Unified Setup Functions for SDK-based Tests
// ============================================================================

/// Complete pool setup result containing all necessary state.
pub struct PoolSetup {
    pub env: TestEnv,
    pub creator: Keypair,
    pub tokens: TokenSetup,
    pub amm_config: Pubkey,
    pub pdas: AmmPdas,
}

/// Setup a complete pool environment in a single call.
pub async fn setup_pool_environment(program_id: Pubkey, amm_config_index: u16) -> PoolSetup {
    setup_pool_environment_inner(program_id, amm_config_index, false).await
}

/// Setup a complete pool environment with forester for auto-compression tests.
pub async fn setup_pool_environment_with_forester(
    program_id: Pubkey,
    amm_config_index: u16,
) -> PoolSetup {
    setup_pool_environment_inner(program_id, amm_config_index, true).await
}

/// Internal helper for pool environment setup.
async fn setup_pool_environment_inner(
    program_id: Pubkey,
    amm_config_index: u16,
    with_forester: bool,
) -> PoolSetup {
    let mut env = if with_forester {
        setup_test_environment_with_forester(program_id).await
    } else {
        setup_test_environment(program_id).await
    };

    let creator = Keypair::new();
    env.rpc
        .airdrop_lamports(&creator.pubkey(), 100_000_000_000)
        .await
        .unwrap();

    let admin = get_admin_keypair();
    env.rpc
        .airdrop_lamports(&admin.pubkey(), 10_000_000_000)
        .await
        .unwrap();

    let initial_balance = 1_000_000;
    let tokens =
        setup_token_mints(&mut env.rpc, &env.payer, &creator.pubkey(), initial_balance).await;

    let amm_config = create_amm_config(
        &mut env.rpc,
        &env.payer,
        &admin,
        program_id,
        amm_config_index,
    )
    .await;
    setup_create_pool_fee_account(&mut env.rpc, &env.payer, &env.payer.pubkey()).await;

    let pdas = derive_amm_pdas(
        &program_id,
        &amm_config,
        &tokens.token_0_mint,
        &tokens.token_1_mint,
        &creator.pubkey(),
    );

    PoolSetup {
        env,
        creator,
        tokens,
        amm_config,
        pdas,
    }
}
