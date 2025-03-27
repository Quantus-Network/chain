#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use sp_std::marker::PhantomData;

/// Weight functions for `pallet_mining_rewards`.
pub trait WeightInfo {
    fn on_initialize() -> Weight;
    fn process_miner_data() -> Weight;
}

/// Weights for `pallet_mining_rewards` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    fn on_initialize() -> Weight {
        Weight::from_parts(10_000, 0)
            .saturating_add(T::DbWeight::get().reads(2))
            .saturating_add(T::DbWeight::get().writes(2))
    }

    fn process_miner_data() -> Weight {
        Weight::from_parts(15_000, 0)
            .saturating_add(T::DbWeight::get().reads(1))
            .saturating_add(T::DbWeight::get().writes(1))
    }
}

// For tests
impl WeightInfo for () {
    fn on_initialize() -> Weight {
        Weight::from_parts(10_000, 0)
    }

    fn process_miner_data() -> Weight {
        Weight::from_parts(15_000, 0)
    }
}