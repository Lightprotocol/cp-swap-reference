/// Clean integration test for cp-swap using CpSwapSdk.
/// Tests the full lifecycle: Initialize -> Wait -> Compress -> Load -> Execute Operations
use light_client::interface::{create_load_instructions, AccountSpec, LightProgramInterface};
use light_client::rpc::Rpc;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::Instruction;
use solana_sdk::transaction::Transaction;
use solana_signer::Signer;

mod helpers;
mod program;

use helpers::*;
use program::{CpSwapInstruction, CpSwapSdk};

fn log_transaction_size(name: &str, ixs: &[Instruction]) {
    let tx = Transaction::new_with_payer(ixs, None);
    let serialized = bincode::serialize(&tx).expect("Failed to serialize transaction");
    println!(
        "{}: {} bytes ({} instructions)",
        name,
        serialized.len(),
        ixs.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_sdk_lifecycle() {
    let program_id = raydium_cp_swap::ID;

    // ==================== PHASE 1: Setup & Initialize Pool ====================
    // Use forester-enabled environment for auto-compression
    let mut setup = setup_pool_environment_with_forester(program_id, 10).await;

    let proof_result =
        get_pool_create_accounts_proof(&setup.env.rpc, &program_id, &setup.pdas).await;
    let init_ix = build_initialize_instruction(
        program_id,
        setup.creator.pubkey(),
        setup.amm_config,
        &setup.pdas,
        &setup.tokens,
        setup.env.config_pda,
        &proof_result,
        100_000,
        100_000,
        0,
    );
    log_transaction_size("Initialize transaction", &[init_ix.clone()]);

    // Create Address Lookup Table for the initialize transaction
    let lut_addresses = extract_lut_addresses(&proof_result.remaining_accounts);
    let lut =
        create_address_lookup_table(&mut setup.env.rpc, &setup.env.payer, lut_addresses).await;

    // Add compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

    setup
        .env
        .rpc
        .create_and_send_versioned_transaction(
            &[compute_budget_ix, init_ix],
            &setup.creator.pubkey(),
            &[&setup.creator],
            &[lut],
        )
        .await
        .unwrap();

    // ==================== PHASE 2: Verify Hot Accounts Exist ====================
    assert_pool_accounts_exist(&mut setup.env.rpc, &setup.pdas, &setup.tokens).await;

    // ==================== PHASE 3: Manual Compression ====================
    // The forester's compression timing is based on SLOTS_PER_EPOCH (13500) which is
    // hardcoded in light-compressible crate. We can't change this constant at runtime,
    // so instead we manually compress the PDA accounts using compress_accounts_idempotent.
    // This simulates what the forester would do after rent expires.
    println!("Manually compressing pool PDA accounts...");

    // Wait a moment for indexer to sync compressed account states
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    compress_pool_pda_accounts(
        &mut setup.env.rpc,
        &setup.env.payer,
        &program_id,
        &setup.pdas,
        &setup.env.config_pda,
    )
    .await
    .expect("Manual compression should succeed");

    // Wait for indexer to process the compression
    println!("Waiting for indexer to process compression...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    wait_for_indexer(&setup.env.rpc)
        .await
        .expect("Indexer should sync after compression");

    // ==================== PHASE 4: Assert PDA Accounts Are Compressed ====================
    // Note: Only pool_state and observation_state are compressed by our helper.
    // Other accounts (mints, vaults, ATAs) are handled differently.
    assert_onchain_closed(&mut setup.env.rpc, &setup.pdas.pool_state).await;
    assert_onchain_closed(&mut setup.env.rpc, &setup.pdas.observation_state).await;
    println!("Pool accounts verified as closed on-chain");

    // ==================== PHASE 5: Create SDK from Compressed State ====================
    // Now that Photon supports looking up fully compressed accounts by their PDA pubkey,
    // we can use get_account_interface directly.
    println!("Fetching compressed pool state via get_account_interface...");

    let pool_interface = setup
        .env
        .rpc
        .get_account_interface(&setup.pdas.pool_state, None)
        .await
        .expect("get_account_interface should succeed")
        .value
        .expect("pool_state should be found via get_account_interface");

    let data = pool_interface.data();
    println!(
        "Found pool_state via get_account_interface: key={}, is_cold={}, data_len={}, discriminator={:?}",
        pool_interface.key,
        pool_interface.is_cold(),
        data.len(),
        if data.len() >= 8 { &data[..8] } else { data }
    );
    assert!(
        pool_interface.is_cold(),
        "pool_state should be cold after compression"
    );

    // Debug: check if discriminator matches PoolState::LIGHT_DISCRIMINATOR
    if data.len() >= 8 {
        let disc: [u8; 8] = data[..8].try_into().unwrap();
        println!("Discriminator bytes: {:?}", disc);
        // PoolState::LIGHT_DISCRIMINATOR is [0, 236, 227, 245, 215, 195, 222, 70] from program.rs
        println!("Expected PoolState::LIGHT_DISCRIMINATOR: [0, 236, 227, 245, 215, 195, 222, 70]");
    }

    // Debug: print the account info
    println!("About to call from_keyed_accounts with {} accounts", 1);
    println!("Account key: {}", pool_interface.key);
    println!("Account data len: {}", pool_interface.data().len());
    if pool_interface.data().len() >= 8 {
        let disc: [u8; 8] = pool_interface.data()[..8].try_into().unwrap();
        println!("Account discriminator from data(): {:?}", disc);
    }

    let mut sdk = CpSwapSdk::from_keyed_accounts(&[pool_interface])
        .expect("from_keyed_accounts should succeed");

    // ==================== PHASE 6: Fetch & Update SDK ====================
    let accounts_to_fetch = sdk.get_accounts_to_update(&CpSwapInstruction::Deposit);
    let keyed_accounts = setup
        .env
        .rpc
        .fetch_accounts(&accounts_to_fetch, None)
        .await
        .expect("fetch_accounts should succeed");

    sdk.update(&keyed_accounts)
        .expect("sdk.update should succeed");

    // ==================== PHASE 7: Build Specs for Load ====================
    let mut all_specs = sdk.get_specs_for_instruction(&CpSwapInstruction::Deposit);

    // Fetch creator's ATAs and add to specs
    // These are Light Token accounts (on-chain), so they should return as hot
    let creator_lp_ata_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.pdas.lp_mint, None)
        .await
        .expect("get_ata_interface for creator_lp_token should succeed")
        .value
        .expect("creator_lp_token should exist");
    all_specs.push(AccountSpec::Ata(creator_lp_ata_interface));

    let creator_token_0_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.tokens.token_0_mint, None)
        .await
        .expect("get_ata_interface for creator_token_0 should succeed")
        .value
        .expect("creator_token_0 should exist");
    all_specs.push(AccountSpec::Ata(creator_token_0_interface));

    let creator_token_1_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.tokens.token_1_mint, None)
        .await
        .expect("get_ata_interface for creator_token_1 should succeed")
        .value
        .expect("creator_token_1 should exist");
    all_specs.push(AccountSpec::Ata(creator_token_1_interface));

    // ==================== PHASE 8: Create Load Instructions ====================
    // Debug: print tree info from the specs
    for spec in &all_specs {
        if let AccountSpec::Pda(pda_spec) = spec {
            if let Some(compressed) = pda_spec.compressed() {
                println!(
                    "DEBUG: PDA spec tree_info: tree={}, queue={}, tree_type={:?}",
                    compressed.tree_info.tree,
                    compressed.tree_info.queue,
                    compressed.tree_info.tree_type
                );
            }
        }
    }

    let all_load_ixs = create_load_instructions(
        &all_specs,
        setup.env.payer.pubkey(),
        setup.env.config_pda,
        &setup.env.rpc,
    )
    .await
    .expect("create_load_instructions should succeed");

    // ==================== PHASE 9: Execute Load ====================
    log_transaction_size("Load transaction", &all_load_ixs);

    // Debug: Print all required signers for the instructions
    println!("DEBUG: Required signers in instructions:");
    let mut required_signers = std::collections::HashSet::new();
    for ix in &all_load_ixs {
        for meta in &ix.accounts {
            if meta.is_signer {
                required_signers.insert(meta.pubkey);
            }
        }
    }
    for signer in &required_signers {
        println!("  Required: {}", signer);
    }
    println!("DEBUG: Provided signers:");
    println!("  payer: {}", setup.env.payer.pubkey());
    println!("  creator: {}", setup.creator.pubkey());

    // Execute load transaction
    // Note: ATAs are returned as hot (on-chain) since Light Token accounts exist on-chain.
    // Only the PDA decompress instructions are generated for cold PDAs.
    // Use only payer if that's the only required signer
    let signers = if required_signers.contains(&setup.creator.pubkey()) {
        println!("DEBUG: Adding creator to signers");
        vec![&setup.env.payer, &setup.creator]
    } else {
        println!("DEBUG: Only payer needed");
        vec![&setup.env.payer]
    };

    setup
        .env
        .rpc
        .create_and_send_transaction(&all_load_ixs, &setup.env.payer.pubkey(), &signers)
        .await
        .expect("Load should succeed");

    // ==================== PHASE 10: Verify Accounts Are Loaded ====================
    assert_pool_accounts_exist(&mut setup.env.rpc, &setup.pdas, &setup.tokens).await;

    // ==================== PHASE 11: Execute Operations ====================
    // Deposit
    let deposit_ix = build_deposit_instruction(
        program_id,
        setup.creator.pubkey(),
        &setup.pdas,
        &setup.tokens,
        setup.tokens.creator_token_0,
        setup.tokens.creator_token_1,
        500,
        10_000,
        10_000,
    );
    log_transaction_size("Deposit transaction", &[deposit_ix.clone()]);

    // Log combined Load + Deposit
    let mut load_plus_deposit = all_load_ixs.clone();
    load_plus_deposit.push(deposit_ix.clone());
    log_transaction_size("Load + Deposit transaction", &load_plus_deposit);

    setup
        .env
        .rpc
        .create_and_send_transaction(&[deposit_ix], &setup.creator.pubkey(), &[&setup.creator])
        .await
        .unwrap();

    // Swap
    let swap_ix = build_swap_instruction(
        program_id,
        setup.creator.pubkey(),
        setup.amm_config,
        &setup.pdas,
        &setup.tokens,
        setup.tokens.creator_token_0,
        setup.tokens.creator_token_1,
        true,
        100,
        1,
    );
    log_transaction_size("Swap transaction", &[swap_ix.clone()]);

    // Log combined Load + Swap
    let mut load_plus_swap = all_load_ixs.clone();
    load_plus_swap.push(swap_ix.clone());
    log_transaction_size("Load + Swap transaction", &load_plus_swap);

    setup
        .env
        .rpc
        .create_and_send_transaction(&[swap_ix], &setup.creator.pubkey(), &[&setup.creator])
        .await
        .unwrap();

    // Withdraw
    let lp_balance = get_token_balance(&mut setup.env.rpc, setup.pdas.creator_lp_token).await;
    let withdraw_ix = build_withdraw_instruction(
        program_id,
        setup.creator.pubkey(),
        &setup.pdas,
        &setup.tokens,
        setup.tokens.creator_token_0,
        setup.tokens.creator_token_1,
        lp_balance / 2,
        0,
        0,
    );
    setup
        .env
        .rpc
        .create_and_send_transaction(&[withdraw_ix], &setup.creator.pubkey(), &[&setup.creator])
        .await
        .unwrap();
}
