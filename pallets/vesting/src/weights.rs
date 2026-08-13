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
/// insert cost, before the clamp — see [`payout_weight`].
fn payout_weight_unclamped(
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
}

/// Benchmarked base with its benchmark-time tree ops swapped for the flat marginal
/// insert cost — DB ops, Poseidon hashing and PoV all priced by
/// [`pallet_zk_tree::INSERT_LEAF_*`]. The benchmarks were generated against the
/// per-insert path-update code, so the marginal append can be cheaper than the ops
/// it replaces; clamp to the measured base rather than under-charging it until the
/// benchmarks are regenerated.
/// [`tests::payout_clamp_engagement_matches_current_benchmarks`] pins, per call,
/// whether the clamp is currently live, so regenerated benchmarks that flip the
/// direction fail loudly instead of leaving a dead clamp behind.
fn payout_weight(
	base: Weight,
	db: RuntimeDbWeight,
	benchmark_tree_writes: u64,
	tree_ops: (u64, u64),
	tree_hash_time: u64,
) -> Weight {
	payout_weight_unclamped(base, db, benchmark_tree_writes, tree_ops, tree_hash_time).max(base)
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

	/// Pins, per payout call, whether the marginal-insert substitution currently
	/// falls below the benchmarked base — i.e. whether `payout_weight`'s
	/// `.max(base)` clamp is live. (Asserting `payout_weight().all_gte(base)`
	/// would be tautological: the clamp makes it true for any constants.)
	///
	/// With the current generated bases: `claim` swaps 3 benchmark tree writes for
	/// 4 marginal ones, so the substitution over-covers and the clamp is idle;
	/// `end_schedule` / `retarget_schedule` swap 4 writes for 4 and drop 2 reads,
	/// so the substitution under-covers and the clamp is what holds the floor.
	/// When the benchmarks are regenerated against the batched-settlement code,
	/// these directions change and this test fails — that is the signal to
	/// re-derive the model and drop the clamp if it has gone dead.
	#[test]
	fn payout_clamp_engagement_matches_current_benchmarks() {
		let db = RocksDbWeight::get();
		let cases = [
			("claim", <() as generated::WeightInfo>::claim(), CLAIM_BENCHMARK_TREE_WRITES, false),
			(
				"end_schedule",
				<() as generated::WeightInfo>::end_schedule(),
				BENCHMARK_TREE_WRITES,
				true,
			),
			(
				"retarget_schedule",
				<() as generated::WeightInfo>::retarget_schedule(),
				BENCHMARK_TREE_WRITES,
				true,
			),
		];
		for (name, base, benchmark_tree_writes, clamp_live) in cases {
			let unclamped = payout_weight_unclamped(
				base,
				db,
				benchmark_tree_writes,
				pallet_zk_tree::INSERT_LEAF_DB_OPS,
				pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
			);
			assert_eq!(
				!unclamped.all_gte(base),
				clamp_live,
				"{name}: clamp engagement flipped — benchmarks were regenerated; \
				 re-derive the substitution model and drop the clamp if it is dead \
				 (unclamped {unclamped:?}, base {base:?})"
			);

			let clamped = payout_weight(
				base,
				db,
				benchmark_tree_writes,
				pallet_zk_tree::INSERT_LEAF_DB_OPS,
				pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
			);
			assert!(clamped.all_gte(base), "{name}: clamp must hold the benchmarked floor");
			assert!(clamped.all_gte(unclamped), "{name}: clamp must never reduce the charge");
		}
	}
}
