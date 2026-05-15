//! Benchmarking setup for pallet-miner-aggregation.

extern crate alloc;

use super::*;
use alloc::vec::Vec;
use codec::Decode;
use frame_benchmarking::v2::*;
use frame_support::{
	assert_noop, assert_ok,
	traits::{Currency, Get},
};
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use qp_wormhole_verifier::parse_aggregated_public_inputs;
use sp_runtime::traits::{One, SaturatedConversion, Saturating};

const AGGREGATED_PROOF_HEX: &str = include_str!("../../wormhole/test-data/aggregated.hex");
const L1_FIXTURE_L0_CANDIDATE_0_HEX: &str = include_str!("../test-data/l0_candidate_0.hex");
const L1_FIXTURE_AGGREGATE_HEX: &str = include_str!("../test-data/l1_aggregate.hex");

fn balance<T: Config>(value: u128) -> BalanceOf<T> {
	value.saturated_into()
}

fn proof_bytes() -> Vec<u8> {
	hex::decode(AGGREGATED_PROOF_HEX.trim()).expect("valid aggregated proof hex")
}

fn l1_fixture_l0_candidate_0_bytes() -> Vec<u8> {
	hex::decode(L1_FIXTURE_L0_CANDIDATE_0_HEX.trim()).expect("valid L1 fixture L0 proof hex")
}

fn l1_fixture_aggregate_bytes() -> Vec<u8> {
	hex::decode(L1_FIXTURE_AGGREGATE_HEX.trim()).expect("valid L1 aggregate proof hex")
}

fn fund<T: Config>(account: &T::AccountId) {
	let amount = balance::<T>(1_000_000_000_000_000_000);
	let _ = <T as Config>::Currency::deposit_creating(account, amount);
}

fn setup_matching_block_hash<T: Config>(proof_bytes: &[u8]) {
	let proof = pallet_wormhole::Pallet::<T>::deserialize_aggregated_proof(proof_bytes)
		.expect("proof deserializes");
	let inputs = parse_aggregated_public_inputs(&proof).expect("public inputs parse");
	let block_number = BlockNumberFor::<T>::from(inputs.block_data.block_number);
	let block_hash_bytes: [u8; 32] =
		inputs.block_data.block_hash.as_ref().try_into().expect("digest is 32 bytes");
	let block_hash = <T as frame_system::Config>::Hash::decode(&mut &block_hash_bytes[..])
		.expect("runtime hash decodes from 32 bytes");
	frame_system::BlockHash::<T>::insert(block_number, block_hash);
}

fn invalid_l0_proof_bytes<T: Config>() -> Vec<u8> {
	let valid_bytes = proof_bytes();
	setup_matching_block_hash::<T>(&valid_bytes);

	for idx in (0..valid_bytes.len()).rev() {
		let mut mutated = valid_bytes.clone();
		mutated[idx] ^= 1;
		let Ok(proof) = pallet_wormhole::Pallet::<T>::deserialize_aggregated_proof(&mutated) else {
			continue;
		};
		if pallet_wormhole::Pallet::<T>::parse_aggregated_inputs_from_proof(&proof).is_err() {
			continue;
		}
		if matches!(
			pallet_wormhole::Pallet::<T>::verify_aggregated_proof_for_candidate(&mutated),
			Err(pallet_wormhole::Error::<T>::AggregatedVerificationFailed)
		) {
			return mutated;
		}
	}

	panic!("could not derive invalid proof bytes that still parse");
}

fn candidate_id_for(proof_bytes: &[u8]) -> CandidateId {
	sp_io::hashing::blake2_256(proof_bytes)
}

fn seed_candidate_queue<T: Config>(
	submitter: &T::AccountId,
	invalid_first: bool,
) -> (CandidateId, BundleGroupKey) {
	fund::<T>(submitter);
	let proof_bytes = if invalid_first { invalid_l0_proof_bytes::<T>() } else { proof_bytes() };
	setup_matching_block_hash::<T>(&proof_bytes);
	assert_ok!(Pallet::<T>::submit_l0_candidate(
		RawOrigin::Signed(submitter.clone()).into(),
		proof_bytes.clone(),
		balance::<T>(1_000)
	));

	let first_id = candidate_id_for(&proof_bytes);
	let first = L0Candidates::<T>::get(first_id).expect("candidate stored");
	let group_key = first.group_key.clone();
	let required = T::NumLayer0Proofs::get().max(1) as usize;

	for index in 1usize..required {
		let synthetic_id = sp_io::hashing::blake2_256(&index.to_le_bytes());
		let mut nullifiers = first.nullifiers.to_vec();
		for (nullifier_index, nullifier) in nullifiers.iter_mut().enumerate() {
			*nullifier = [index as u8; 32];
			nullifier[31] = nullifier_index as u8;
		}
		let candidate = L0Candidate {
			proof_hash: synthetic_id,
			public_inputs_hash: [index as u8; 32],
			group_key: first.group_key.clone(),
			submitter: first.submitter.clone(),
			submitted_at: first.submitted_at,
			expires_at: first.expires_at,
			proof_bytes: first.proof_bytes.to_vec().try_into().expect("proof bytes fit"),
			nullifiers: nullifiers.try_into().expect("nullifiers fit"),
			exit_summary: first.exit_summary.to_vec().try_into().expect("exit summary fits"),
			aggregation_tip: first.aggregation_tip,
			storage_bond: first.storage_bond,
			validity_bond: first.validity_bond,
			status: L0CandidateStatus::Pending,
		};
		L0Candidates::<T>::insert(synthetic_id, candidate);
		PendingQueues::<T>::try_mutate(&group_key, |queue| {
			queue.try_push(synthetic_id).map_err(|_| Error::<T>::QueueFull)
		})
		.expect("synthetic candidate fits queue");
	}

	(first_id, group_key)
}

fn register_benchmark_aggregator<T: Config>(
	miner: &T::AccountId,
	reward_address: [u8; 32],
	max_active_jobs: u32,
) {
	fund::<T>(miner);
	assert_ok!(Pallet::<T>::register_aggregator(
		RawOrigin::Signed(miner.clone()).into(),
		reward_address,
		max_active_jobs,
		balance::<T>(100_000)
	));
}

fn setup_claimable_bundle<T: Config>(
	invalid_first: bool,
) -> (T::AccountId, CandidateId, BundleGroupKey, [u8; 32]) {
	let submitter: T::AccountId = account("submitter", 0, 0);
	let miner: T::AccountId = account("miner", 0, 0);
	let reward_address = [7u8; 32];
	let (candidate_id, group_key) = seed_candidate_queue::<T>(&submitter, invalid_first);
	register_benchmark_aggregator::<T>(
		&miner,
		reward_address,
		T::MaxActiveBundlesPerMiner::get().max(1),
	);
	(miner, candidate_id, group_key, reward_address)
}

fn claim_seeded_bundle<T: Config>(invalid_first: bool) -> (T::AccountId, CandidateId, BundleId) {
	let (miner, candidate_id, group_key, reward_address) =
		setup_claimable_bundle::<T>(invalid_first);
	assert_ok!(Pallet::<T>::claim_bundle(
		RawOrigin::Signed(miner.clone()).into(),
		group_key,
		reward_address,
		T::MinMinerBond::get()
	));
	let bundle_id = MinerActiveBundles::<T>::get(&miner)
		.first()
		.copied()
		.expect("active bundle recorded");
	(miner, candidate_id, bundle_id)
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn submit_l0_candidate() {
		let submitter: T::AccountId = whitelisted_caller();
		fund::<T>(&submitter);
		let proof_bytes = proof_bytes();
		setup_matching_block_hash::<T>(&proof_bytes);
		let candidate_id = candidate_id_for(&proof_bytes);

		#[extrinsic_call]
		_(RawOrigin::Signed(submitter.clone()), proof_bytes, balance::<T>(1_000));

		assert!(L0Candidates::<T>::contains_key(candidate_id));
	}

	#[benchmark]
	fn register_aggregator() {
		let miner: T::AccountId = whitelisted_caller();
		fund::<T>(&miner);
		let reward_address = [7u8; 32];

		#[extrinsic_call]
		_(RawOrigin::Signed(miner.clone()), reward_address, 4, balance::<T>(100_000));

		assert!(RegisteredAggregators::<T>::contains_key(miner));
	}

	#[benchmark]
	fn update_aggregator() {
		let miner: T::AccountId = whitelisted_caller();
		register_benchmark_aggregator::<T>(&miner, [7u8; 32], 4);
		let new_reward_address = [8u8; 32];

		#[extrinsic_call]
		_(RawOrigin::Signed(miner.clone()), new_reward_address, 2, balance::<T>(50_000));

		assert_eq!(
			RegisteredAggregators::<T>::get(miner)
				.expect("aggregator stored")
				.reward_address,
			new_reward_address
		);
	}

	#[benchmark]
	fn unregister_aggregator() {
		let miner: T::AccountId = whitelisted_caller();
		register_benchmark_aggregator::<T>(&miner, [7u8; 32], 4);

		#[extrinsic_call]
		_(RawOrigin::Signed(miner.clone()));

		assert!(!RegisteredAggregators::<T>::contains_key(miner));
	}

	#[benchmark]
	fn claim_bundle() {
		let (miner, candidate_id, group_key, reward_address) = setup_claimable_bundle::<T>(false);

		#[extrinsic_call]
		_(RawOrigin::Signed(miner.clone()), group_key, reward_address, T::MinMinerBond::get());

		assert!(matches!(
			L0Candidates::<T>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::Claimed { .. }
		));
	}

	#[benchmark]
	fn timeout_bundle() {
		let (miner, _candidate_id, bundle_id) = claim_seeded_bundle::<T>(false);
		let bundle = Bundles::<T>::get(bundle_id).expect("bundle stored");
		frame_system::Pallet::<T>::set_block_number(bundle.deadline.saturating_add(One::one()));

		#[extrinsic_call]
		_(RawOrigin::Signed(miner.clone()), bundle_id);

		assert!(MinerActiveBundles::<T>::get(miner).is_empty());
	}

	#[benchmark]
	fn submit_l1_aggregate_cheap_reject() {
		let (miner, _candidate_id, bundle_id) = claim_seeded_bundle::<T>(false);

		#[block]
		{
			assert_noop!(
				Pallet::<T>::submit_l1_aggregate(
					RawOrigin::Signed(miner.clone()).into(),
					bundle_id,
					Vec::new()
				),
				Error::<T>::MalformedL1Proof
			);
		}
	}

	#[benchmark]
	fn submit_l1_aggregate_valid_proof() {
		assert_eq!(T::NumLayer0Proofs::get(), 1, "L1 fixture benchmark requires NumLayer0Proofs=1");
		assert!(
			pallet_wormhole::get_layer1_verifier().is_ok(),
			"L1 fixture benchmark requires QP_GENERATE_LAYER1=true"
		);
		let submitter: T::AccountId = account("submitter", 0, 0);
		let miner: T::AccountId = account("miner", 0, 0);
		let reward_address = [7u8; 32];
		fund::<T>(&submitter);
		fund::<T>(&miner);
		let l0_proof_bytes = l1_fixture_l0_candidate_0_bytes();
		setup_matching_block_hash::<T>(&l0_proof_bytes);
		assert_ok!(Pallet::<T>::submit_l0_candidate(
			RawOrigin::Signed(submitter).into(),
			l0_proof_bytes.clone(),
			balance::<T>(1_000)
		));
		let candidate_id = candidate_id_for(&l0_proof_bytes);
		let group_key = L0Candidates::<T>::get(candidate_id).expect("candidate stored").group_key;
		register_benchmark_aggregator::<T>(&miner, reward_address, 1);
		assert_ok!(Pallet::<T>::claim_bundle(
			RawOrigin::Signed(miner.clone()).into(),
			group_key,
			reward_address,
			T::MinMinerBond::get()
		));
		let bundle_id = MinerActiveBundles::<T>::get(&miner)
			.first()
			.copied()
			.expect("active bundle recorded");

		#[block]
		{
			assert_ok!(Pallet::<T>::submit_l1_aggregate(
				RawOrigin::Signed(miner.clone()).into(),
				bundle_id,
				l1_fixture_aggregate_bytes()
			));
		}
	}

	#[benchmark]
	fn challenge_invalid_l0_candidate() {
		let submitter: T::AccountId = account("submitter", 0, 0);
		let challenger: T::AccountId = whitelisted_caller();
		let (candidate_id, _group_key) = seed_candidate_queue::<T>(&submitter, true);
		fund::<T>(&challenger);

		#[extrinsic_call]
		_(RawOrigin::Signed(challenger), candidate_id);

		assert_eq!(
			L0Candidates::<T>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::ChallengedInvalid
		);
	}

	#[benchmark]
	fn challenge_invalid_l0_in_bundle() {
		let (miner, candidate_id, bundle_id) = claim_seeded_bundle::<T>(true);
		let challenger: T::AccountId = whitelisted_caller();
		fund::<T>(&challenger);

		#[extrinsic_call]
		_(RawOrigin::Signed(challenger), bundle_id, candidate_id);

		assert!(MinerActiveBundles::<T>::get(miner).is_empty());
		assert_eq!(
			L0Candidates::<T>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::ChallengedInvalid
		);
	}

	#[benchmark]
	fn drop_expired_candidate() {
		let submitter: T::AccountId = account("submitter", 0, 0);
		let caller: T::AccountId = whitelisted_caller();
		let (candidate_id, _group_key) = seed_candidate_queue::<T>(&submitter, false);
		fund::<T>(&caller);
		let candidate = L0Candidates::<T>::get(candidate_id).expect("candidate stored");
		frame_system::Pallet::<T>::set_block_number(
			candidate.expires_at.saturating_add(One::one()),
		);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller), candidate_id);

		assert_eq!(
			L0Candidates::<T>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::Expired
		);
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
