#![allow(dead_code)]

/// Functional integration test for cp-swap program.
/// Tests pool initialization with light-program-test framework.
use anchor_lang::{InstructionData, ToAccountMetas};
use light_anchor_spl::memo::spl_memo;
use light_client::interface::{
    get_create_accounts_proof, CreateAccountsProofInput, CreateAccountsProofResult,
    InitializeRentFreeConfig,
};
use light_program_test::{
    program_test::{setup_mock_program_data, LightProgramTest, TestRpc},
    Indexer, ProgramTestConfig, Rpc,
};
use light_token::{
    constants::CPI_AUTHORITY_PDA,
    constants::LIGHT_TOKEN_PROGRAM_ID,
    instruction::{
        find_mint_address, get_associated_token_address_and_bump, get_spl_interface_pda_and_bump,
        CreateAssociatedTokenAccount, CreateMint, CreateMintParams, MintTo, COMPRESSIBLE_CONFIG_V1,
        RENT_SPONSOR as LIGHT_TOKEN_RENT_SPONSOR,
    },
    spl_interface::CreateSplInterfacePda,
};
use raydium_cp_swap::{
    instructions::initialize::LP_MINT_SIGNER_SEED,
    states::{AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_SEED, POOL_VAULT_SEED},
    InitializeParams, AUTH_SEED,
};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_pubkey::pubkey;
use solana_pubkey::Pubkey;
use solana_sdk::{program_pack::Pack, signature::SeedDerivable};
use solana_signer::Signer;
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
    pub token_0_mint_signer: Pubkey,
    pub token_1_mint_signer: Pubkey,
    pub creator_token_0: Pubkey,
    pub creator_token_1: Pubkey,
}

// ============================================================================
// Setup Functions
// ============================================================================

/// Initialize the test environment with LightProgramTest and compression config.
pub async fn setup_test_environment(program_id: Pubkey) -> TestEnv {
    let mut config = ProgramTestConfig::new_v2(true, Some(vec![("raydium_cp_swap", program_id)]));
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
                fee_payer: Some(payer.pubkey()),
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

/// Create the SPL interface PDA (token pool) for an SPL/Token-2022 mint.
/// This is required before SPL tokens can be transferred to/from Light Token accounts.
pub async fn create_spl_interface_pda(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    mint: &Pubkey,
) -> Pubkey {
    create_spl_interface_pda_with_program(rpc, payer, mint, spl_token::id()).await
}

/// Create the SPL interface PDA for a mint with specified token program.
pub async fn create_spl_interface_pda_with_program(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    mint: &Pubkey,
    token_program: Pubkey,
) -> Pubkey {
    let (spl_interface_pda, _bump) = get_spl_interface_pda_and_bump(mint);

    let ix = CreateSplInterfacePda::new(payer.pubkey(), *mint, token_program, false).instruction();

    rpc.create_and_send_transaction(&[ix], &payer.pubkey(), &[payer])
        .await
        .expect("Create SPL interface PDA should succeed");

    spl_interface_pda
}

/// Create SPL interface PDAs for any non-Light tokens in the setup.
pub async fn create_spl_interface_pdas_for_setup(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    setup: &FlexibleTokenSetup,
) {
    if setup.token_0_type.needs_spl_interface() {
        create_spl_interface_pda_with_program(
            rpc,
            payer,
            &setup.token_0_mint,
            setup.token_0_type.program_id(),
        )
        .await;
    }
    if setup.token_1_type.needs_spl_interface() {
        create_spl_interface_pda_with_program(
            rpc,
            payer,
            &setup.token_1_mint,
            setup.token_1_type.program_id(),
        )
        .await;
    }
}

/// Create token mints and fund creator with initial balances.
pub async fn setup_token_mints(
    rpc: &mut LightProgramTest,
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

/// Create an SPL token mint (not Light token) and fund creator with initial balance.
pub async fn setup_spl_mint(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    decimals: u8,
    recipients: Vec<(u64, Pubkey)>,
) -> (Pubkey, Keypair, Vec<Pubkey>) {
    use light_anchor_spl::associated_token::spl_associated_token_account;
    use solana_sdk::program_pack::Pack;

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();

    // Create mint account
    let mint_rent = rpc
        .get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)
        .await
        .unwrap();
    let create_mint_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &mint_pubkey,
        mint_rent,
        spl_token::state::Mint::LEN as u64,
        &spl_token::id(),
    );

    let init_mint_ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint_pubkey,
        &payer.pubkey(),
        None,
        decimals,
    )
    .unwrap();

    rpc.create_and_send_transaction(
        &[create_mint_ix, init_mint_ix],
        &payer.pubkey(),
        &[payer, &mint_keypair],
    )
    .await
    .expect("Create SPL mint should succeed");

    if recipients.is_empty() {
        return (mint_pubkey, mint_keypair, vec![]);
    }

    let mut ata_pubkeys = Vec::with_capacity(recipients.len());

    for (amount, owner) in &recipients {
        let ata = spl_associated_token_account::get_associated_token_address(owner, &mint_pubkey);
        ata_pubkeys.push(ata);

        let create_ata_ix =
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                owner,
                &mint_pubkey,
                &spl_token::id(),
            );

        rpc.create_and_send_transaction(&[create_ata_ix], &payer.pubkey(), &[payer])
            .await
            .expect("Create SPL ATA should succeed");

        if *amount > 0 {
            let mint_to_ix = spl_token::instruction::mint_to(
                &spl_token::id(),
                &mint_pubkey,
                &ata,
                &payer.pubkey(),
                &[],
                *amount,
            )
            .unwrap();

            rpc.create_and_send_transaction(&[mint_to_ix], &payer.pubkey(), &[payer])
                .await
                .expect("Mint SPL tokens should succeed");
        }
    }

    (mint_pubkey, mint_keypair, ata_pubkeys)
}

/// Token type for flexible test setup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TokenType {
    Light,
    Spl,
    Token2022,
}

impl TokenType {
    pub fn program_id(&self) -> Pubkey {
        match self {
            TokenType::Light => light_token_program_id(),
            TokenType::Spl => spl_token::id(),
            TokenType::Token2022 => spl_token_2022::id(),
        }
    }

    pub fn needs_spl_interface(&self) -> bool {
        matches!(self, TokenType::Spl | TokenType::Token2022)
    }
}

/// Flexible token setup that works with any combination of token types.
pub struct FlexibleTokenSetup {
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_mint_signer: Pubkey,
    pub token_1_mint_signer: Pubkey,
    pub creator_token_0: Pubkey,
    pub creator_token_1: Pubkey,
    pub token_0_type: TokenType,
    pub token_1_type: TokenType,
}

impl FlexibleTokenSetup {
    pub fn to_token_setup(&self) -> TokenSetup {
        TokenSetup {
            token_0_mint: self.token_0_mint,
            token_1_mint: self.token_1_mint,
            token_0_mint_signer: self.token_0_mint_signer,
            token_1_mint_signer: self.token_1_mint_signer,
            creator_token_0: self.creator_token_0,
            creator_token_1: self.creator_token_1,
        }
    }

    pub fn build_spl_interface(&self) -> SplInterfaceInfo {
        let (token_0_pda, token_0_bump) = if self.token_0_type.needs_spl_interface() {
            let (pda, bump) = get_spl_interface_pda_and_bump(&self.token_0_mint);
            (Some(pda), Some(bump))
        } else {
            (None, None)
        };

        let (token_1_pda, token_1_bump) = if self.token_1_type.needs_spl_interface() {
            let (pda, bump) = get_spl_interface_pda_and_bump(&self.token_1_mint);
            (Some(pda), Some(bump))
        } else {
            (None, None)
        };

        SplInterfaceInfo {
            token_0_pda,
            token_0_bump,
            token_1_pda,
            token_1_bump,
        }
    }
}

/// Create a Token-2022 mint (no extensions) and fund creator with initial balance.
pub async fn setup_token2022_mint(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    decimals: u8,
    recipients: Vec<(u64, Pubkey)>,
) -> (Pubkey, Keypair, Vec<Pubkey>) {
    use light_anchor_spl::associated_token::spl_associated_token_account;
    use solana_sdk::program_pack::Pack;

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();

    // Create mint account for Token-2022
    let mint_rent = rpc
        .get_minimum_balance_for_rent_exemption(spl_token_2022::state::Mint::LEN)
        .await
        .unwrap();
    let create_mint_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &mint_pubkey,
        mint_rent,
        spl_token_2022::state::Mint::LEN as u64,
        &spl_token_2022::id(),
    );

    let init_mint_ix = spl_token_2022::instruction::initialize_mint(
        &spl_token_2022::id(),
        &mint_pubkey,
        &payer.pubkey(),
        None,
        decimals,
    )
    .unwrap();

    rpc.create_and_send_transaction(
        &[create_mint_ix, init_mint_ix],
        &payer.pubkey(),
        &[payer, &mint_keypair],
    )
    .await
    .expect("Create Token-2022 mint should succeed");

    if recipients.is_empty() {
        return (mint_pubkey, mint_keypair, vec![]);
    }

    let mut ata_pubkeys = Vec::with_capacity(recipients.len());

    for (amount, owner) in &recipients {
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            owner,
            &mint_pubkey,
            &spl_token_2022::id(),
        );
        ata_pubkeys.push(ata);

        let create_ata_ix =
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                owner,
                &mint_pubkey,
                &spl_token_2022::id(),
            );

        rpc.create_and_send_transaction(&[create_ata_ix], &payer.pubkey(), &[payer])
            .await
            .expect("Create Token-2022 ATA should succeed");

        if *amount > 0 {
            let mint_to_ix = spl_token_2022::instruction::mint_to(
                &spl_token_2022::id(),
                &mint_pubkey,
                &ata,
                &payer.pubkey(),
                &[],
                *amount,
            )
            .unwrap();

            rpc.create_and_send_transaction(&[mint_to_ix], &payer.pubkey(), &[payer])
                .await
                .expect("Mint Token-2022 tokens should succeed");
        }
    }

    (mint_pubkey, mint_keypair, ata_pubkeys)
}

/// Create a single token based on type.
async fn create_single_token(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    creator: &Pubkey,
    initial_balance: u64,
    token_type: TokenType,
) -> (Pubkey, Pubkey, Pubkey) {
    // Returns (mint, mint_signer, creator_ata)
    match token_type {
        TokenType::Light => {
            let (mint, atas, mint_seed) = setup_create_mint(
                rpc,
                payer,
                payer.pubkey(),
                9,
                vec![(initial_balance, *creator)],
            )
            .await;
            (mint, mint_seed.pubkey(), atas[0])
        }
        TokenType::Spl => {
            let (mint, _keypair, atas) =
                setup_spl_mint(rpc, payer, 9, vec![(initial_balance, *creator)]).await;
            (mint, Pubkey::default(), atas[0])
        }
        TokenType::Token2022 => {
            let (mint, _keypair, atas) =
                setup_token2022_mint(rpc, payer, 9, vec![(initial_balance, *creator)]).await;
            (mint, Pubkey::default(), atas[0])
        }
    }
}

/// Create token pair with specified types.
pub async fn setup_token_pair(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    creator: &Pubkey,
    initial_balance: u64,
    type_a: TokenType,
    type_b: TokenType,
) -> FlexibleTokenSetup {
    let (mint_a, signer_a, ata_a) =
        create_single_token(rpc, payer, creator, initial_balance, type_a).await;
    let (mint_b, signer_b, ata_b) =
        create_single_token(rpc, payer, creator, initial_balance, type_b).await;

    // Ensure proper ordering: token_0_mint < token_1_mint
    if mint_a < mint_b {
        FlexibleTokenSetup {
            token_0_mint: mint_a,
            token_1_mint: mint_b,
            token_0_mint_signer: signer_a,
            token_1_mint_signer: signer_b,
            creator_token_0: ata_a,
            creator_token_1: ata_b,
            token_0_type: type_a,
            token_1_type: type_b,
        }
    } else {
        FlexibleTokenSetup {
            token_0_mint: mint_b,
            token_1_mint: mint_a,
            token_0_mint_signer: signer_b,
            token_1_mint_signer: signer_a,
            creator_token_0: ata_b,
            creator_token_1: ata_a,
            token_0_type: type_b,
            token_1_type: type_a,
        }
    }
}

/// Legacy: Token setup for mixed SPL + Light token pair.
pub struct MixedTokenSetup {
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_mint_signer: Pubkey,
    pub token_1_mint_signer: Pubkey,
    pub creator_token_0: Pubkey,
    pub creator_token_1: Pubkey,
    pub token_0_is_spl: bool,
    pub token_1_is_spl: bool,
}

/// Legacy: Create token mints where one is SPL and other is Light.
pub async fn setup_mixed_token_mints(
    rpc: &mut LightProgramTest,
    payer: &Keypair,
    creator: &Pubkey,
    initial_balance: u64,
) -> MixedTokenSetup {
    let flex = setup_token_pair(
        rpc,
        payer,
        creator,
        initial_balance,
        TokenType::Spl,
        TokenType::Light,
    )
    .await;
    MixedTokenSetup {
        token_0_mint: flex.token_0_mint,
        token_1_mint: flex.token_1_mint,
        token_0_mint_signer: flex.token_0_mint_signer,
        token_1_mint_signer: flex.token_1_mint_signer,
        creator_token_0: flex.creator_token_0,
        creator_token_1: flex.creator_token_1,
        token_0_is_spl: flex.token_0_type == TokenType::Spl,
        token_1_is_spl: flex.token_1_type == TokenType::Spl,
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
            CreateAccountsProofInput::mint_from_signer(pdas.lp_mint_signer),
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
    spl_interface: SplInterfaceInfo,
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
        spl_interface_pda_0: spl_interface.token_0_pda,
        spl_interface_pda_1: spl_interface.token_1_pda,
    };

    let instruction_data = raydium_cp_swap::instruction::Withdraw {
        lp_token_amount,
        minimum_token_0_amount,
        minimum_token_1_amount,
        spl_interface_bump_0: spl_interface.token_0_bump,
        spl_interface_bump_1: spl_interface.token_1_bump,
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
    spl_interface: SplInterfaceInfo,
    token_0_program: Pubkey,
    token_1_program: Pubkey,
) -> Instruction {
    let (input_vault, output_vault, input_mint, output_mint, input_program, output_program) =
        if is_token_0_input {
            (
                pdas.token_0_vault,
                pdas.token_1_vault,
                tokens.token_0_mint,
                tokens.token_1_mint,
                token_0_program,
                token_1_program,
            )
        } else {
            (
                pdas.token_1_vault,
                pdas.token_0_vault,
                tokens.token_1_mint,
                tokens.token_0_mint,
                token_1_program,
                token_0_program,
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
        input_token_program: input_program,
        output_token_program: output_program,
        input_token_mint: input_mint,
        output_token_mint: output_mint,
        observation_state: pdas.observation_state,
        light_token_program: light_token_program_id(),
        system_program: solana_sdk::system_program::ID,
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
        spl_interface_pda_0: spl_interface.token_0_pda,
        spl_interface_pda_1: spl_interface.token_1_pda,
    };

    let instruction_data = raydium_cp_swap::instruction::SwapBaseInput {
        amount_in,
        minimum_amount_out,
        spl_interface_bump_0: spl_interface.token_0_bump,
        spl_interface_bump_1: spl_interface.token_1_bump,
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
    spl_interface: SplInterfaceInfo,
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
        spl_interface_pda_0: spl_interface.token_0_pda,
        spl_interface_pda_1: spl_interface.token_1_pda,
    };

    let instruction_data = raydium_cp_swap::instruction::Deposit {
        lp_token_amount,
        maximum_token_0_amount,
        maximum_token_1_amount,
        spl_interface_bump_0: spl_interface.token_0_bump,
        spl_interface_bump_1: spl_interface.token_1_bump,
    };

    Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: instruction_data.data(),
    }
}

/// SPL interface info for instructions involving SPL/Token-2022 tokens.
#[derive(Default, Clone)]
pub struct SplInterfaceInfo {
    pub token_0_pda: Option<Pubkey>,
    pub token_0_bump: Option<u8>,
    pub token_1_pda: Option<Pubkey>,
    pub token_1_bump: Option<u8>,
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
    build_initialize_instruction_with_spl(
        program_id,
        creator,
        amm_config,
        pdas,
        tokens,
        config_pda,
        proof_result,
        init_amount_0,
        init_amount_1,
        open_time,
        light_token_program_id(),
        light_token_program_id(),
        SplInterfaceInfo::default(),
    )
}

/// Build the Initialize instruction with SPL interface support.
pub fn build_initialize_instruction_with_spl(
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
    token_0_program: Pubkey,
    token_1_program: Pubkey,
    spl_interface: SplInterfaceInfo,
) -> Instruction {
    let init_params = InitializeParams {
        init_amount_0,
        init_amount_1,
        open_time,
        create_accounts_proof: proof_result.create_accounts_proof.clone(),
        lp_mint_signer_bump: pdas.lp_mint_signer_bump,
        creator_lp_token_bump: pdas.creator_lp_token_bump,
        authority_bump: pdas.authority_bump,
        spl_interface_bump_0: spl_interface.token_0_bump,
        spl_interface_bump_1: spl_interface.token_1_bump,
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
        token_0_program,
        token_1_program,
        associated_token_program: light_anchor_spl::associated_token::ID,
        system_program: solana_sdk::system_program::ID,
        rent: solana_sdk::sysvar::rent::ID,
        compression_config: config_pda,
        light_token_compressible_config: Pubkey::from(COMPRESSIBLE_CONFIG_V1),
        light_token_rent_sponsor: Pubkey::from(LIGHT_TOKEN_RENT_SPONSOR),
        light_token_program: light_token_program_id(),
        light_token_cpi_authority: CPI_AUTHORITY_PDA,
        spl_interface_pda_0: spl_interface.token_0_pda,
        spl_interface_pda_1: spl_interface.token_1_pda,
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

/// Assert that an account exists on-chain.
pub async fn assert_onchain_exists(rpc: &mut LightProgramTest, pda: &Pubkey) {
    assert!(
        rpc.get_account(*pda).await.unwrap().is_some(),
        "Account {} should exist on-chain",
        pda
    );
}

/// Assert that an account is closed (doesn't exist or has 0 lamports).
pub async fn assert_onchain_closed(rpc: &mut LightProgramTest, pda: &Pubkey) {
    let acc = rpc.get_account(*pda).await.unwrap();
    assert!(
        acc.is_none() || acc.unwrap().lamports == 0,
        "Account {} should be closed",
        pda
    );
}

/// Assert all pool accounts exist on-chain (hot or decompressed state).
pub async fn assert_pool_accounts_exist(
    rpc: &mut LightProgramTest,
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
    rpc: &mut LightProgramTest,
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
    setup_create_pool_fee_account(&mut env.rpc, &env.payer.pubkey());

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
