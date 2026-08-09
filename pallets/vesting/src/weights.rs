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

const BENCHMARK_TREE_READS: u64 = 5;
const BENCHMARK_TREE_WRITES: u64 = 4;
const CLAIM_BENCHMARK_TREE_WRITES: u64 = 3;

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
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config + pallet_zk_tree::Config> WeightInfo for SubstrateWeight<T> {
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
