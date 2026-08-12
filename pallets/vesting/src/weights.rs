//! Weights for `pallet_vesting`. Payout paths replace the benchmarked tree component
//! with the flat marginal [`pallet_zk_tree::INSERT_LEAF_*`] insert pricing (the
//! batched root recomputation's depth-dependent tail is reserved per block by the
//! zk-tree pallet's `on_initialize`), clamped to never fall below the benchmarked
//! base.

use crate::weights_generated as generated;
use core::marker::PhantomData;
use frame_support::{
	traits::Get,
	weights::{constants::RocksDbWeight, RuntimeDbWeight, Weight},
};

pub trait WeightInfo {
	fn claim() -> Weight;
	fn create_schedule() -> Weight;
	fn end_schedule() -> Weight;
	fn retarget_schedule() -> Weight;
}

/// ZK-tree storage ops the benchmark itself performed, per the storage tables in
/// [`crate::weights_generated`]: `LeafCount` + `Depth` + 3×`Leaves` reads (= 5), and
/// `LeafCount` + `Leaves` + `Root` writes (= 3); `Depth` is written too once the insert
/// grows the tree, which the shallow benchmark tree does for `end_schedule` /
/// `retarget_schedule` (= 4) but not for `claim` (= 3). They are subtracted back out so
/// the flat marginal insert cost can replace them;
/// [`tests::payout_weight_never_undercharges_the_benchmarked_base`] pins that the
/// replacement never under-charges the measured base.
const BENCHMARK_TREE_READS: u64 = 5;
const BENCHMARK_TREE_WRITES: u64 = 4;
const CLAIM_BENCHMARK_TREE_WRITES: u64 = 3;

/// Benchmarked base with its benchmark-time tree ops swapped for the flat marginal
/// insert cost — DB ops, Poseidon hashing and PoV all priced by
/// [`pallet_zk_tree::INSERT_LEAF_*`]. The benchmarks were generated against the
/// per-insert path-update code, so the marginal append can be cheaper than the ops
/// it replaces; clamp to the measured base rather than under-charging it until the
/// benchmarks are regenerated.
fn payout_weight(
	base: Weight,
	db: RuntimeDbWeight,
	benchmark_tree_writes: u64,
	(tree_reads, tree_writes): (u64, u64),
	tree_hash_time: u64,
) -> Weight {
	base.saturating_sub(db.reads(BENCHMARK_TREE_READS))
		.saturating_sub(db.writes(benchmark_tree_writes))
		.saturating_add(Weight::from_parts(
			tree_hash_time,
			tree_reads
				.saturating_add(tree_writes)
				.saturating_mul(pallet_zk_tree::TREE_KEY_POV),
		))
		.saturating_add(db.reads(tree_reads))
		.saturating_add(db.writes(tree_writes))
		.max(base)
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn claim() -> Weight {
		payout_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::claim(),
			T::DbWeight::get(),
			CLAIM_BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}

	fn create_schedule() -> Weight {
		<generated::SubstrateWeight<T> as generated::WeightInfo>::create_schedule()
	}

	fn end_schedule() -> Weight {
		payout_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::end_schedule(),
			T::DbWeight::get(),
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}

	fn retarget_schedule() -> Weight {
		payout_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::retarget_schedule(),
			T::DbWeight::get(),
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}
}

impl WeightInfo for () {
	fn claim() -> Weight {
		payout_weight(
			<() as generated::WeightInfo>::claim(),
			RocksDbWeight::get(),
			CLAIM_BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}

	fn create_schedule() -> Weight {
		<() as generated::WeightInfo>::create_schedule()
	}

	fn end_schedule() -> Weight {
		payout_weight(
			<() as generated::WeightInfo>::end_schedule(),
			RocksDbWeight::get(),
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}

	fn retarget_schedule() -> Weight {
		payout_weight(
			<() as generated::WeightInfo>::retarget_schedule(),
			RocksDbWeight::get(),
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::INSERT_LEAF_DB_OPS,
			pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Every payout call, paired with the benchmark tree writes `payout_weight`
	/// subtracts back out for it.
	fn payout_bases() -> [(Weight, u64); 3] {
		[
			(<() as generated::WeightInfo>::claim(), CLAIM_BENCHMARK_TREE_WRITES),
			(<() as generated::WeightInfo>::end_schedule(), BENCHMARK_TREE_WRITES),
			(<() as generated::WeightInfo>::retarget_schedule(), BENCHMARK_TREE_WRITES),
		]
	}

	/// The augmentation subtracts hand-maintained `BENCHMARK_TREE_*` counts from the
	/// generated base and adds the flat marginal insert back. The marginal append is
	/// cheaper (in DB ops) than the benchmark-time path update it replaces, so
	/// `payout_weight` clamps to the measured base — pin that the clamp keeps the
	/// augmented weight from ever falling below it.
	#[test]
	fn payout_weight_never_undercharges_the_benchmarked_base() {
		let db = RocksDbWeight::get();
		for (base, benchmark_tree_writes) in payout_bases() {
			let augmented = payout_weight(
				base,
				db,
				benchmark_tree_writes,
				pallet_zk_tree::INSERT_LEAF_DB_OPS,
				pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
			);
			assert!(
				augmented.all_gte(base),
				"augmented {augmented:?} falls below benchmarked {base:?}"
			);
		}
	}
}
