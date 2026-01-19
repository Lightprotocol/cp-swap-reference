/// Functional integration test for cp-swap program.
/// Tests pool initialization with light-program-test framework.

use anchor_lang::{InstructionData, ToAccountMetas};
use light_client::interface::{
    get_create_accounts_proof, CreateAccountsProofInput, CreateAccountsProofResult,
    InitializeRentFreeConfig,
};
use solana_pubkey::pubkey;
use light_program_test::{
    program_test::{setup_mock_program_data, LightProgramTest, TestRpc},
    Indexer, ProgramTestConfig, Rpc,
};
use light_sdk_types::LIGHT_TOKEN_PROGRAM_ID;
use light_token_sdk::{
    constants::CPI_AUTHORITY_PDA,
    token::{
        find_mint_address, get_associated_token_address_and_bump, CreateAssociatedTokenAccount,
        CreateMint, CreateMintParams, MintTo, COMPRESSIBLE_CONFIG_V1,
        RENT_SPONSOR as LIGHT_TOKEN_RENT_SPONSOR,
    },
};
use raydium_cp_swap::{
    instructions::initialize::LP_MINT_SIGNER_SEED,
    states::{AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_SEED, POOL_VAULT_SEED},
    InitializeParams, AUTH_SEED,
};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_sdk::{program_pack::Pack, signature::SeedDerivable};
use anchor_spl::memo::spl_memo;
use spl_token_2022;



// ============================================================================
// Constants
// ============================================================================

const RENT_SPONSOR: Pubkey = pubkey!("CLEuMG7pzJX9xAuKCFzBP154uiG1GaNo4Fq7x6KAcAfG");

pub fn light_token_program_id() -> Pubkey {
    Pubkey::from(LIGHT_TOKEN_PROGRAM_ID)
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
    pub rpc: LightProgramTest,
    pub payer: Keypair,
    pub config_pda: Pubkey,
}

/// Token mints and creator accounts for the pool.
pub struct TokenSetup {
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub creator_token_0: Pubkey,
    pub creator_token_1: Pubkey,
}

// ============================================================================
// Setup Functions
// ============================================================================

/// Initialize the test environment with LightProgramTest and compression config.
pub async fn setup_test_environment(program_id: Pubkey) -> TestEnv {
    let mut config =
        ProgramTestConfig::new_v2(true, Some(vec![("raydium_cp_swap", program_id)]));
    config = config.with_light_protocol_events();

    let mut rpc = LightProgramTest::new(config).await.unwrap();
    let payer = rpc.get_payer().insecure_clone();

    let program_data_pda = setup_mock_program_data(&mut rpc, &payer, &program_id);

    let (init_config_ix, config_pda) = InitializeRentFreeConfig::new(
        &program_id,
        &payer.pubkey(),
        &program_data_pda,
        RENT_SPONSOR,
        payer.pubkey(),
    )
    .build();

    rpc.create_and_send_transaction(&[init_config_ix], &payer.pubkey(), &[&payer])
        .await
        .expect("Initialize config should succeed");

    TestEnv {
        rpc,
        payer,
        config_pda,
    }
}

/// Create a compressed mint with ATAs for recipients.
pub async fn setup_create_mint(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    mint_authority: Pubkey,
    decimals: u8,
    recipients: Vec<(u64, Pubkey)>,
) -> (Pubkey, Vec<Pubkey>, Keypair) {
    let mint_seed = Keypair::new();
    let address_tree = rpc.get_address_tree_v2();
    let output_queue = rpc.get_random_state_tree_info().unwrap().queue;

    let compression_address = light_token_sdk::token::derive_mint_compressed_address(
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
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    creator: &Pubkey,
    initial_balance: u64,
) -> TokenSetup {
    let (mint_a, ata_pubkeys_a, _) = setup_create_mint(
        rpc,
        payer,
        payer.pubkey(),
        9,
        vec![(initial_balance, *creator)],
    )
    .await;

    let (mint_b, ata_pubkeys_b, _) = setup_create_mint(
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
            creator_token_0: ata_pubkeys_a[0],
            creator_token_1: ata_pubkeys_b[0],
        }
    } else {
        TokenSetup {
            token_0_mint: mint_b,
            token_1_mint: mint_a,
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
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    admin: &Keypair,
    program_id: Pubkey,
    index: u16,
) -> Pubkey {
    let (amm_config_pda, _) = Pubkey::find_program_address(
        &[AMM_CONFIG_SEED.as_bytes(), &index.to_be_bytes()],
        &program_id,
    );

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
pub fn setup_create_pool_fee_account(rpc: &mut LightProgramTest, owner: &Pubkey) {
    let create_pool_fee_receiver = raydium_cp_swap::create_pool_fee_receiver::ID;
    let wsol_mint = spl_token::native_mint::id();

    let mut fee_receiver_data = vec![0u8; spl_token::state::Account::LEN];
    let fee_account = spl_token::state::Account {
        mint: wsol_mint,
        owner: *owner,
        amount: 0,
        delegate: solana_sdk::program_option::COption::None,
        state: spl_token::state::AccountState::Initialized,
        is_native: solana_sdk::program_option::COption::Some(0),
        delegated_amount: 0,
        close_authority: solana_sdk::program_option::COption::None,
    };
    spl_token::state::Account::pack(fee_account, &mut fee_receiver_data).unwrap();

    rpc.set_account(
        create_pool_fee_receiver,
        solana_sdk::account::Account {
            lamports: 1_000_000_000,
            data: fee_receiver_data,
            owner: spl_token::id(),
            executable: false,
            rent_epoch: 0,
        },
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
    rpc: &LightProgramTest,
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
        associated_token_program: anchor_spl::associated_token::ID,
        system_program: solana_sdk::system_program::ID,
        rent: solana_sdk::sysvar::rent::ID,
        compression_config: config_pda,
        light_token_compressible_config: Pubkey::from(COMPRESSIBLE_CONFIG_V1),
        light_token_rent_sponsor: Pubkey::from(LIGHT_TOKEN_RENT_SPONSOR),
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
pub async fn get_token_balance(rpc: &mut LightProgramTest, account: Pubkey) -> u64 {
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

/// Verify that the pool was initialized correctly.
pub async fn assert_pool_initialized(rpc: &mut LightProgramTest, pdas: &AmmPdas) {
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
    rpc: &mut LightProgramTest,
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
    rpc: &mut LightProgramTest,
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
    rpc: &mut LightProgramTest,
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
pub async fn assert_amm_config_created(rpc: &mut LightProgramTest, amm_config: Pubkey) {
    let account = rpc.get_account(amm_config).await.unwrap();
    assert!(account.is_some(), "AmmConfig account should exist");
}
