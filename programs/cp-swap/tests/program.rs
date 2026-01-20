#![allow(dead_code)]

/// CpSwap SDK implementing LightProgramInterface trait.
///
/// Provides:
/// - Parsing pool accounts from AccountInterface
/// - Tracking account state (hot/cold)
/// - Building AccountSpec for load instructions

use anchor_lang::AnchorDeserialize;
use light_client::interface::{
    AccountInterface, AccountSpec, AccountToFetch, ColdContext, LightProgramInterface, PdaSpec,
    TokenAccountInterface,
};
use light_sdk::LightDiscriminator;
use light_token::compat::{CTokenData, TokenData};
use raydium_cp_swap::instructions::initialize::LP_MINT_SIGNER_SEED;
use raydium_cp_swap::{
    raydium_cp_swap::{LightAccountVariant, TokenAccountVariant},
    states::{ObservationState, PoolState},
    AUTH_SEED,
};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

pub const PROGRAM_ID: Pubkey = raydium_cp_swap::ID;

/// Instructions supported by the cp-swap program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpSwapInstruction {
    Swap,
    Deposit,
    Withdraw,
}

/// Error type for SDK operations.
#[derive(Debug, Clone)]
pub enum CpSwapSdkError {
    ParseError(String),
    UnknownDiscriminator([u8; 8]),
    MissingField(&'static str),
    PoolStateNotParsed,
    AccountNotFound(Pubkey),
}

impl std::fmt::Display for CpSwapSdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::UnknownDiscriminator(disc) => write!(f, "Unknown discriminator: {:?}", disc),
            Self::MissingField(field) => write!(f, "Missing field: {}", field),
            Self::PoolStateNotParsed => write!(f, "Pool state must be parsed first"),
            Self::AccountNotFound(key) => write!(f, "Account not found: {}", key),
        }
    }
}

impl std::error::Error for CpSwapSdkError {}

/// SDK for managing cp-swap pool accounts and building decompression instructions.
#[derive(Debug, Clone)]
pub struct CpSwapSdk {
    /// Pool state pubkey
    pub pool_state_pubkey: Option<Pubkey>,
    /// AMM config pubkey
    pub amm_config: Option<Pubkey>,
    /// Token 0 mint pubkey
    pub token_0_mint: Option<Pubkey>,
    /// Token 1 mint pubkey
    pub token_1_mint: Option<Pubkey>,
    /// Token 0 vault pubkey
    pub token_0_vault: Option<Pubkey>,
    /// Token 1 vault pubkey
    pub token_1_vault: Option<Pubkey>,
    /// LP mint pubkey
    pub lp_mint: Option<Pubkey>,
    /// LP mint signer pubkey
    pub lp_mint_signer: Option<Pubkey>,
    /// Observation state pubkey
    pub observation_key: Option<Pubkey>,
    /// Authority pubkey
    pub authority: Option<Pubkey>,
    /// Cached PDA specs keyed by pubkey (includes pool_state, observation, and vaults)
    pda_specs: HashMap<Pubkey, PdaSpec<LightAccountVariant>>,
    /// Cached mint interfaces keyed by pubkey
    mint_specs: HashMap<Pubkey, AccountInterface>,
}

impl Default for CpSwapSdk {
    fn default() -> Self {
        Self::new()
    }
}

impl CpSwapSdk {
    /// Create a new empty SDK instance.
    pub fn new() -> Self {
        Self {
            pool_state_pubkey: None,
            amm_config: None,
            token_0_mint: None,
            token_1_mint: None,
            token_0_vault: None,
            token_1_vault: None,
            lp_mint: None,
            lp_mint_signer: None,
            observation_key: None,
            authority: None,
            pda_specs: HashMap::new(),
            mint_specs: HashMap::new(),
        }
    }

    /// Parse pool state from AccountInterface and populate SDK fields.
    fn parse_pool_state(&mut self, interface: AccountInterface) -> Result<(), CpSwapSdkError> {
        let data = interface.data();
        if data.len() < 8 {
            return Err(CpSwapSdkError::ParseError(
                "Account data too short".to_string(),
            ));
        }

        // Skip 8-byte discriminator
        let pool_state = PoolState::deserialize(&mut &data[8..])
            .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;

        let pool_pubkey = interface.key;
        self.pool_state_pubkey = Some(pool_pubkey);
        self.amm_config = Some(pool_state.amm_config);
        self.token_0_mint = Some(pool_state.token_0_mint);
        self.token_1_mint = Some(pool_state.token_1_mint);
        self.token_0_vault = Some(pool_state.token_0_vault);
        self.token_1_vault = Some(pool_state.token_1_vault);
        self.lp_mint = Some(pool_state.lp_mint);
        self.observation_key = Some(pool_state.observation_key);

        // Derive lp_mint_signer and authority PDAs
        let (lp_mint_signer, _) =
            Pubkey::find_program_address(&[LP_MINT_SIGNER_SEED, pool_pubkey.as_ref()], &PROGRAM_ID);
        self.lp_mint_signer = Some(lp_mint_signer);

        let (authority, _) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &PROGRAM_ID);
        self.authority = Some(authority);

        // Create PdaSpec with variant
        let variant = LightAccountVariant::PoolState {
            data: pool_state.clone(),
            amm_config: pool_state.amm_config,
            token_0_mint: pool_state.token_0_mint,
            token_1_mint: pool_state.token_1_mint,
        };
        let spec = PdaSpec::new(interface, variant, PROGRAM_ID);
        self.pda_specs.insert(pool_pubkey, spec);

        Ok(())
    }

    /// Parse observation state from AccountInterface.
    fn parse_observation_state(&mut self, interface: AccountInterface) -> Result<(), CpSwapSdkError> {
        let pool_state = self
            .pool_state_pubkey
            .ok_or(CpSwapSdkError::PoolStateNotParsed)?;

        let data = interface.data();
        if data.len() < 8 {
            return Err(CpSwapSdkError::ParseError(
                "Account data too short".to_string(),
            ));
        }

        let obs_pubkey = interface.key;
        let obs_state = ObservationState::deserialize(&mut &data[8..])
            .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;

        let variant = LightAccountVariant::ObservationState {
            data: obs_state,
            pool_state,
        };
        let spec = PdaSpec::new(interface, variant, PROGRAM_ID);
        self.pda_specs.insert(obs_pubkey, spec);

        Ok(())
    }

    /// Store token vault interface.
    /// Vaults are program-owned PDAs, so we convert them to PdaSpec with CTokenData variant.
    pub fn set_token_vault(&mut self, interface: TokenAccountInterface, is_vault_0: bool) {
        let key = interface.key;
        let pool_state = self.pool_state_pubkey.expect("pool_state must be set before vaults");
        let mint = if is_vault_0 {
            self.token_0_mint.expect("token_0_mint must be set")
        } else {
            self.token_1_mint.expect("token_1_mint must be set")
        };

        // Build TokenData from TokenAccountInterface
        let token_data = TokenData {
            mint: interface.mint(),
            owner: interface.owner(),
            amount: interface.amount(),
            delegate: if interface.parsed.delegate.option == [1, 0, 0, 0] {
                Some(Pubkey::from(interface.parsed.delegate.value))
            } else {
                None
            },
            state: light_token::compat::AccountState::Initialized,
            tlv: None,
        };

        // Build variant based on which vault this is
        let variant = if is_vault_0 {
            LightAccountVariant::CTokenData(CTokenData {
                variant: TokenAccountVariant::Token0Vault {
                    pool_state,
                    token_0_mint: mint,
                },
                token_data,
            })
        } else {
            LightAccountVariant::CTokenData(CTokenData {
                variant: TokenAccountVariant::Token1Vault {
                    pool_state,
                    token_1_mint: mint,
                },
                token_data,
            })
        };

        // Convert TokenAccountInterface to AccountInterface for PdaSpec
        // For cold vaults, we need to convert ColdContext::Token to ColdContext::Account
        let cold = if let Some(ColdContext::Token(ct)) = &interface.cold {
            Some(ColdContext::Account(ct.account.clone()))
        } else {
            None
        };

        let account_interface = AccountInterface {
            key,
            account: interface.account.clone(),
            cold,
        };

        let spec = PdaSpec::new(account_interface, variant, PROGRAM_ID);

        self.pda_specs.insert(key, spec);
        if is_vault_0 {
            self.token_0_vault = Some(key);
        } else {
            self.token_1_vault = Some(key);
        }
    }

    /// Store LP mint interface.
    pub fn set_lp_mint(&mut self, interface: AccountInterface) {
        let key = interface.key;
        self.lp_mint = Some(key);
        self.mint_specs.insert(key, interface);
    }

    /// Parse token vault from AccountInterface and store as PdaSpec.
    fn parse_token_vault(
        &mut self,
        account: &AccountInterface,
        is_vault_0: bool,
    ) -> Result<(), CpSwapSdkError> {
        let pool_state = self
            .pool_state_pubkey
            .ok_or(CpSwapSdkError::PoolStateNotParsed)?;

        // Deserialize token data properly
        let token_data = TokenData::deserialize(&mut &account.data()[..])
            .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;

        // Build variant based on which vault this is
        let variant = if is_vault_0 {
            let token_0_mint = self
                .token_0_mint
                .ok_or(CpSwapSdkError::MissingField("token_0_mint"))?;
            LightAccountVariant::CTokenData(CTokenData {
                variant: TokenAccountVariant::Token0Vault {
                    pool_state,
                    token_0_mint,
                },
                token_data,
            })
        } else {
            let token_1_mint = self
                .token_1_mint
                .ok_or(CpSwapSdkError::MissingField("token_1_mint"))?;
            LightAccountVariant::CTokenData(CTokenData {
                variant: TokenAccountVariant::Token1Vault {
                    pool_state,
                    token_1_mint,
                },
                token_data,
            })
        };

        // For token vaults, convert ColdContext::Token to ColdContext::Account
        // because they're decompressed as PDAs, not as token accounts
        let interface = if account.is_cold() {
            let compressed_account = match &account.cold {
                Some(ColdContext::Token(ct)) => ct.account.clone(),
                Some(ColdContext::Account(ca)) => ca.clone(),
                None => return Err(CpSwapSdkError::MissingField("cold_context")),
            };
            AccountInterface {
                key: account.key,
                account: account.account.clone(),
                cold: Some(ColdContext::Account(compressed_account)),
            }
        } else {
            account.clone()
        };

        let spec = PdaSpec::new(interface, variant, PROGRAM_ID);
        self.pda_specs.insert(account.key, spec);

        Ok(())
    }

    /// Parse LP mint from AccountInterface.
    fn parse_mint(&mut self, account: &AccountInterface) -> Result<(), CpSwapSdkError> {
        self.mint_specs.insert(account.key, account.clone());
        Ok(())
    }

    /// Parse any account and route to appropriate parser.
    fn parse_account(&mut self, account: &AccountInterface) -> Result<(), CpSwapSdkError> {
        // Check if this is a known vault by pubkey
        if Some(account.key) == self.token_0_vault {
            return self.parse_token_vault(account, true);
        }
        if Some(account.key) == self.token_1_vault {
            return self.parse_token_vault(account, false);
        }

        // Check discriminator for pool/observation state
        let data = account.data();
        if data.len() >= 8 {
            let discriminator: [u8; 8] = data[..8].try_into().unwrap_or_default();

            if discriminator == PoolState::LIGHT_DISCRIMINATOR {
                return self.parse_pool_state(account.clone());
            }
            if discriminator == ObservationState::LIGHT_DISCRIMINATOR {
                return self.parse_observation_state(account.clone());
            }
        }

        // Check if this is an LP mint by matching the signer
        if let Some(lp_mint_signer) = self.lp_mint_signer {
            if let Some(mint_signer) = account.mint_signer() {
                if Pubkey::new_from_array(mint_signer) == lp_mint_signer {
                    return self.parse_mint(account);
                }
            }
        }

        // Check if this is a vault mint (token_0_mint or token_1_mint)
        if Some(account.key) == self.token_0_mint || Some(account.key) == self.token_1_mint {
            return self.parse_mint(account);
        }

        Ok(())
    }

    /// Check if pool state is cold.
    pub fn is_pool_state_cold(&self) -> bool {
        self.pool_state_pubkey
            .and_then(|k| self.pda_specs.get(&k))
            .map_or(false, |s| s.is_cold())
    }

    /// Check if observation state is cold.
    pub fn is_observation_cold(&self) -> bool {
        self.observation_key
            .and_then(|k| self.pda_specs.get(&k))
            .map_or(false, |s| s.is_cold())
    }

    /// Check if token 0 vault is cold.
    pub fn is_vault_0_cold(&self) -> bool {
        self.token_0_vault
            .and_then(|k| self.pda_specs.get(&k))
            .map_or(false, |s| s.is_cold())
    }

    /// Check if token 1 vault is cold.
    pub fn is_vault_1_cold(&self) -> bool {
        self.token_1_vault
            .and_then(|k| self.pda_specs.get(&k))
            .map_or(false, |s| s.is_cold())
    }

    /// Check if LP mint is cold.
    pub fn is_lp_mint_cold(&self) -> bool {
        self.lp_mint
            .and_then(|k| self.mint_specs.get(&k))
            .map_or(false, |s| s.is_cold())
    }

    /// Get pool state pubkey.
    pub fn pool_state(&self) -> Option<Pubkey> {
        self.pool_state_pubkey
    }
}

impl LightProgramInterface for CpSwapSdk {
    type Variant = LightAccountVariant;
    type Instruction = CpSwapInstruction;
    type Error = CpSwapSdkError;

    fn program_id(&self) -> Pubkey {
        PROGRAM_ID
    }

    fn from_keyed_accounts(accounts: &[AccountInterface]) -> Result<Self, Self::Error> {
        let mut sdk = Self::new();

        // First pass: find and parse pool state
        for account in accounts {
            let data = account.data();
            if data.len() >= 8 {
                let discriminator: [u8; 8] = data[..8].try_into().unwrap_or_default();
                if discriminator == PoolState::LIGHT_DISCRIMINATOR {
                    sdk.parse_pool_state(account.clone())?;
                    break;
                }
            }
        }

        if sdk.pool_state_pubkey.is_none() {
            return Err(CpSwapSdkError::MissingField("pool_state"));
        }

        // Second pass: parse other accounts
        for account in accounts {
            let data = account.data();
            if data.len() >= 8 {
                let discriminator: [u8; 8] = data[..8].try_into().unwrap_or_default();
                if discriminator == ObservationState::LIGHT_DISCRIMINATOR {
                    sdk.parse_observation_state(account.clone())?;
                }
            }
        }

        Ok(sdk)
    }

    fn get_accounts_to_update(&self, ix: &Self::Instruction) -> Vec<AccountToFetch> {
        let mut accounts = Vec::new();

        // All instructions need pool_state and observation_state
        if let Some(pubkey) = self.pool_state_pubkey {
            accounts.push(AccountToFetch::pda(pubkey, PROGRAM_ID));
        }
        if let Some(pubkey) = self.observation_key {
            accounts.push(AccountToFetch::pda(pubkey, PROGRAM_ID));
        }

        // All instructions need token vaults
        if let Some(pubkey) = self.token_0_vault {
            accounts.push(AccountToFetch::token(pubkey));
        }
        if let Some(pubkey) = self.token_1_vault {
            accounts.push(AccountToFetch::token(pubkey));
        }

        // All instructions need vault mints (token_0_mint and token_1_mint)
        if let Some(pubkey) = self.token_0_mint {
            accounts.push(AccountToFetch::mint(pubkey));
        }
        if let Some(pubkey) = self.token_1_mint {
            accounts.push(AccountToFetch::mint(pubkey));
        }

        // Deposit and Withdraw also need LP mint
        match ix {
            CpSwapInstruction::Deposit | CpSwapInstruction::Withdraw => {
                if let Some(pubkey) = self.lp_mint {
                    accounts.push(AccountToFetch::mint(pubkey));
                }
            }
            CpSwapInstruction::Swap => {}
        }

        accounts
    }

    fn update(&mut self, accounts: &[AccountInterface]) -> Result<(), Self::Error> {
        for account in accounts {
            self.parse_account(account)?;
        }
        Ok(())
    }

    fn get_all_specs(&self) -> Vec<AccountSpec<Self::Variant>> {
        let mut specs = Vec::new();

        // Add PDA specs (includes pool_state, observation, and vaults)
        for spec in self.pda_specs.values() {
            specs.push(AccountSpec::Pda(spec.clone()));
        }

        // Add mint specs
        for spec in self.mint_specs.values() {
            specs.push(AccountSpec::Mint(spec.clone()));
        }

        specs
    }

    fn get_specs_for_instruction(&self, ix: &Self::Instruction) -> Vec<AccountSpec<Self::Variant>> {
        let mut specs = Vec::new();

        // Pool state and observation state needed for all instructions
        if let Some(pubkey) = self.pool_state_pubkey {
            if let Some(spec) = self.pda_specs.get(&pubkey) {
                specs.push(AccountSpec::Pda(spec.clone()));
            }
        }
        if let Some(pubkey) = self.observation_key {
            if let Some(spec) = self.pda_specs.get(&pubkey) {
                specs.push(AccountSpec::Pda(spec.clone()));
            }
        }

        // Token vaults needed for all instructions (stored as PDA specs with CTokenData variant)
        if let Some(pubkey) = self.token_0_vault {
            if let Some(spec) = self.pda_specs.get(&pubkey) {
                specs.push(AccountSpec::Pda(spec.clone()));
            }
        }
        if let Some(pubkey) = self.token_1_vault {
            if let Some(spec) = self.pda_specs.get(&pubkey) {
                specs.push(AccountSpec::Pda(spec.clone()));
            }
        }

        // Vault mints (token_0_mint and token_1_mint) needed for all instructions
        if let Some(pubkey) = self.token_0_mint {
            if let Some(spec) = self.mint_specs.get(&pubkey) {
                specs.push(AccountSpec::Mint(spec.clone()));
            }
        }
        if let Some(pubkey) = self.token_1_mint {
            if let Some(spec) = self.mint_specs.get(&pubkey) {
                specs.push(AccountSpec::Mint(spec.clone()));
            }
        }

        // LP mint needed for deposit/withdraw
        match ix {
            CpSwapInstruction::Deposit | CpSwapInstruction::Withdraw => {
                if let Some(pubkey) = self.lp_mint {
                    if let Some(spec) = self.mint_specs.get(&pubkey) {
                        specs.push(AccountSpec::Mint(spec.clone()));
                    }
                }
            }
            CpSwapInstruction::Swap => {}
        }

        specs
    }
}
