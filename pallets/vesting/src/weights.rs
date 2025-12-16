#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};

/// Weight functions for `pallet_vesting`.
pub trait WeightInfo {
    fn create_vesting_schedule() -> Weight;
    fn claim() -> Weight;
    fn cancel_vesting_schedule() -> Weight;
}

/// Weights for `pallet_mining_rewards` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    fn create_vesting_schedule() -> Weight {
        Weight::from_parts(10_000, 0)
            .saturating_add(T::DbWeight::get().reads(2))
            .saturating_add(T::DbWeight::get().writes(2))
    }

    fn claim() -> Weight {
        Weight::from_parts(15_000, 0)
            .saturating_add(T::DbWeight::get().reads(1))
            .saturating_add(T::DbWeight::get().writes(1))
    }
    
    fn cancel_vesting_schedule() -> Weight {
        Weight::from_parts(15_000, 0)
            .saturating_add(T::DbWeight::get().reads(1))
            .saturating_add(T::DbWeight::get().writes(1))
    }
}

// For tests
impl WeightInfo for () {
    fn create_vesting_schedule() -> Weight {
        Weight::from_parts(10_000, 0)
    }
    fn claim() -> Weight {
        Weight::from_parts(10_000, 0)
    }
    fn cancel_vesting_schedule() -> Weight {
        Weight::from_parts(10_000, 0)
    }
}