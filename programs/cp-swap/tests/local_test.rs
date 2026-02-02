//! Local integration tests for cp-swap program using LightProgramTest.
//! These tests run with LiteSVM (no external validator required).

use light_client::rpc::Rpc;
use solana_keypair::Keypair;
use solana_signer::Signer;

mod helpers_local;
use helpers_local::*;

/// Test the full pool lifecycle: Initialize -> Deposit -> Swap -> Withdraw
#[tokio::test]
async fn test_full_lifecycle_local() {
    let program_id = raydium_cp_swap::ID;

    // ========================================================================
    // Setup
    // ========================================================================
    let mut env = setup_test_environment(program_id).await;

    // Create and fund creator
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

    // Setup token mints with initial balance for lifecycle operations
    let initial_balance = 1_000_000;
    let tokens =
        setup_token_mints(&mut env.rpc, &env.payer, &creator.pubkey(), initial_balance).await;

    // Create AMM config
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

    // Check initial LP token balance
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

    let deposit_lp_amount = 500;
    let max_token_0 = 10_000;
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
    let token_0_balance_before = get_token_balance(&mut env.rpc, tokens.creator_token_0).await;
    let token_1_balance_before = get_token_balance(&mut env.rpc, tokens.creator_token_1).await;

    let swap_amount_in = 100;
    let min_amount_out = 1;

    let swap_instruction = build_swap_instruction(
        program_id,
        creator.pubkey(),
        amm_config,
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        true,
        swap_amount_in,
        min_amount_out,
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
    // Withdraw
    // ========================================================================
    let lp_balance_before_withdraw = get_token_balance(&mut env.rpc, pdas.creator_lp_token).await;
    let withdraw_lp_amount = lp_balance_before_withdraw / 2;

    let withdraw_instruction = build_withdraw_instruction(
        program_id,
        creator.pubkey(),
        &pdas,
        &tokens,
        tokens.creator_token_0,
        tokens.creator_token_1,
        withdraw_lp_amount,
        0,
        0,
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

/// Test AMM config creation
#[tokio::test]
async fn test_amm_config_local() {
    let program_id = raydium_cp_swap::ID;

    let mut env = setup_test_environment(program_id).await;

    let admin = get_admin_keypair();
    env.rpc
        .airdrop_lamports(&admin.pubkey(), 10_000_000_000)
        .await
        .unwrap();

    let amm_config = create_amm_config(&mut env.rpc, &env.payer, &admin, program_id, 0).await;
    assert_amm_config_created(&mut env.rpc, amm_config).await;

    println!("AMM config created at: {}", amm_config);
}
