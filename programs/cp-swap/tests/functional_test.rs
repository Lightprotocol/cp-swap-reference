/// Functional integration test for cp-swap program.
/// Tests pool initialization with light-program-test framework.
use light_client::interface::AccountInterfaceExt;
use light_program_test::program_test::TestRpc;
use light_program_test::Rpc;
use solana_keypair::Keypair;
use solana_signer::Signer;

mod helpers;
mod program;
use helpers::*;

#[tokio::test]
async fn test_full_lifecycle() {
    let program_id = raydium_cp_swap::ID;

    // ========================================================================
    // Setup
    // ========================================================================
    let mut env = setup_test_environment(program_id).await;

    // Create and fund creator with more lamports for multiple transactions
    let creator = Keypair::new();
    env.rpc
        .airdrop_lamports(&creator.pubkey(), 100_000_000_000)
        .await
        .unwrap();

    // Get admin keypair and fund it
    let admin = get_admin_keypair();
    env.rpc
        .airdrop_lamports(&admin.pubkey(), 10_000_000_000)
        .await
        .unwrap();

    // Setup token mints with larger initial balance for lifecycle operations
    let initial_balance = 1_000_000;
    let tokens =
        setup_token_mints(&mut env.rpc, &env.payer, &creator.pubkey(), initial_balance).await;

    // Create AMM config (use index 1 to avoid collision with test_initialize_pool)
    let amm_config = create_amm_config(&mut env.rpc, &env.payer, &admin, program_id, 1).await;
    assert_amm_config_created(&mut env.rpc, amm_config).await;

    // Setup create pool fee account
    setup_create_pool_fee_account(&mut env.rpc, &env.payer.pubkey());

    // Derive PDAs
    let pdas = derive_amm_pdas(
        &program_id,
        &amm_config,
        &tokens.token_0_mint,
        &tokens.token_1_mint,
        &creator.pubkey(),
    );

    // ========================================================================
    // Initialize Pool
    // ========================================================================
    let proof_result = get_pool_create_accounts_proof(&env.rpc, &program_id, &pdas).await;

    let init_amount_0 = 100_000;
    let init_amount_1 = 100_000;

    let init_instruction = build_initialize_instruction(
        program_id,
        creator.pubkey(),
        amm_config,
        &pdas,
        &tokens,
        env.config_pda,
        &proof_result,
        init_amount_0,
        init_amount_1,
        0, // open_time = 0 (immediate)
    );

    env.rpc
        .create_and_send_transaction(&[init_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Initialize should succeed");

    assert_pool_initialized(&mut env.rpc, &pdas).await;

    // Check initial LP token balance (should have received initial LP tokens from initialize)
    let lp_balance_after_init = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;
    println!("LP balance after init: {}", lp_balance_after_init);
    assert!(
        lp_balance_after_init > 0,
        "Should have received LP tokens from initialization"
    );

    // ========================================================================
    // Deposit
    // ========================================================================
    let lp_balance_before_deposit = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;

    // Deposit: request LP tokens, allow 10% slippage on tokens provided
    let deposit_lp_amount = 500;
    let max_token_0 = 10_000; // Allow generous slippage
    let max_token_1 = 10_000;

    let deposit_instruction = build_deposit_instruction(
        program_id,
        creator.pubkey(),
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        deposit_lp_amount,
        max_token_0,
        max_token_1,
        SplInterfaceInfo::default(),
    );

    env.rpc
        .create_and_send_transaction(&[deposit_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Deposit should succeed");

    assert_deposit_succeeded(
        &mut env.rpc,
        pdas.creator_lp_token,
        lp_balance_before_deposit,
        deposit_lp_amount,
    )
    .await;

    println!(
        "Deposit succeeded. LP balance: {}",
        get_token_balance(&mut env.rpc, pdas.creator_lp_token).await
    );

    // ========================================================================
    // Swap (token_0 -> token_1)
    // ========================================================================
    // Warp time forward so pool is open for swaps (open_time = block_timestamp + 1)
    env.rpc.warp_to_slot(100).unwrap();

    let token_0_balance_before = get_token_balance(&mut env.rpc, tokens.creator_token_0).await;
    let token_1_balance_before = get_token_balance(&mut env.rpc, tokens.creator_token_1).await;

    // Swap: 100 token_0 for token_1, allow 50% slippage
    let swap_amount_in = 100;
    let min_amount_out = 1; // Allow high slippage for test stability

    let swap_instruction = build_swap_instruction(
        program_id,
        creator.pubkey(),
        amm_config,
        &pdas,
        &tokens,
        tokens.creator_token_0, // input
        tokens.creator_token_1, // output
        true,                   // is_token_0_input
        swap_amount_in,
        min_amount_out,
        SplInterfaceInfo::default(),
        light_token_program_id(),
        light_token_program_id(),
    );

    env.rpc
        .create_and_send_transaction(&[swap_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Swap should succeed");

    assert_swap_succeeded(
        &mut env.rpc,
        tokens.creator_token_0,
        tokens.creator_token_1,
        token_0_balance_before,
        token_1_balance_before,
        swap_amount_in,
        min_amount_out,
    )
    .await;

    println!(
        "Swap succeeded. Token 0 balance: {}, Token 1 balance: {}",
        get_token_balance(&mut env.rpc, tokens.creator_token_0).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_1).await
    );

    // ========================================================================
    // Withdraw (burn half of LP tokens)
    // ========================================================================
    let lp_balance_before_withdraw = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;
    let withdraw_lp_amount = lp_balance_before_withdraw / 2;

    // Allow any amount of tokens out (0 minimum)
    let withdraw_instruction = build_withdraw_instruction(
        program_id,
        creator.pubkey(),
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        withdraw_lp_amount,
        0, // minimum_token_0_amount - accept any
        0, // minimum_token_1_amount - accept any
        SplInterfaceInfo::default(),
    );

    env.rpc
        .create_and_send_transaction(&[withdraw_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Withdraw should succeed");

    assert_withdraw_succeeded(
        &mut env.rpc,
        pdas.creator_lp_token,
        lp_balance_before_withdraw,
        withdraw_lp_amount,
    )
    .await;

    println!(
        "Withdraw succeeded. LP balance: {}, Token 0 balance: {}, Token 1 balance: {}",
        get_token_balance(&mut env.rpc, pdas.creator_lp_token).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_0).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_1).await
    );

    println!("Full lifecycle test completed successfully!");
}

// ============================================================================
// Token Type Pool Tests - Full Lifecycle
// ============================================================================

/// Generic full lifecycle test for any token type combination.
/// Tests: Initialize -> Deposit -> Swap -> Withdraw
async fn test_pool_with_token_types(type_a: TokenType, type_b: TokenType, config_index: u16) {
    let test_name = format!("{:?} + {:?}", type_a, type_b);
    println!("\n=== {} Full Lifecycle ===", test_name);

    let program_id = raydium_cp_swap::ID;

    // Setup environment
    let mut env = setup_test_environment(program_id).await;

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

    // Create token pair with specified types
    let initial_balance = 1_000_000;
    let flex_tokens = setup_token_pair(
        &mut env.rpc,
        &env.payer,
        &creator.pubkey(),
        initial_balance,
        type_a,
        type_b,
    )
    .await;

    println!(
        "Tokens: 0={:?}, 1={:?}",
        flex_tokens.token_0_type, flex_tokens.token_1_type
    );

    // Create SPL interface PDAs for any SPL/Token-2022 mints
    create_spl_interface_pdas_for_setup(&mut env.rpc, &env.payer, &flex_tokens).await;

    let tokens = flex_tokens.to_token_setup();
    let spl_interface = flex_tokens.build_spl_interface();

    // Create AMM config
    let amm_config =
        create_amm_config(&mut env.rpc, &env.payer, &admin, program_id, config_index).await;
    assert_amm_config_created(&mut env.rpc, amm_config).await;
    setup_create_pool_fee_account(&mut env.rpc, &env.payer.pubkey());

    let pdas = derive_amm_pdas(
        &program_id,
        &amm_config,
        &tokens.token_0_mint,
        &tokens.token_1_mint,
        &creator.pubkey(),
    );

    // ========================================================================
    // Initialize
    // ========================================================================
    let proof_result = get_pool_create_accounts_proof(&env.rpc, &program_id, &pdas).await;

    let init_instruction = build_initialize_instruction_with_spl(
        program_id,
        creator.pubkey(),
        amm_config,
        &pdas,
        &tokens,
        env.config_pda,
        &proof_result,
        100_000,
        100_000,
        0,
        flex_tokens.token_0_type.program_id(),
        flex_tokens.token_1_type.program_id(),
        spl_interface.clone(),
    );

    env.rpc
        .create_and_send_transaction(&[init_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Initialize should succeed");

    assert_pool_initialized(&mut env.rpc, &pdas).await;
    let lp_after_init = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;
    assert!(lp_after_init > 0, "Should have LP tokens after init");
    println!("[Init] LP: {}", lp_after_init);

    // ========================================================================
    // Deposit
    // ========================================================================
    let lp_before_deposit = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;

    let deposit_instruction = build_deposit_instruction(
        program_id,
        creator.pubkey(),
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        500,    // lp_amount
        10_000, // max_token_0
        10_000, // max_token_1
        spl_interface.clone(),
    );

    env.rpc
        .create_and_send_transaction(&[deposit_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Deposit should succeed");

    assert_deposit_succeeded(&mut env.rpc, pdas.creator_lp_token, lp_before_deposit, 500).await;
    println!(
        "[Deposit] LP: {}",
        get_token_balance(&mut env.rpc, pdas.creator_lp_token).await
    );

    // ========================================================================
    // Swap (token_0 -> token_1)
    // ========================================================================
    env.rpc.warp_to_slot(100).unwrap();

    let t0_before = get_token_balance(&mut env.rpc, tokens.creator_token_0).await;
    let t1_before = get_token_balance(&mut env.rpc, tokens.creator_token_1).await;

    let swap_instruction = build_swap_instruction(
        program_id,
        creator.pubkey(),
        amm_config,
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        true, // is_token_0_input
        100,  // amount_in
        1,    // min_amount_out
        spl_interface.clone(),
        flex_tokens.token_0_type.program_id(),
        flex_tokens.token_1_type.program_id(),
    );

    env.rpc
        .create_and_send_transaction(&[swap_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Swap should succeed");

    assert_swap_succeeded(
        &mut env.rpc,
        tokens.creator_token_0,
        tokens.creator_token_1,
        t0_before,
        t1_before,
        100,
        1,
    )
    .await;
    println!(
        "[Swap] T0: {}, T1: {}",
        get_token_balance(&mut env.rpc, tokens.creator_token_0).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_1).await
    );

    // ========================================================================
    // Withdraw
    // ========================================================================
    let lp_before_withdraw = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;
    let withdraw_amount = lp_before_withdraw / 2;

    let withdraw_instruction = build_withdraw_instruction(
        program_id,
        creator.pubkey(),
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        withdraw_amount,
        0,
        0,
        spl_interface,
    );

    env.rpc
        .create_and_send_transaction(&[withdraw_instruction], &creator.pubkey(), &[&creator])
        .await
        .expect("Withdraw should succeed");

    assert_withdraw_succeeded(
        &mut env.rpc,
        pdas.creator_lp_token,
        lp_before_withdraw,
        withdraw_amount,
    )
    .await;
    println!(
        "[Withdraw] LP: {}, T0: {}, T1: {}",
        get_token_balance(&mut env.rpc, pdas.creator_lp_token).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_0).await,
        get_token_balance(&mut env.rpc, tokens.creator_token_1).await
    );

    println!("=== {} Full Lifecycle PASSED ===\n", test_name);
}

#[tokio::test]
async fn test_pool_spl_light() {
    test_pool_with_token_types(TokenType::Spl, TokenType::Light, 10).await;
}

#[tokio::test]
async fn test_pool_spl_spl() {
    test_pool_with_token_types(TokenType::Spl, TokenType::Spl, 11).await;
}

#[tokio::test]
async fn test_pool_spl_token2022() {
    test_pool_with_token_types(TokenType::Spl, TokenType::Token2022, 12).await;
}

#[tokio::test]
async fn test_pool_light_token2022() {
    test_pool_with_token_types(TokenType::Light, TokenType::Token2022, 13).await;
}

/// Test SDK initialization from fetched accounts and account requirements.
#[tokio::test]
async fn test_sdk_from_keyed_accounts() {
    use light_client::interface::LightProgramInterface;
    use program::{CpSwapInstruction, CpSwapSdk};

    let program_id = raydium_cp_swap::ID;

    // Setup environment and initialize pool
    let mut setup = setup_pool_environment(program_id, 2).await;

    // Initialize pool first (SDK requires actual account data)
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
    setup
        .env
        .rpc
        .create_and_send_transaction(&[init_ix], &setup.creator.pubkey(), &[&setup.creator])
        .await
        .expect("Initialize should succeed");

    // Fetch pool state account
    let pool_interface = setup
        .env
        .rpc
        .get_account_interface(&setup.pdas.pool_state, &program_id)
        .await
        .expect("get_account_interface should succeed");

    // Create SDK from fetched account
    let sdk = CpSwapSdk::from_keyed_accounts(&[pool_interface])
        .expect("from_keyed_accounts should succeed");

    // Verify SDK parsed addresses match expected
    assert_eq!(sdk.pool_state_pubkey, Some(setup.pdas.pool_state));
    assert_eq!(sdk.observation_key, Some(setup.pdas.observation_state));
    assert_eq!(sdk.token_0_vault, Some(setup.pdas.token_0_vault));
    assert_eq!(sdk.token_1_vault, Some(setup.pdas.token_1_vault));
    assert_eq!(sdk.lp_mint, Some(setup.pdas.lp_mint));
    assert_eq!(sdk.amm_config, Some(setup.amm_config));
    assert_eq!(sdk.token_0_mint, Some(setup.tokens.token_0_mint));
    assert_eq!(sdk.token_1_mint, Some(setup.tokens.token_1_mint));

    // Check account requirements for each instruction type
    let swap_accounts = sdk.get_accounts_for_instruction(CpSwapInstruction::Swap);
    assert_eq!(
        swap_accounts.len(),
        6,
        "Swap needs 6 accounts: pool, observation, vault0, vault1, mint0, mint1"
    );

    let deposit_accounts = sdk.get_accounts_for_instruction(CpSwapInstruction::Deposit);
    assert_eq!(
        deposit_accounts.len(),
        7,
        "Deposit needs 7 accounts: pool, observation, vault0, vault1, lp_mint, mint0, mint1"
    );

    let withdraw_accounts = sdk.get_accounts_for_instruction(CpSwapInstruction::Withdraw);
    assert_eq!(
        withdraw_accounts.len(),
        7,
        "Withdraw needs 7 accounts: pool, observation, vault0, vault1, lp_mint, mint0, mint1"
    );

    // Verify program_id method
    assert_eq!(sdk.program_id(), program_id);

    println!("SDK initialization test completed successfully!");
}
