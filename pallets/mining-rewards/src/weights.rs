// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.


//! Autogenerated weights for `pallet_mining_rewards`
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 47.2.0
//! DATE: 2025-06-24, STEPS: `50`, REPEAT: `20`, LOW RANGE: `[]`, HIGH RANGE: `[]`
//! WORST CASE MAP SIZE: `1000000`
//! HOSTNAME: `MacBook-Pro-4.local`, CPU: `<UNKNOWN>`
//! WASM-EXECUTION: `Compiled`, CHAIN: `None`, DB CACHE: `1024`

// Executed Command:
// frame-omni-bencher
// v1
// benchmark
// pallet
// --runtime
// ./target/release/wbuild/quantus-runtime/quantus_runtime.wasm
// --pallet
// pallet-mining-rewards
// --extrinsic
// *
// --template
// ./.maintain/frame-weight-template.hbs
// --output
// ./pallets/mining-rewards/src/weights.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_mining_rewards`.
pub trait WeightInfo {
	fn on_finalize_rewarded_miner() -> Weight;
}

/// Weights for `pallet_mining_rewards` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `MiningRewards::CollectedFees` (r:1 w:1)
	/// Proof: `MiningRewards::CollectedFees` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn on_finalize_rewarded_miner() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `362`
		//  Estimated: `6196`
		// Minimum execution time: 50_000_000 picoseconds.
		Weight::from_parts(53_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	/// Storage: `MiningRewards::CollectedFees` (r:1 w:1)
	/// Proof: `MiningRewards::CollectedFees` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn on_finalize_rewarded_miner() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `362`
		//  Estimated: `6196`
		// Minimum execution time: 50_000_000 picoseconds.
		Weight::from_parts(53_000_000, 6196)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
}
