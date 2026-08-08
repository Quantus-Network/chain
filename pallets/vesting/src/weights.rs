//! Depth-aware weights for `pallet_vesting`.

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
/// [`crate::weights_generated`]: `LeafCount` + `Depth` + 3×`Leaves` reads, and
/// `LeafCount` + `Leaves` + `Root` writes (`Depth` too once the insert grows the tree,
/// which the shallow benchmark tree does for `end_schedule`/`retarget_schedule` but
/// not for `claim`). They are subtracted back out so the live-depth insert cost can
/// replace them; `payout_weight_covers_the_benchmarked_base` pins the result.
const BENCHMARK_TREE_READS: u64 = 5;
const BENCHMARK_TREE_WRITES: u64 = 4;
const CLAIM_BENCHMARK_TREE_WRITES: u64 = 3;

/// Benchmarked base with its benchmark-depth tree ops swapped for the live-depth
/// insert cost — DB ops, Poseidon path hashing and PoV all priced by
/// [`pallet_zk_tree::insert_leaf_weight_at_depth`].
fn payout_weight(
	base: Weight,
	db: RuntimeDbWeight,
	benchmark_tree_writes: u64,
	insert_leaf: Weight,
) -> Weight {
	base.saturating_sub(db.reads(BENCHMARK_TREE_READS))
		.saturating_sub(db.writes(benchmark_tree_writes))
		.saturating_add(insert_leaf)
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config + pallet_zk_tree::Config> WeightInfo for SubstrateWeight<T> {
	fn claim() -> Weight {
		payout_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::claim(),
			T::DbWeight::get(),
			CLAIM_BENCHMARK_TREE_WRITES,
			pallet_zk_tree::Pallet::<T>::insert_leaf_weight(T::DbWeight::get()),
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
			pallet_zk_tree::Pallet::<T>::insert_leaf_weight(T::DbWeight::get()),
		)
	}

	fn retarget_schedule() -> Weight {
		payout_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::retarget_schedule(),
			T::DbWeight::get(),
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::Pallet::<T>::insert_leaf_weight(T::DbWeight::get()),
		)
	}
}

/// Worst-case insert cost for the depth-blind `()` impl.
fn max_depth_insert_leaf() -> Weight {
	pallet_zk_tree::insert_leaf_weight_at_depth(
		RocksDbWeight::get(),
		pallet_zk_tree::MAX_TREE_DEPTH,
	)
}

impl WeightInfo for () {
	fn claim() -> Weight {
		payout_weight(
			<() as generated::WeightInfo>::claim(),
			RocksDbWeight::get(),
			CLAIM_BENCHMARK_TREE_WRITES,
			max_depth_insert_leaf(),
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
			max_depth_insert_leaf(),
		)
	}

	fn retarget_schedule() -> Weight {
		payout_weight(
			<() as generated::WeightInfo>::retarget_schedule(),
			RocksDbWeight::get(),
			BENCHMARK_TREE_WRITES,
			max_depth_insert_leaf(),
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

	#[test]
	fn payout_ref_time_grows_with_tree_depth() {
		let db = RuntimeDbWeight { read: 0, write: 0 };
		let base = Weight::zero();
		let shallow = payout_weight(
			base,
			db,
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::insert_leaf_weight_at_depth(db, 1),
		);
		let deep = payout_weight(
			base,
			db,
			BENCHMARK_TREE_WRITES,
			pallet_zk_tree::insert_leaf_weight_at_depth(db, pallet_zk_tree::MAX_TREE_DEPTH),
		);

		assert!(deep.ref_time() > shallow.ref_time());
		assert!(deep.proof_size() > shallow.proof_size());
	}

	/// The augmentation subtracts hand-maintained `BENCHMARK_TREE_*` counts from the
	/// generated base and adds the live-depth insert back. If a zk-tree cost-model
	/// change ever made a live insert cheaper than the benchmark-time ops it replaces,
	/// the subtraction would silently under-charge — no compile error, no failing
	/// benchmark. Pin it: at every reachable depth the augmented weight must still
	/// cover the measured base.
	#[test]
	fn payout_weight_never_undercharges_the_benchmarked_base() {
		let db = RocksDbWeight::get();
		for depth in 0..=pallet_zk_tree::MAX_TREE_DEPTH {
			let insert_leaf = pallet_zk_tree::insert_leaf_weight_at_depth(db, depth);
			for (base, benchmark_tree_writes) in payout_bases() {
				let augmented = payout_weight(base, db, benchmark_tree_writes, insert_leaf);
				assert!(
					augmented.all_gte(base),
					"depth {depth}: augmented {augmented:?} falls below benchmarked {base:?}"
				);
			}
		}
	}

	/// The depth-blind `()` impl must stay a worst-case bound on the live-depth one.
	#[test]
	fn unit_impl_is_the_worst_case() {
		let db = RocksDbWeight::get();
		for (base, benchmark_tree_writes) in payout_bases() {
			let at_max = payout_weight(base, db, benchmark_tree_writes, max_depth_insert_leaf());
			for depth in 0..=pallet_zk_tree::MAX_TREE_DEPTH {
				let live = payout_weight(
					base,
					db,
					benchmark_tree_writes,
					pallet_zk_tree::insert_leaf_weight_at_depth(db, depth),
				);
				assert!(at_max.all_gte(live), "depth {depth} exceeds the max-depth bound");
			}
		}
	}
}
