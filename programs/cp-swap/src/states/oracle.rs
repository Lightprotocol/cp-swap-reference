/// Oracle provides price data useful for a wide variety of system designs
///
use anchor_lang::prelude::*;

use light_sdk::{
    compressible::{CompressionInfo, HasCompressionInfo},
    sha::LightHasher,
    LightDiscriminator,
};
use light_sdk_macros::Compressible;

#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
/// Seed to derive account address and signature
pub const OBSERVATION_SEED: &str = "observation";
// Number of ObservationState element
pub const OBSERVATION_NUM: usize = 20;
pub const OBSERVATION_UPDATE_DURATION_DEFAULT: u64 = 15;

/// The element of observations in ObservationState
#[derive(Default, Clone, Copy, AnchorSerialize, AnchorDeserialize, InitSpace, Debug)]
pub struct Observation {
    /// The block timestamp of the observation
    pub block_timestamp: u64,
    /// the cumulative of token0 price during the duration time, Q32.32, the remaining 64 bit for overflow
    pub cumulative_token_0_price_x32: u128,
    /// the cumulative of token1 price during the duration time, Q32.32, the remaining 64 bit for overflow
    pub cumulative_token_1_price_x32: u128,
}
/// Tip: The 'Compressible' macro derives compress/decompress methods for the
/// account. CompressionInfo tracks the last_written_slot. Whenever a
/// compressible account is written to, last_written_slot must be updated. If
/// last_written_slot >= threshold (compression_delay), the account becomes
/// eligible for compression. Eligible accounts can be compressed
/// asynchronously.
#[account]
#[derive(LightHasher, LightDiscriminator, Compressible, InitSpace, Debug)]
#[compress_as(observations = None)]
pub struct ObservationState {
    /// Whether the ObservationState is initialized
    pub initialized: bool,
    /// the most-recently updated index of the observations array
    pub observation_index: u16,
    pub pool_id: Pubkey,
    /// observation array
    pub observations: Option<[Observation; OBSERVATION_NUM]>,
    /// #[skip] is required. Is Some when the account is decompressed and None
    /// when compressed.
    #[skip]
    pub compression_info: Option<CompressionInfo>,
    /// padding for feature update
    pub padding: [u64; 4],
}

impl Default for ObservationState {
    #[inline]
    fn default() -> ObservationState {
        ObservationState {
            initialized: false,
            observation_index: 0,
            pool_id: Pubkey::default(),
            observations: None,
            compression_info: None,
            padding: [0u64; 4],
        }
    }
}

impl ObservationState {
    // Writes an oracle observation to the account, returning the next observation_index.
    /// Writable at most once per second. Index represents the most recently written element.
    /// If the index is at the end of the allowable array length (100 - 1), the next index will turn to 0.
    ///
    /// # Arguments
    ///
    /// * `self` - The ObservationState account to write in
    /// * `block_timestamp` - The current timestamp of to update
    /// * `token_0_price_x32` - The token_0_price_x32 at the time of the new observation
    /// * `token_1_price_x32` - The token_1_price_x32 at the time of the new observation
    /// * `observation_index` - The last update index of element in the oracle array
    ///
    /// # Return
    /// * `next_observation_index` - The new index of element to update in the oracle array
    ///
    pub fn update(
        &mut self,
        block_timestamp: u64,
        token_0_price_x32: u128,
        token_1_price_x32: u128,
    ) {
        let observation_index = self.observation_index;

        if !self.initialized {
            let observations = self
                .observations
                .get_or_insert_with(|| [Observation::default(); OBSERVATION_NUM]);
            // skip the pool init price
            self.initialized = true;
            observations[observation_index as usize].block_timestamp = block_timestamp;
            observations[observation_index as usize].cumulative_token_0_price_x32 = 0;
            observations[observation_index as usize].cumulative_token_1_price_x32 = 0;

            // The account is being initialized, so we must set compression_info.
            self.compression_info = Some(CompressionInfo::new_decompressed().unwrap());
        } else {
            let observations = &mut self.observations.as_mut().unwrap();

            let last_observation = observations[observation_index as usize];
            let delta_time = block_timestamp.saturating_sub(last_observation.block_timestamp);
            if delta_time < OBSERVATION_UPDATE_DURATION_DEFAULT {
                return;
            }
            let delta_token_0_price_x32 = token_0_price_x32.checked_mul(delta_time.into()).unwrap();
            let delta_token_1_price_x32 = token_1_price_x32.checked_mul(delta_time.into()).unwrap();
            let next_observation_index = if observation_index as usize == OBSERVATION_NUM - 1 {
                0
            } else {
                observation_index + 1
            };
            observations[next_observation_index as usize].block_timestamp = block_timestamp;
            // cumulative_token_price_x32 only occupies the first 64 bits, and the remaining 64 bits are used to store overflow data
            observations[next_observation_index as usize].cumulative_token_0_price_x32 =
                last_observation
                    .cumulative_token_0_price_x32
                    .wrapping_add(delta_token_0_price_x32);
            observations[next_observation_index as usize].cumulative_token_1_price_x32 =
                last_observation
                    .cumulative_token_1_price_x32
                    .wrapping_add(delta_token_1_price_x32);
            self.observation_index = next_observation_index;

            // The account was written to, so we must update CompressionInfo.
            self.compression_info_mut()
                .bump_last_written_slot()
                .unwrap();
        }
    }
}

/// Returns the block timestamp truncated to 32 bits, i.e. mod 2**32
///
pub fn block_timestamp() -> u64 {
    Clock::get().unwrap().unix_timestamp as u64 // truncation is desired
}

#[cfg(test)]
pub fn block_timestamp_mock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
