use anchor_lang::prelude::*;
use light_sdk::compressible::{CompressionInfo, HasCompressionInfo};
use light_sdk_macros::RentFreeAccount;

#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

pub const OBSERVATION_SEED: &str = "observation";
pub const OBSERVATION_NUM: usize = 20;
pub const OBSERVATION_UPDATE_DURATION_DEFAULT: u64 = 15;

#[derive(Default, Clone, Copy, AnchorSerialize, AnchorDeserialize, InitSpace, Debug)]
pub struct Observation {
    pub block_timestamp: u64,
    pub cumulative_token_0_price_x32: u128,
    pub cumulative_token_1_price_x32: u128,
}

#[derive(Default, Debug, InitSpace, RentFreeAccount)]
#[compress_as(observations = None)]
#[account]
pub struct ObservationState {
    pub compression_info: Option<CompressionInfo>,
    pub initialized: bool,
    pub observation_index: u16,
    pub pool_id: Pubkey,
    pub observations: Option<[Observation; OBSERVATION_NUM]>,
    pub padding: [u64; 4],
}

impl ObservationState {
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
            self.initialized = true;
            observations[observation_index as usize].block_timestamp = block_timestamp;
            observations[observation_index as usize].cumulative_token_0_price_x32 = 0;
            observations[observation_index as usize].cumulative_token_1_price_x32 = 0;
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
            observations[next_observation_index as usize].cumulative_token_0_price_x32 =
                last_observation
                    .cumulative_token_0_price_x32
                    .wrapping_add(delta_token_0_price_x32);
            observations[next_observation_index as usize].cumulative_token_1_price_x32 =
                last_observation
                    .cumulative_token_1_price_x32
                    .wrapping_add(delta_token_1_price_x32);
            self.observation_index = next_observation_index;
            self.compression_info_mut()
                .bump_last_written_slot()
                .unwrap();
        }
    }
}

pub fn block_timestamp() -> u64 {
    Clock::get().unwrap().unix_timestamp as u64
}

#[cfg(test)]
pub fn block_timestamp_mock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
