#![cfg_attr(not(feature = "std"), no_std)]
#![allow(unused_parens)]

//! Weights for `pallet_upgrade_gov`.
//!
//! Hand-written placeholder weights modeled on similar minimal pallets. Regenerate with the
//! benchmark CLI before relying on these for fee accuracy.

use core::marker::PhantomData;
use frame_support::{
	traits::Get,
	weights::{constants::RocksDbWeight, Weight},
};

/// Weight functions needed for `pallet_upgrade_gov`.
pub trait WeightInfo {
	fn propose() -> Weight;
	fn approve() -> Weight;
	fn cancel() -> Weight;
}

/// Weights for `pallet_upgrade_gov` using the recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn propose() -> Weight {
		Weight::from_parts(15_000_000, 4000)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	fn approve() -> Weight {
		Weight::from_parts(12_000_000, 4000)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	fn cancel() -> Weight {
		Weight::from_parts(9_000_000, 2000)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	fn propose() -> Weight {
		Weight::from_parts(15_000_000, 4000)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn approve() -> Weight {
		Weight::from_parts(12_000_000, 4000)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn cancel() -> Weight {
		Weight::from_parts(9_000_000, 2000)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
}
