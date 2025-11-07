//! Benchmarking setup for pallet-dag-consensus

use super::*;

#[allow(unused)]
use crate::Pallet as DagConsensus;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_std::vec;

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn add_genesis_block() {
		let caller: T::AccountId = whitelisted_caller();
		let genesis_hash = T::Hashing::hash(b"genesis");

		#[extrinsic_call]
		add_block_to_dag(RawOrigin::Signed(caller), genesis_hash, vec![]);

		assert!(GhostdagData::<T>::contains_key(&genesis_hash));
	}

	#[benchmark]
	fn add_linear_block() {
		let caller: T::AccountId = whitelisted_caller();
		let genesis_hash = T::Hashing::hash(b"genesis");
		let child_hash = T::Hashing::hash(b"child");

		// Setup: add genesis first
		let _ = DagConsensus::<T>::add_block_to_dag(
			RawOrigin::Signed(caller.clone()).into(),
			genesis_hash,
			vec![],
		);

		#[extrinsic_call]
		add_block_to_dag(RawOrigin::Signed(caller), child_hash, vec![genesis_hash]);

		assert!(GhostdagData::<T>::contains_key(&child_hash));
	}

	#[benchmark]
	fn add_merge_block() {
		let caller: T::AccountId = whitelisted_caller();
		let genesis_hash = T::Hashing::hash(b"genesis");
		let parent1_hash = T::Hashing::hash(b"parent1");
		let parent2_hash = T::Hashing::hash(b"parent2");
		let merge_hash = T::Hashing::hash(b"merge");

		// Setup: create a simple fork
		let _ = DagConsensus::<T>::add_block_to_dag(
			RawOrigin::Signed(caller.clone()).into(),
			genesis_hash,
			vec![],
		);
		let _ = DagConsensus::<T>::add_block_to_dag(
			RawOrigin::Signed(caller.clone()).into(),
			parent1_hash,
			vec![genesis_hash],
		);
		let _ = DagConsensus::<T>::add_block_to_dag(
			RawOrigin::Signed(caller.clone()).into(),
			parent2_hash,
			vec![genesis_hash],
		);

		#[extrinsic_call]
		add_block_to_dag(RawOrigin::Signed(caller), merge_hash, vec![parent1_hash, parent2_hash]);

		assert!(GhostdagData::<T>::contains_key(&merge_hash));
	}

	#[benchmark]
	fn recalculate_virtual_state() {
		let caller: T::AccountId = whitelisted_caller();
		let genesis_hash = T::Hashing::hash(b"genesis");

		// Setup: add genesis block
		let _ = DagConsensus::<T>::add_block_to_dag(
			RawOrigin::Signed(caller.clone()).into(),
			genesis_hash,
			vec![],
		);

		#[extrinsic_call]
		recalculate_virtual_state(RawOrigin::Signed(caller));

		// Virtual state should be updated
		assert_ne!(VirtualState::<T>::get().selected_parent, T::Hash::default());
	}

	impl_benchmark_test_suite!(DagConsensus, crate::mock::new_test_ext(), crate::mock::Test);
}
