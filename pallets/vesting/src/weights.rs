//! Weights for `pallet_vesting`. Calls that record wormhole leaves replace the
//! benchmarked insert-phase tree ops with the flat marginal
//! `pallet_zk_tree::INSERT_LEAF_*` price per leaf, at
//! [`pallet_zk_tree::CIRCUIT_MAX_TREE_DEPTH`].

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

/// Insert-phase ZK-tree ops the benchmark itself performed, per the storage tables in
/// [`crate::weights_generated`]: `LeafCount` + `UnprocessedLeaves` read and written
/// once per call (= 2 reads, 2 writes), plus one `Leaves` write per recorded leaf.
/// They are subtracted back out so the flat marginal insert price can replace them;
/// [`tests::recorded_weight_never_undercharges_the_benchmarked_base`] pins that the
/// replacement never under-charges the measured base.
const BENCHMARK_TREE_READS: u64 = 2;
const BENCHMARK_TREE_WRITES: u64 = 2;

/// Leaves each call records: `claim` pays the beneficiary, `create_schedule` funds
/// the pot from the treasury, `end_schedule` pays the beneficiary and refunds the
/// treasury. `retarget_schedule` moves no funds and records nothing.
const CLAIM_LEAVES: u64 = 1;
const CREATE_SCHEDULE_LEAVES: u64 = 1;
const END_SCHEDULE_LEAVES: u64 = 2;

/// Benchmarked base with its insert-phase tree ops swapped for `leaves` flat
/// marginal inserts — DB ops, Poseidon hashing and PoV all priced by
/// `pallet_zk_tree::INSERT_LEAF_*`. The recorder's `TransferCount` work is in the
/// base already: the benchmarks exercise every leg.
fn recorded_weight(base: Weight, db: RuntimeDbWeight, leaves: u64) -> Weight {
	let (tree_reads, tree_writes) = pallet_zk_tree::INSERT_LEAF_DB_OPS;
	let reads = leaves.saturating_mul(tree_reads);
	let writes = leaves.saturating_mul(tree_writes);
	base.saturating_sub(db.reads(BENCHMARK_TREE_READS))
		.saturating_sub(db.writes(BENCHMARK_TREE_WRITES.saturating_add(leaves)))
		.saturating_add(Weight::from_parts(
			leaves.saturating_mul(pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS),
			reads.saturating_add(writes).saturating_mul(pallet_zk_tree::TREE_KEY_POV),
		))
		.saturating_add(db.reads(reads))
		.saturating_add(db.writes(writes))
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn claim() -> Weight {
		recorded_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::claim(),
			T::DbWeight::get(),
			CLAIM_LEAVES,
		)
	}

	fn create_schedule() -> Weight {
		recorded_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::create_schedule(),
			T::DbWeight::get(),
			CREATE_SCHEDULE_LEAVES,
		)
	}

	fn end_schedule() -> Weight {
		recorded_weight(
			<generated::SubstrateWeight<T> as generated::WeightInfo>::end_schedule(),
			T::DbWeight::get(),
			END_SCHEDULE_LEAVES,
		)
	}

	fn retarget_schedule() -> Weight {
		<generated::SubstrateWeight<T> as generated::WeightInfo>::retarget_schedule()
	}
}

impl WeightInfo for () {
	fn claim() -> Weight {
		recorded_weight(<() as generated::WeightInfo>::claim(), RocksDbWeight::get(), CLAIM_LEAVES)
	}

	fn create_schedule() -> Weight {
		recorded_weight(
			<() as generated::WeightInfo>::create_schedule(),
			RocksDbWeight::get(),
			CREATE_SCHEDULE_LEAVES,
		)
	}

	fn end_schedule() -> Weight {
		recorded_weight(
			<() as generated::WeightInfo>::end_schedule(),
			RocksDbWeight::get(),
			END_SCHEDULE_LEAVES,
		)
	}

	fn retarget_schedule() -> Weight {
		<() as generated::WeightInfo>::retarget_schedule()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Every recording call, paired with the leaves it records.
	fn recorded_bases() -> [(Weight, u64); 3] {
		[
			(<() as generated::WeightInfo>::claim(), CLAIM_LEAVES),
			(<() as generated::WeightInfo>::create_schedule(), CREATE_SCHEDULE_LEAVES),
			(<() as generated::WeightInfo>::end_schedule(), END_SCHEDULE_LEAVES),
		]
	}

	/// The augmentation subtracts hand-maintained `BENCHMARK_TREE_*` counts from the
	/// generated base and adds the flat marginal insert back. If a zk-tree cost-model
	/// change ever made that insert cheaper (in DB ops) than the benchmark-time ops it
	/// replaces, the subtraction would silently under-charge — no compile error, no
	/// failing benchmark. Pin it: the augmented weight must still cover the measured
	/// base.
	#[test]
	fn recorded_weight_never_undercharges_the_benchmarked_base() {
		let db = RocksDbWeight::get();
		for (base, leaves) in recorded_bases() {
			let augmented = recorded_weight(base, db, leaves);
			assert!(
				augmented.all_gte(base),
				"augmented {augmented:?} falls below benchmarked {base:?}"
			);
		}
	}
}
