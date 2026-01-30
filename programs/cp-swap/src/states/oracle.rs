use anchor_lang::prelude::*;
use light_token::anchor::{
    CompressionInfo, LightAccount, LightDiscriminatorTrait as LightDiscriminator,
};

#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

pub const OBSERVATION_SEED: &str = "observation";
pub const OBSERVATION_NUM: usize = 2;
pub const OBSERVATION_UPDATE_DURATION_DEFAULT: u64 = 15;

#[derive(Default, Clone, Copy, AnchorSerialize, AnchorDeserialize, InitSpace, Debug)]
pub struct Observation {
    pub block_timestamp: u64,
    pub cumulative_token_0_price_x32: u128,
    pub cumulative_token_1_price_x32: u128,
}

#[derive(Default, Debug, InitSpace, LightAccount)]
#[account]
pub struct ObservationState {
    pub compression_info: CompressionInfo,
    pub initialized: bool,
    pub observation_index: u16,
    pub pool_id: Pubkey,
    pub observations: [Observation; OBSERVATION_NUM],
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
            self.initialized = true;
            self.observations[observation_index as usize].block_timestamp = block_timestamp;
            self.observations[observation_index as usize].cumulative_token_0_price_x32 = 0;
            self.observations[observation_index as usize].cumulative_token_1_price_x32 = 0;
        } else {
            let last_observation = self.observations[observation_index as usize];
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
            self.observations[next_observation_index as usize].block_timestamp = block_timestamp;
            self.observations[next_observation_index as usize].cumulative_token_0_price_x32 =
                last_observation
                    .cumulative_token_0_price_x32
                    .wrapping_add(delta_token_0_price_x32);
            self.observations[next_observation_index as usize].cumulative_token_1_price_x32 =
                last_observation
                    .cumulative_token_1_price_x32
                    .wrapping_add(delta_token_1_price_x32);
            self.observation_index = next_observation_index;
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
