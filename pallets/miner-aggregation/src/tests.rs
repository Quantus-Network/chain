use crate::{
	mock::*,
	pallet::{
		BundleStatus, Bundles, Error, L0CandidateStatus, L0Candidates, MinerActiveBundles,
		PendingQueues, RegisteredAggregators,
	},
};
use frame_support::{assert_noop, assert_ok};
use qp_wormhole_verifier::parse_aggregated_public_inputs;
use sp_core::H256;

const AGGREGATED_PROOF_HEX: &str = include_str!("../../wormhole/test-data/aggregated.hex");

fn proof_bytes() -> Vec<u8> {
	hex::decode(AGGREGATED_PROOF_HEX.trim()).expect("valid aggregated proof hex")
}

fn setup_matching_block_hash(proof_bytes: &[u8]) {
	let proof = Wormhole::deserialize_aggregated_proof(proof_bytes).expect("proof deserializes");
	let inputs = parse_aggregated_public_inputs(&proof).expect("public inputs parse");
	let block_number = inputs.block_data.block_number as u64;
	let block_hash_bytes: [u8; 32] = inputs.block_data.block_hash.as_ref().try_into().unwrap();
	frame_system::BlockHash::<Test>::insert(block_number, H256::from(block_hash_bytes));
}

fn submit_candidate() -> ([u8; 32], crate::BundleGroupKey) {
	let proof_bytes = proof_bytes();
	setup_matching_block_hash(&proof_bytes);
	assert_ok!(MinerAggregation::submit_l0_candidate(
		RuntimeOrigin::signed(account_id(1)),
		proof_bytes.clone(),
		5
	));
	let candidate_id = sp_io::hashing::blake2_256(&proof_bytes);
	let group_key = L0Candidates::<Test>::get(candidate_id).expect("candidate stored").group_key;
	(candidate_id, group_key)
}

fn register_miner() {
	let reward_account = account_id(2);
	let reward_address: [u8; 32] = *reward_account.as_ref();
	assert_ok!(MinerAggregation::register_aggregator(
		RuntimeOrigin::signed(account_id(2)),
		reward_address,
		2,
		100
	));
}

#[test]
fn valid_candidate_is_queued_and_bonded() {
	new_test_ext().execute_with(|| {
		let submitter = account_id(1);
		let proof_bytes = proof_bytes();
		setup_matching_block_hash(&proof_bytes);

		assert_ok!(MinerAggregation::submit_l0_candidate(
			RuntimeOrigin::signed(submitter.clone()),
			proof_bytes.clone(),
			5
		));

		let candidate_id = sp_io::hashing::blake2_256(&proof_bytes);
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.submitter, submitter);
		assert_eq!(candidate.status, L0CandidateStatus::Pending);
		assert_eq!(candidate.aggregation_tip, 5);
		assert!(!candidate.nullifiers.is_empty());
		assert!(!candidate.exit_summary.is_empty());

		let queue = PendingQueues::<Test>::get(&candidate.group_key);
		assert_eq!(queue.as_slice(), &[candidate_id]);
		assert_eq!(Balances::reserved_balance(&account_id(1)), 35);
	});
}

#[test]
fn candidate_submission_does_not_lock_nullifiers() {
	new_test_ext().execute_with(|| {
		let proof_bytes = proof_bytes();
		setup_matching_block_hash(&proof_bytes);

		assert_ok!(MinerAggregation::submit_l0_candidate(
			RuntimeOrigin::signed(account_id(1)),
			proof_bytes.clone(),
			0
		));

		let candidate_id = sp_io::hashing::blake2_256(&proof_bytes);
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		for nullifier in candidate.nullifiers {
			assert!(!Wormhole::is_nullifier_locked(&nullifier));
			assert!(!Wormhole::is_nullifier_used(&nullifier));
		}
	});
}

#[test]
fn duplicate_candidate_is_rejected() {
	new_test_ext().execute_with(|| {
		let proof_bytes = proof_bytes();
		setup_matching_block_hash(&proof_bytes);

		assert_ok!(MinerAggregation::submit_l0_candidate(
			RuntimeOrigin::signed(account_id(1)),
			proof_bytes.clone(),
			0
		));
		assert_noop!(
			MinerAggregation::submit_l0_candidate(
				RuntimeOrigin::signed(account_id(1)),
				proof_bytes,
				0
			),
			Error::<Test>::DuplicateCandidate
		);
	});
}

#[test]
fn oversized_candidate_is_rejected() {
	new_test_ext().execute_with(|| {
		let too_large = vec![0u8; MaxL0ProofBytes::get() as usize + 1];
		assert_noop!(
			MinerAggregation::submit_l0_candidate(
				RuntimeOrigin::signed(account_id(1)),
				too_large,
				0
			),
			Error::<Test>::ProofTooLarge
		);
	});
}

#[test]
fn stale_or_missing_block_reference_is_rejected() {
	new_test_ext().execute_with(|| {
		let proof_bytes = proof_bytes();
		assert_noop!(
			MinerAggregation::submit_l0_candidate(
				RuntimeOrigin::signed(account_id(1)),
				proof_bytes,
				0
			),
			Error::<Test>::StaleBlockReference
		);
	});
}

#[test]
fn aggregator_registration_is_stored() {
	new_test_ext().execute_with(|| {
		let account = account_id(1);
		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(account.clone()),
			*account.as_ref(),
			2,
			100
		));
		assert!(RegisteredAggregators::<Test>::contains_key(account));
	});
}

#[test]
fn claim_bundle_requires_registration() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, group_key) = submit_candidate();
		assert_noop!(
			MinerAggregation::claim_bundle(
				RuntimeOrigin::signed(account_id(2)),
				group_key,
				*account_id(2).as_ref(),
				MinMinerBond::get()
			),
			Error::<Test>::AggregatorNotRegistered
		);
	});
}

#[test]
fn claim_bundle_locks_nullifiers_and_marks_candidate_claimed() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		register_miner();

		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key.clone(),
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let L0CandidateStatus::Claimed { bundle_id } = candidate.status else {
			panic!("candidate should be claimed");
		};
		for nullifier in candidate.nullifiers {
			assert!(Wormhole::is_nullifier_locked(&nullifier));
		}
		assert!(PendingQueues::<Test>::get(&group_key).is_empty());
		assert_eq!(MinerActiveBundles::<Test>::get(account_id(2)).as_slice(), &[bundle_id]);
		assert_eq!(
			Bundles::<Test>::get(bundle_id).expect("bundle stored").status,
			BundleStatus::Claimed
		);
	});
}

#[test]
fn claim_before_direct_l0_makes_direct_validation_fail() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		register_miner();
		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key,
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let nullifiers = candidate.nullifiers.to_vec();
		let err = Wormhole::ensure_nullifiers_available_for_direct_settlement(&nullifiers)
			.expect_err("direct settlement should reject claimed nullifier");
		assert!(matches!(err, pallet_wormhole::Error::<Test>::NullifierLocked));
	});
}

#[test]
fn timeout_bundle_unlocks_nullifiers_and_returns_candidate() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		register_miner();
		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key.clone(),
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let L0CandidateStatus::Claimed { bundle_id } = candidate.status else {
			panic!("candidate should be claimed");
		};

		System::set_block_number(BundleProvingPeriod::get() + 2);
		assert_ok!(MinerAggregation::timeout_bundle(
			RuntimeOrigin::signed(account_id(1)),
			bundle_id
		));

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Pending);
		for nullifier in candidate.nullifiers {
			assert!(!Wormhole::is_nullifier_locked(&nullifier));
		}
		assert_eq!(PendingQueues::<Test>::get(&group_key).as_slice(), &[candidate_id]);
		assert!(MinerActiveBundles::<Test>::get(account_id(2)).is_empty());
	});
}

#[test]
fn timeout_bundle_refunds_expired_claimed_candidate() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		register_miner();
		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key.clone(),
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let L0CandidateStatus::Claimed { bundle_id } = candidate.status else {
			panic!("candidate should be claimed");
		};
		assert_eq!(Balances::reserved_balance(&account_id(1)), 35);

		System::set_block_number(CandidateLifetime::get() + 1);
		assert_ok!(MinerAggregation::timeout_bundle(
			RuntimeOrigin::signed(account_id(1)),
			bundle_id
		));

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Expired);
		assert!(PendingQueues::<Test>::get(&group_key).is_empty());
		assert_eq!(Balances::reserved_balance(&account_id(1)), 0);
	});
}

#[test]
fn submit_l1_aggregate_rejects_malformed_proof_before_verification() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, group_key) = submit_candidate();
		register_miner();
		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key,
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));
		let bundle_id = MinerActiveBundles::<Test>::get(account_id(2))[0];

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				bundle_id,
				Vec::new()
			),
			Error::<Test>::MalformedL1Proof
		);
	});
}

#[test]
fn challenge_valid_candidate_does_not_slash_submitter() {
	new_test_ext().execute_with(|| {
		let (candidate_id, _group_key) = submit_candidate();
		let reserved_before = Balances::reserved_balance(&account_id(1));

		assert_noop!(
			MinerAggregation::challenge_invalid_l0_candidate(
				RuntimeOrigin::signed(account_id(2)),
				candidate_id
			),
			Error::<Test>::CandidateValid
		);

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Pending);
		assert_eq!(Balances::reserved_balance(&account_id(1)), reserved_before);
	});
}

#[test]
fn drop_expired_candidate_refunds_reserves_and_removes_queue_entry() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		assert_eq!(Balances::reserved_balance(&account_id(1)), 35);

		System::set_block_number(CandidateLifetime::get() + 1);
		assert_ok!(MinerAggregation::drop_expired_candidate(
			RuntimeOrigin::signed(account_id(2)),
			candidate_id
		));

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Expired);
		assert!(PendingQueues::<Test>::get(&group_key).is_empty());
		assert_eq!(Balances::reserved_balance(&account_id(1)), 0);
	});
}
