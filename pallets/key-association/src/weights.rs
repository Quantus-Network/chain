//! Placeholder weights for pallet-key-association.
//!
//! These are temporary weights that should be replaced with benchmarked values.
//! Run `cargo run --release -p quantus-node benchmark pallet` to generate real weights.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_key_association`.
pub trait WeightInfo {
	fn associate() -> Weight;
}

/// Placeholder weights for `pallet_key_association`.
///
/// These are conservative estimates. Real benchmarks should be run for production.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Weight for the `associate` extrinsic.
	///
	/// Storage: `System::BlockHash` (r:1 for block hash lookup)
	/// Storage: `KeyAssociation::KeyIndex` (r:1 w:1)
	/// Storage: `KeyAssociation::Associations` (r:1 w:1)
	///
	/// Computation: Signature verification (ECDSA ~50µs, Ed25519 ~30µs)
	fn associate() -> Weight {
		// Conservative placeholder: 100ms execution + storage ops
		// ECDSA verification is more expensive than Ed25519
		Weight::from_parts(100_000_000, 0)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

/// Default implementation for tests.
impl WeightInfo for () {
	fn associate() -> Weight {
		Weight::from_parts(100_000_000, 0)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
}
