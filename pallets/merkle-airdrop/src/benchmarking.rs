//! Benchmarking setup for pallet-merkle-airdrop

extern crate alloc;

use super::*;
use crate::Pallet as MerkleAirdrop;
use frame_benchmarking::v2::*;
use frame_support::BoundedVec;
use frame_system::RawOrigin;
use sp_io::hashing::blake2_256;
use sp_runtime::traits::{Get, Saturating};

// Helper function to mirror pallet's Merkle proof verification logic
fn calculate_expected_root_for_benchmark(
	initial_leaf_hash: MerkleHash,
	proof_elements: &[MerkleHash],
) -> MerkleHash {
	let mut computed_hash = initial_leaf_hash;
	for proof_element in proof_elements.iter() {
		// The comparison logic must match how MerkleHash is ordered in your pallet
		if computed_hash.as_ref() < proof_element.as_ref() {
			// This replicates Self::calculate_parent_hash_blake2(&computed_hash, proof_element)
			let mut combined_data = computed_hash.as_ref().to_vec();
			combined_data.extend_from_slice(proof_element.as_ref());
			computed_hash = blake2_256(&combined_data);
		} else {
			// This replicates Self::calculate_parent_hash_blake2(proof_element, &computed_hash)
			let mut combined_data = proof_element.as_ref().to_vec();
			combined_data.extend_from_slice(computed_hash.as_ref());
			computed_hash = blake2_256(&combined_data);
		}
	}
	computed_hash
}

#[benchmarks(
    where
    T: Send + Sync,
    T: Config + pallet_vesting::Config,
)]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn create_airdrop() {
		let caller: T::AccountId = whitelisted_caller();
		let merkle_root = [0u8; 32];
		let vesting_period = None;
		let vesting_schedule = None;

		#[extrinsic_call]
		create_airdrop(RawOrigin::Signed(caller), merkle_root, vesting_period, vesting_schedule);
	}

	#[benchmark]
	fn fund_airdrop() {
		let caller: T::AccountId = whitelisted_caller();
		let merkle_root = [0u8; 32];

		let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();
		AirdropInfo::<T>::insert(
			airdrop_id,
			AirdropMetadata {
				merkle_root,
				balance: 0u32.into(),
				creator: caller.clone(),
				vesting_period: None,
				vesting_delay: None,
			},
		);

		NextAirdropId::<T>::put(airdrop_id + 1);

		let amount: BalanceOf<T> = 100u32.into();

		// Get ED and ensure caller has sufficient balance
		let ed = CurrencyOf::<T>::minimum_balance();

		let caller_balance = ed.saturating_mul(10u32.into()).saturating_add(amount);
		CurrencyOf::<T>::make_free_balance_be(&caller, caller_balance);

		CurrencyOf::<T>::make_free_balance_be(&MerkleAirdrop::<T>::account_id(), ed);

		#[extrinsic_call]
		fund_airdrop(RawOrigin::Signed(caller), airdrop_id, amount);
	}

	#[benchmark]
	fn claim(p: Linear<0, { T::MaxProofs::get() }>) {
		let caller: T::AccountId = whitelisted_caller();
		let recipient: T::AccountId = account("recipient", 0, 0);

		let amount: BalanceOf<T> = 100u32.into();

		// 1. Calculate the initial leaf hash
		let leaf_hash = MerkleAirdrop::<T>::calculate_leaf_hash_blake2(&recipient, amount);

		// 2. Generate `p` dummy proof elements that will be passed to the extrinsic
		let proof_elements_for_extrinsic: alloc::vec::Vec<MerkleHash> = (0..p)
			.map(|i| {
				let mut dummy_data = [0u8; 32];
				dummy_data[0] = i as u8; // Make them slightly different for each proof element
				blake2_256(&dummy_data) // Hash it to make it a valid MerkleHash type
			})
			.collect();

		let merkle_root_to_store =
			calculate_expected_root_for_benchmark(leaf_hash, &proof_elements_for_extrinsic);

		let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();

		AirdropInfo::<T>::insert(
			airdrop_id,
			AirdropMetadata {
				merkle_root: merkle_root_to_store,
				balance: amount.saturating_mul(2u32.into()), // Ensure enough balance for the claim
				creator: caller.clone(),
				vesting_period: None, // Simplest case: no vesting period
				vesting_delay: None,  // Simplest case: no vesting delay
			},
		);

		let large_balance =
			amount.saturating_mul(T::MaxProofs::get().into()).saturating_add(amount);

		// Creator might not be strictly needed for `claim` from `None` origin, but good practice
		CurrencyOf::<T>::make_free_balance_be(&caller, large_balance);
		// Recipient starts with minimal balance or nothing, will receive the airdrop
		CurrencyOf::<T>::make_free_balance_be(&recipient, amount);
		// Pallet's account needs funds to make the transfer
		CurrencyOf::<T>::make_free_balance_be(
			&MerkleAirdrop::<T>::account_id(),
			large_balance, // Pallet account needs enough to cover the claim
		);

		AirdropInfo::<T>::mutate(airdrop_id, |maybe_info| {
			if let Some(info) = maybe_info {
				info.balance = large_balance;
			}
		});

		// Prepare the Merkle proof argument for the extrinsic call
		let merkle_proof_arg =
			BoundedVec::<MerkleHash, T::MaxProofs>::try_from(proof_elements_for_extrinsic)
				.expect("Proof elements vector should fit into BoundedVec");

		// Ensure recipient hasn't claimed yet (benchmark state should be clean)
		assert!(!Claimed::<T>::contains_key(airdrop_id, &recipient));

		#[extrinsic_call]
		claim(RawOrigin::None, airdrop_id, recipient.clone(), amount, merkle_proof_arg);

		// Verify successful claim
		assert!(Claimed::<T>::contains_key(airdrop_id, &recipient));
	}

	#[benchmark]
	fn delete_airdrop() {
		let caller: T::AccountId = whitelisted_caller();
		let merkle_root = [0u8; 32];

		// Create an airdrop first
		let airdrop_id = MerkleAirdrop::<T>::next_airdrop_id();

		AirdropInfo::<T>::insert(
			airdrop_id,
			AirdropMetadata {
				merkle_root,
				balance: 0u32.into(),
				creator: caller.clone(),
				vesting_period: None,
				vesting_delay: None,
			},
		);

		NextAirdropId::<T>::put(airdrop_id + 1);

		let ed = CurrencyOf::<T>::minimum_balance();
		let tiny_amount: BalanceOf<T> = 1u32.into();
		let large_balance = ed.saturating_mul(1_000_000u32.into());

		CurrencyOf::<T>::make_free_balance_be(&caller, large_balance);
		CurrencyOf::<T>::make_free_balance_be(&MerkleAirdrop::<T>::account_id(), large_balance);

		AirdropInfo::<T>::mutate(airdrop_id, |info| {
			if let Some(info) = info {
				info.balance = tiny_amount;
			}
		});

		#[extrinsic_call]
		delete_airdrop(RawOrigin::Signed(caller), airdrop_id);
	}

	impl_benchmark_test_suite!(MerkleAirdrop, crate::mock::new_test_ext(), crate::mock::Test);
}
