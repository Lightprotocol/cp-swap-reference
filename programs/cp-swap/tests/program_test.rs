/// Clean integration test for cp-swap using CpSwapSdk.
/// Tests the full lifecycle: Initialize -> Warp -> Compress -> Load -> Execute Operations

use light_client::interface::{
    create_load_instructions, AccountInterfaceExt, AccountSpec, LightProgramInterface,
};
use light_program_test::program_test::TestRpc;
use light_program_test::Rpc;
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
    println!("{}: {} bytes ({} instructions)", name, serialized.len(), ixs.len());
}

#[tokio::test]
async fn test_sdk_lifecycle() {
    let program_id = raydium_cp_swap::ID;

    // ==================== PHASE 1: Setup & Initialize Pool ====================
    let mut setup = setup_pool_environment(program_id, 10).await;

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
    setup
        .env
        .rpc
        .create_and_send_transaction(&[init_ix], &setup.creator.pubkey(), &[&setup.creator])
        .await
        .unwrap();

    // ==================== PHASE 2: Verify Hot Accounts Exist ====================
    assert_pool_accounts_exist(&mut setup.env.rpc, &setup.pdas, &setup.tokens).await;

    // ==================== PHASE 3: Warp to Trigger Compression ====================
    setup
        .env
        .rpc
    .warp_epoch_forward(30)
        .await
        .unwrap();

    // ==================== PHASE 4: Assert All Accounts Are Compressed ====================
    assert_pool_accounts_compressed(&mut setup.env.rpc, &setup.pdas, &setup.tokens).await;

    // ==================== PHASE 5: Create SDK from Compressed State ====================
    let pool_interface = setup
        .env
        .rpc
        .get_account_interface(&setup.pdas.pool_state, &program_id)
        .await
        .expect("pool should be compressed");
    assert!(
        pool_interface.is_cold(),
        "pool_state should be cold after warp"
    );

    let mut sdk = CpSwapSdk::from_keyed_accounts(&[pool_interface])
        .expect("from_keyed_accounts should succeed");

    // ==================== PHASE 6: Fetch & Update SDK ====================
    let accounts_to_fetch = sdk.get_accounts_to_update(&CpSwapInstruction::Deposit);
    let keyed_accounts = setup
        .env
        .rpc
        .get_multiple_account_interfaces(&accounts_to_fetch)
        .await
        .expect("get_multiple_account_interfaces should succeed");

    sdk.update(&keyed_accounts)
        .expect("sdk.update should succeed");

    // ==================== PHASE 7: Build Specs for Load ====================
    let mut all_specs = sdk.get_specs_for_instruction(&CpSwapInstruction::Deposit);

    // Fetch creator's ATAs (compressed) and add to specs
    let creator_lp_ata_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.pdas.lp_mint)
        .await
        .expect("get_ata_interface for creator_lp_token should succeed");
    all_specs.push(AccountSpec::Ata(creator_lp_ata_interface));

    let creator_token_0_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.tokens.token_0_mint)
        .await
        .expect("get_ata_interface for creator_token_0 should succeed");
    all_specs.push(AccountSpec::Ata(creator_token_0_interface));

    let creator_token_1_interface = setup
        .env
        .rpc
        .get_ata_interface(&setup.creator.pubkey(), &setup.tokens.token_1_mint)
        .await
        .expect("get_ata_interface for creator_token_1 should succeed");
    all_specs.push(AccountSpec::Ata(creator_token_1_interface));

    // ==================== PHASE 8: Create Load Instructions ====================
    let all_load_ixs = create_load_instructions(
        &all_specs,
        setup.env.payer.pubkey(),
        setup.env.config_pda,
        setup.env.payer.pubkey(),
        &setup.env.rpc,
    )
    .await
    .expect("create_load_instructions should succeed");

    // ==================== PHASE 9: Execute Load ====================
    log_transaction_size("Load transaction", &all_load_ixs);
    setup
        .env
        .rpc
        .create_and_send_transaction(
            &all_load_ixs,
            &setup.env.payer.pubkey(),
            &[&setup.env.payer, &setup.creator],
        )
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
