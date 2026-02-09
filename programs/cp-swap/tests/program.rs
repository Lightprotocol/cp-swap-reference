#![allow(dead_code)]

/// CpSwap SDK implementing LightProgram and Jupiter Amm traits.
///
/// Provides:
/// - Flat struct populated from pool state at construction
/// - LightProgram: instruction_accounts + load_specs for cold account handling
/// - Jupiter AMM: quotes and swap instruction building
use anchor_lang::AnchorDeserialize;
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, KeyedAccount, Quote, QuoteParams, Swap, SwapAndAccountMetas,
    SwapMode, SwapParams,
};
use light_client::interface::{
    AccountInterface, AccountSpec, ColdContext, LightProgram, PdaSpec,
};
use light_token::compat::{CTokenData, TokenData};
use raydium_cp_swap::curve::calculator::CurveCalculator;
use raydium_cp_swap::curve::fees::FEE_RATE_DENOMINATOR_VALUE;
use raydium_cp_swap::instructions::initialize::LP_MINT_SIGNER_SEED;
use raydium_cp_swap::states::config::AmmConfig;
use raydium_cp_swap::{
    raydium_cp_swap::{LightAccountVariant, TokenAccountVariant},
    states::{ObservationState, PoolState, PoolStatusBitIndex},
    AUTH_SEED,
};
use rust_decimal::Decimal;
use solana_instruction::AccountMeta;
use solana_pubkey::Pubkey;

pub const PROGRAM_ID: Pubkey = raydium_cp_swap::ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpSwapInstruction {
    Swap,
    Deposit,
    Withdraw,
}

#[derive(Debug, Clone)]
pub enum CpSwapSdkError {
    ParseError(String),
}

impl std::fmt::Display for CpSwapSdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for CpSwapSdkError {}

/// Flat SDK struct. All pubkey fields populated at construction from pool state.
/// No Options, no HashMaps. Variants built on the fly in `load_specs`.
#[derive(Debug, Clone)]
pub struct CpSwapSdk {
    pub pool_state_pubkey: Pubkey,
    pub amm_config: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub lp_mint_signer: Pubkey,
    pub observation_key: Pubkey,
    pub authority: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    // Jupiter AMM mutable state (populated via Amm::update)
    pub token_0_amount: u64,
    pub token_1_amount: u64,
    pub protocol_fees_token_0: u64,
    pub protocol_fees_token_1: u64,
    pub fund_fees_token_0: u64,
    pub fund_fees_token_1: u64,
    pub trade_fee_rate: u64,
    pub protocol_fee_rate: u64,
    pub fund_fee_rate: u64,
    pub pool_status: u8,
}

impl CpSwapSdk {
    /// Construct from pool state pubkey and its account data.
    pub fn from_pool_data(
        pool_state_pubkey: Pubkey,
        pool_data: &[u8],
    ) -> Result<Self, CpSwapSdkError> {
        let pool = PoolState::deserialize(&mut &pool_data[8..])
            .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;

        let (authority, _) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &PROGRAM_ID);
        let (lp_mint_signer, _) = Pubkey::find_program_address(
            &[LP_MINT_SIGNER_SEED, pool_state_pubkey.as_ref()],
            &PROGRAM_ID,
        );

        Ok(Self {
            pool_state_pubkey,
            amm_config: pool.amm_config,
            token_0_mint: pool.token_0_mint,
            token_1_mint: pool.token_1_mint,
            token_0_vault: pool.token_0_vault,
            token_1_vault: pool.token_1_vault,
            lp_mint: pool.lp_mint,
            lp_mint_signer,
            observation_key: pool.observation_key,
            authority,
            token_0_program: pool.token_0_program,
            token_1_program: pool.token_1_program,
            token_0_amount: 0,
            token_1_amount: 0,
            protocol_fees_token_0: pool.protocol_fees_token_0,
            protocol_fees_token_1: pool.protocol_fees_token_1,
            fund_fees_token_0: pool.fund_fees_token_0,
            fund_fees_token_1: pool.fund_fees_token_1,
            trade_fee_rate: 0,
            protocol_fee_rate: 0,
            fund_fee_rate: 0,
            pool_status: pool.status,
        })
    }

    /// Convert token vault ColdContext::Token -> ColdContext::Account.
    fn convert_vault_interface(
        account: &AccountInterface,
    ) -> Result<AccountInterface, CpSwapSdkError> {
        if account.is_cold() {
            let compressed_account = match &account.cold {
                Some(ColdContext::Token(ct)) => ct.account.clone(),
                Some(ColdContext::Account(ca)) => ca.clone(),
                _ => {
                    return Err(CpSwapSdkError::ParseError(
                        "unexpected cold context for vault".to_string(),
                    ))
                }
            };
            Ok(AccountInterface {
                key: account.key,
                account: account.account.clone(),
                cold: Some(ColdContext::Account(compressed_account)),
            })
        } else {
            Ok(account.clone())
        }
    }

    // Jupiter AMM helpers

    fn vault_amounts_without_fees(&self) -> (u64, u64) {
        let token_0 = self
            .token_0_amount
            .saturating_sub(self.protocol_fees_token_0)
            .saturating_sub(self.fund_fees_token_0);
        let token_1 = self
            .token_1_amount
            .saturating_sub(self.protocol_fees_token_1)
            .saturating_sub(self.fund_fees_token_1);
        (token_0, token_1)
    }

    fn is_swap_enabled(&self) -> bool {
        (self.pool_status & (1 << (PoolStatusBitIndex::Swap as u8))) == 0
    }

    fn calculate_quote(
        &self,
        input_mint: Pubkey,
        output_mint: Pubkey,
        amount: u64,
        swap_mode: SwapMode,
    ) -> Result<Quote, anyhow::Error> {
        let (vault_0, vault_1) = self.vault_amounts_without_fees();

        let (source_amount, dest_amount, fee_mint) =
            if input_mint == self.token_0_mint && output_mint == self.token_1_mint {
                (vault_0 as u128, vault_1 as u128, input_mint)
            } else if input_mint == self.token_1_mint && output_mint == self.token_0_mint {
                (vault_1 as u128, vault_0 as u128, input_mint)
            } else {
                return Err(anyhow::anyhow!("Invalid mint pair"));
            };

        let result = match swap_mode {
            SwapMode::ExactIn => CurveCalculator::swap_base_input(
                amount as u128,
                source_amount,
                dest_amount,
                self.trade_fee_rate,
                self.protocol_fee_rate,
                self.fund_fee_rate,
            ),
            SwapMode::ExactOut => CurveCalculator::swap_base_output(
                amount as u128,
                source_amount,
                dest_amount,
                self.trade_fee_rate,
                self.protocol_fee_rate,
                self.fund_fee_rate,
            ),
        }
        .ok_or_else(|| anyhow::anyhow!("Swap calculation failed"))?;

        let (in_amount, out_amount) = match swap_mode {
            SwapMode::ExactIn => (amount, result.destination_amount_swapped as u64),
            SwapMode::ExactOut => (result.source_amount_swapped as u64, amount),
        };

        let fee_pct =
            Decimal::from(self.trade_fee_rate) / Decimal::from(FEE_RATE_DENOMINATOR_VALUE);

        Ok(Quote {
            in_amount,
            out_amount,
            fee_amount: result.trade_fee as u64,
            fee_mint,
            fee_pct,
        })
    }
}

// ============================================================================
// LightProgram Trait Implementation
// ============================================================================

impl LightProgram for CpSwapSdk {
    type Variant = LightAccountVariant;
    type Instruction = CpSwapInstruction;

    fn program_id() -> Pubkey {
        PROGRAM_ID
    }

    fn instruction_accounts(&self, ix: &Self::Instruction) -> Vec<Pubkey> {
        match ix {
            CpSwapInstruction::Swap => vec![
                self.pool_state_pubkey,
                self.observation_key,
                self.token_0_vault,
                self.token_1_vault,
                self.token_0_mint,
                self.token_1_mint,
            ],
            CpSwapInstruction::Deposit | CpSwapInstruction::Withdraw => vec![
                self.pool_state_pubkey,
                self.observation_key,
                self.token_0_vault,
                self.token_1_vault,
                self.token_0_mint,
                self.token_1_mint,
                self.lp_mint,
            ],
        }
    }

    fn load_specs(
        &self,
        cold_accounts: &[AccountInterface],
    ) -> Result<Vec<AccountSpec<Self::Variant>>, Box<dyn std::error::Error>> {
        let mut specs = Vec::new();
        for account in cold_accounts {
            if account.key == self.pool_state_pubkey {
                let pool = PoolState::deserialize(&mut &account.data()[8..])
                    .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;
                let variant = LightAccountVariant::PoolState {
                    data: pool,
                    amm_config: self.amm_config,
                    token_0_mint: self.token_0_mint,
                    token_1_mint: self.token_1_mint,
                };
                specs.push(AccountSpec::Pda(PdaSpec::new(
                    account.clone(),
                    variant,
                    PROGRAM_ID,
                )));
            } else if account.key == self.observation_key {
                let obs = ObservationState::deserialize(&mut &account.data()[8..])
                    .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;
                let variant = LightAccountVariant::ObservationState {
                    data: obs,
                    pool_state: self.pool_state_pubkey,
                };
                specs.push(AccountSpec::Pda(PdaSpec::new(
                    account.clone(),
                    variant,
                    PROGRAM_ID,
                )));
            } else if account.key == self.token_0_vault {
                let token_data = TokenData::deserialize(&mut &account.data()[..])
                    .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;
                let variant = LightAccountVariant::CTokenData(CTokenData {
                    variant: TokenAccountVariant::Token0Vault {
                        pool_state: self.pool_state_pubkey,
                        token_0_mint: self.token_0_mint,
                    },
                    token_data,
                });
                let interface = Self::convert_vault_interface(account)?;
                specs.push(AccountSpec::Pda(PdaSpec::new(interface, variant, PROGRAM_ID)));
            } else if account.key == self.token_1_vault {
                let token_data = TokenData::deserialize(&mut &account.data()[..])
                    .map_err(|e| CpSwapSdkError::ParseError(e.to_string()))?;
                let variant = LightAccountVariant::CTokenData(CTokenData {
                    variant: TokenAccountVariant::Token1Vault {
                        pool_state: self.pool_state_pubkey,
                        token_1_mint: self.token_1_mint,
                    },
                    token_data,
                });
                let interface = Self::convert_vault_interface(account)?;
                specs.push(AccountSpec::Pda(PdaSpec::new(interface, variant, PROGRAM_ID)));
            } else if account.key == self.token_0_mint
                || account.key == self.token_1_mint
                || account.key == self.lp_mint
            {
                specs.push(AccountSpec::Mint(account.clone()));
            }
        }
        Ok(specs)
    }
}

// ============================================================================
// Jupiter AMM Trait Implementation
// ============================================================================

impl Amm for CpSwapSdk {
    fn from_keyed_account(
        keyed_account: &KeyedAccount,
        _amm_context: &AmmContext,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let data = &keyed_account.account.data;
        let pool = PoolState::deserialize(&mut &data[8..])
            .map_err(|e| anyhow::anyhow!("Failed to parse pool state: {}", e))?;

        let pool_pubkey = keyed_account.key;
        let (authority, _) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &PROGRAM_ID);
        let (lp_mint_signer, _) = Pubkey::find_program_address(
            &[LP_MINT_SIGNER_SEED, pool_pubkey.as_ref()],
            &PROGRAM_ID,
        );

        Ok(Self {
            pool_state_pubkey: pool_pubkey,
            amm_config: pool.amm_config,
            token_0_mint: pool.token_0_mint,
            token_1_mint: pool.token_1_mint,
            token_0_vault: pool.token_0_vault,
            token_1_vault: pool.token_1_vault,
            lp_mint: pool.lp_mint,
            lp_mint_signer,
            observation_key: pool.observation_key,
            authority,
            token_0_program: pool.token_0_program,
            token_1_program: pool.token_1_program,
            token_0_amount: 0,
            token_1_amount: 0,
            protocol_fees_token_0: pool.protocol_fees_token_0,
            protocol_fees_token_1: pool.protocol_fees_token_1,
            fund_fees_token_0: pool.fund_fees_token_0,
            fund_fees_token_1: pool.fund_fees_token_1,
            trade_fee_rate: 0,
            protocol_fee_rate: 0,
            fund_fee_rate: 0,
            pool_status: pool.status,
        })
    }

    fn label(&self) -> String {
        "Raydium CP Swap".to_string()
    }

    fn program_id(&self) -> Pubkey {
        PROGRAM_ID
    }

    fn key(&self) -> Pubkey {
        self.pool_state_pubkey
    }

    fn get_reserve_mints(&self) -> Vec<Pubkey> {
        vec![self.token_0_mint, self.token_1_mint]
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        vec![
            self.pool_state_pubkey,
            self.token_0_vault,
            self.token_1_vault,
            self.amm_config,
        ]
    }

    fn update(&mut self, account_map: &AccountMap) -> Result<(), anyhow::Error> {
        if let Some(account) = account_map.get(&self.pool_state_pubkey) {
            if account.data.len() >= 8 {
                let pool = PoolState::deserialize(&mut &account.data[8..])
                    .map_err(|e| anyhow::anyhow!("Failed to parse pool state: {}", e))?;
                self.protocol_fees_token_0 = pool.protocol_fees_token_0;
                self.protocol_fees_token_1 = pool.protocol_fees_token_1;
                self.fund_fees_token_0 = pool.fund_fees_token_0;
                self.fund_fees_token_1 = pool.fund_fees_token_1;
                self.pool_status = pool.status;
            }
        }

        if let Some(account) = account_map.get(&self.token_0_vault) {
            if account.data.len() >= 72 {
                self.token_0_amount =
                    u64::from_le_bytes(account.data[64..72].try_into().unwrap_or_default());
            }
        }

        if let Some(account) = account_map.get(&self.token_1_vault) {
            if account.data.len() >= 72 {
                self.token_1_amount =
                    u64::from_le_bytes(account.data[64..72].try_into().unwrap_or_default());
            }
        }

        if let Some(account) = account_map.get(&self.amm_config) {
            if account.data.len() >= 8 {
                let config = AmmConfig::deserialize(&mut &account.data[8..])
                    .map_err(|e| anyhow::anyhow!("Failed to parse amm config: {}", e))?;
                self.trade_fee_rate = config.trade_fee_rate;
                self.protocol_fee_rate = config.protocol_fee_rate;
                self.fund_fee_rate = config.fund_fee_rate;
            }
        }

        Ok(())
    }

    fn quote(&self, quote_params: &QuoteParams) -> Result<Quote, anyhow::Error> {
        if !self.is_swap_enabled() {
            return Err(anyhow::anyhow!("Swap is disabled for this pool"));
        }
        self.calculate_quote(
            quote_params.input_mint,
            quote_params.output_mint,
            quote_params.amount,
            quote_params.swap_mode,
        )
    }

    fn get_swap_and_account_metas(
        &self,
        swap_params: &SwapParams<'_, '_>,
    ) -> Result<SwapAndAccountMetas, anyhow::Error> {
        let (input_vault, output_vault, input_mint, output_mint, input_program, output_program) =
            if swap_params.source_mint == self.token_0_mint {
                (
                    self.token_0_vault,
                    self.token_1_vault,
                    self.token_0_mint,
                    self.token_1_mint,
                    self.token_0_program,
                    self.token_1_program,
                )
            } else {
                (
                    self.token_1_vault,
                    self.token_0_vault,
                    self.token_1_mint,
                    self.token_0_mint,
                    self.token_1_program,
                    self.token_0_program,
                )
            };

        let account_metas = vec![
            AccountMeta::new_readonly(swap_params.token_transfer_authority, true),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.amm_config, false),
            AccountMeta::new(self.pool_state_pubkey, false),
            AccountMeta::new(swap_params.source_token_account, false),
            AccountMeta::new(swap_params.destination_token_account, false),
            AccountMeta::new(input_vault, false),
            AccountMeta::new(output_vault, false),
            AccountMeta::new_readonly(input_program, false),
            AccountMeta::new_readonly(output_program, false),
            AccountMeta::new_readonly(input_mint, false),
            AccountMeta::new_readonly(output_mint, false),
            AccountMeta::new(self.observation_key, false),
        ];

        Ok(SwapAndAccountMetas {
            swap: Swap::RaydiumCP,
            account_metas,
        })
    }

    fn clone_amm(&self) -> Box<dyn Amm + Send + Sync> {
        Box::new(self.clone())
    }

    fn supports_exact_out(&self) -> bool {
        true
    }

    fn is_active(&self) -> bool {
        self.is_swap_enabled()
    }
}
