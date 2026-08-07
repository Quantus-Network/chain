//! Weights for `pallet_vesting`.
//!
//! Base numbers benchmarked with the Substrate benchmark CLI (STEPS 50, REPEAT 20,
//! WASM compiled) via `scripts/regenerate_weights.sh`. `claim` and `end_schedule`
//! record a wormhole transfer proof, whose ZK-tree leaf insert walks the tree
//! leaf-to-root — their weight therefore adds depth-dependent tree ops on top of the
//! benchmarked base. **Keep this augmentation when regenerating from benchmarks**
//! (benchmarks measure the compute base only, at benchmark-time tree depth).

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::{Weight, RuntimeDbWeight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `pallet_vesting`.
pub trait WeightInfo {
	fn claim() -> Weight;
	fn create_schedule() -> Weight;
	fn end_schedule() -> Weight;
	fn retarget_schedule() -> Weight;
}

/// Non-tree storage ops of `claim`: `Vesting::Schedules` (r:1 w:1), `Timestamp::Now`
/// (r:1 w:0), `System::Account` (r:2 w:2), `Wormhole::TransferCount` (r:1 w:1).
const CLAIM_BASE_READS: u64 = 5;
const CLAIM_BASE_WRITES: u64 = 4;

/// Non-tree storage ops of `end_schedule`: `TreasuryPallet::TreasuryAccount` (r:1 w:0),
/// `Vesting::Schedules` (r:1 w:1), `Timestamp::Now` (r:1 w:0), `System::Account`
/// (r:3 w:3), `Wormhole::TransferCount` (r:1 w:1).
const END_SCHEDULE_BASE_READS: u64 = 7;
const END_SCHEDULE_BASE_WRITES: u64 = 5;

/// Benchmarked base (compute + non-tree storage) plus the depth-dependent ZK-tree leaf
/// insert performed by the wormhole proof recorder. `insert_leaf` walks the tree
/// leaf-to-root, so DB ops and PoV scale with `tree_ops` via
/// [`pallet_zk_tree::TREE_KEY_POV`].
fn claim_weight(db: RuntimeDbWeight, (tree_reads, tree_writes): (u64, u64)) -> Weight {
	// Minimum execution time: 111_000_000 picoseconds.
	Weight::from_parts(114_000_000, 8619)
		.saturating_add(Weight::from_parts(
			0,
			tree_reads.saturating_mul(pallet_zk_tree::TREE_KEY_POV),
		))
		.saturating_add(db.reads(CLAIM_BASE_READS.saturating_add(tree_reads)))
		.saturating_add(db.writes(CLAIM_BASE_WRITES.saturating_add(tree_writes)))
}

/// See [`claim_weight`].
fn end_schedule_weight(db: RuntimeDbWeight, (tree_reads, tree_writes): (u64, u64)) -> Weight {
	// Minimum execution time: 136_000_000 picoseconds.
	Weight::from_parts(138_000_000, 8799)
		.saturating_add(Weight::from_parts(
			0,
			tree_reads.saturating_mul(pallet_zk_tree::TREE_KEY_POV),
		))
		.saturating_add(db.reads(END_SCHEDULE_BASE_READS.saturating_add(tree_reads)))
		.saturating_add(db.writes(END_SCHEDULE_BASE_WRITES.saturating_add(tree_writes)))
}

/// Weights for `pallet_vesting` using the Substrate node and recommended hardware.
///
/// Bounded on `pallet_zk_tree::Config` because the payout calls' weight reads the
/// current tree depth: paying out records a wormhole proof, which inserts a ZK-tree
/// leaf whose storage cost grows with depth.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config + pallet_zk_tree::Config> WeightInfo for SubstrateWeight<T> {
	/// [`claim_weight`] — benchmarked base plus live-depth tree ops.
	fn claim() -> Weight {
		claim_weight(T::DbWeight::get(), pallet_zk_tree::Pallet::<T>::insert_leaf_db_ops())
	}
	/// Storage: `TreasuryPallet::TreasuryAccount` (r:1 w:0)
	/// Proof: `TreasuryPallet::TreasuryAccount` (`max_values`: Some(1), `max_size`: Some(32), added: 527, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Vesting::NextScheduleId` (r:1 w:1)
	/// Proof: `Vesting::NextScheduleId` (`max_values`: Some(1), `max_size`: Some(8), added: 503, mode: `MaxEncodedLen`)
	/// Storage: `Vesting::Schedules` (r:0 w:1)
	/// Proof: `Vesting::Schedules` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
	fn create_schedule() -> Weight {
		// Minimum execution time: 58_000_000 picoseconds.
		Weight::from_parts(63_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(4_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// [`end_schedule_weight`] — benchmarked base plus live-depth tree ops.
	fn end_schedule() -> Weight {
		end_schedule_weight(T::DbWeight::get(), pallet_zk_tree::Pallet::<T>::insert_leaf_db_ops())
	}
	/// Storage: `Vesting::Schedules` (r:1 w:1)
	/// Proof: `Vesting::Schedules` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
	fn retarget_schedule() -> Weight {
		// Minimum execution time: 10_000_000 picoseconds.
		Weight::from_parts(11_000_000, 3569)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
}

// For backwards compatibility and tests: charges the worst-case (max-depth) tree walk
// since it cannot read the live depth without a `pallet_zk_tree::Config` bound.
impl WeightInfo for () {
	fn claim() -> Weight {
		claim_weight(
			RocksDbWeight::get(),
			pallet_zk_tree::insert_leaf_db_ops_at_depth(pallet_zk_tree::MAX_TREE_DEPTH),
		)
	}
	fn create_schedule() -> Weight {
		Weight::from_parts(63_000_000, 6196)
			.saturating_add(RocksDbWeight::get().reads(4_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn end_schedule() -> Weight {
		end_schedule_weight(
			RocksDbWeight::get(),
			pallet_zk_tree::insert_leaf_db_ops_at_depth(pallet_zk_tree::MAX_TREE_DEPTH),
		)
	}
	fn retarget_schedule() -> Weight {
		Weight::from_parts(11_000_000, 3569)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}
