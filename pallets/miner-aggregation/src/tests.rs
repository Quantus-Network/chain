use crate::{
	mock::*,
	pallet::{
		BundleStatus, Bundles, Error, L0CandidateStatus, L0Candidates, MinerActiveBundles,
		PendingQueues, RegisteredAggregators,
	},
};
use frame_support::{assert_noop, assert_ok, traits::fungible::Inspect};
use qp_wormhole_verifier::{
	parse_aggregated_public_inputs, BlockData, BytesDigest, Layer1AggregatedPublicCircuitInputs,
	PublicInputsByAccount,
};
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
	register_miner_with_reward(account_id(2));
}

fn register_miner_with_reward(reward_account: AccountId) {
	let reward_address: [u8; 32] = *reward_account.as_ref();
	assert_ok!(MinerAggregation::register_aggregator(
		RuntimeOrigin::signed(account_id(2)),
		reward_address,
		2,
		100
	));
}

fn claim_candidate_bundle() -> ([u8; 32], [u8; 32], Vec<[u8; 32]>) {
	claim_candidate_bundle_with_reward(account_id(2))
}

fn claim_candidate_bundle_with_reward(
	reward_account: AccountId,
) -> ([u8; 32], [u8; 32], Vec<[u8; 32]>) {
	let (candidate_id, group_key) = submit_candidate();
	register_miner_with_reward(reward_account.clone());
	assert_ok!(MinerAggregation::claim_bundle(
		RuntimeOrigin::signed(account_id(2)),
		group_key,
		*reward_account.as_ref(),
		MinMinerBond::get()
	));
	let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
	let bundle_id = match candidate.status {
		L0CandidateStatus::Claimed { bundle_id } => bundle_id,
		_ => panic!("candidate should be claimed"),
	};
	(candidate_id, bundle_id, candidate.nullifiers.to_vec())
}

fn public_outputs_from_candidate(candidate_id: [u8; 32]) -> Vec<PublicInputsByAccount> {
	let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
	candidate
		.exit_summary
		.iter()
		.map(|exit| PublicInputsByAccount {
			summed_output_amount: exit.summed_output_amount,
			exit_account: BytesDigest::new_unchecked(exit.exit_account),
		})
		.collect()
}

fn layer1_inputs_for_candidate(
	candidate_id: [u8; 32],
	bundle_id: [u8; 32],
	aggregator_address: [u8; 32],
) -> Layer1AggregatedPublicCircuitInputs {
	let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
	let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
	let account_data = public_outputs_from_candidate(candidate_id);
	let nullifiers = candidate
		.nullifiers
		.iter()
		.map(|nullifier| BytesDigest::new_unchecked(*nullifier))
		.collect();

	Layer1AggregatedPublicCircuitInputs {
		aggregator_address: BytesDigest::new_unchecked(aggregator_address),
		asset_id: bundle.group_key.asset_id,
		volume_fee_bps: bundle.group_key.volume_fee_bps,
		block_data: BlockData {
			block_hash: BytesDigest::new_unchecked(bundle.group_key.block_hash),
			block_number: bundle.group_key.block_number,
		},
		total_exit_slots: account_data.len() as u32,
		account_data,
		nullifiers,
		bundle_root: None,
		circuit_id: None,
		layout_version: None,
	}
}

fn expected_exit_amount_for(
	account_data: &[PublicInputsByAccount],
	account: &AccountId,
) -> Balance {
	let account_bytes: &[u8] = account.as_ref();
	account_data
		.iter()
		.filter(|exit| exit.exit_account.as_ref() == account_bytes)
		.map(|exit| {
			(exit.summed_output_amount as Balance)
				.saturating_mul(pallet_wormhole::SCALE_DOWN_FACTOR)
		})
		.sum()
}

fn below_minimum_public_outputs() -> Vec<PublicInputsByAccount> {
	vec![PublicInputsByAccount {
		summed_output_amount: 1,
		exit_account: BytesDigest::new_unchecked(*account_id(3).as_ref()),
	}]
}

fn fail_verified_l1_settlement(candidate_id: [u8; 32], bundle_id: [u8; 32]) {
	let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
	let nullifiers = L0Candidates::<Test>::get(candidate_id)
		.expect("candidate stored")
		.nullifiers
		.to_vec();
	let err = MinerAggregation::settle_verified_l1_bundle(
		bundle_id,
		bundle,
		nullifiers,
		&below_minimum_public_outputs(),
		VolumeFeeRateBps::get(),
	)
	.unwrap_err();

	assert_eq!(err, Error::<Test>::ProofMismatch.into());
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
fn aggregator_registration_re_register_does_not_strand_old_bond() {
	new_test_ext().execute_with(|| {
		let miner = account_id(2);
		let first_reward = account_id(42);
		let second_reward = account_id(43);

		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*first_reward.as_ref(),
			2,
			100
		));
		assert_eq!(Balances::reserved_balance(&miner), 100);

		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*second_reward.as_ref(),
			3,
			40
		));
		let info = RegisteredAggregators::<Test>::get(&miner).expect("registered");
		let second_reward_address: [u8; 32] = *second_reward.as_ref();
		assert_eq!(info.reward_address, second_reward_address);
		assert_eq!(info.max_active_jobs, 3);
		assert_eq!(info.bond, 40);
		assert_eq!(info.active_jobs, 0);
		assert_eq!(Balances::reserved_balance(&miner), 40);

		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*first_reward.as_ref(),
			4,
			75
		));
		let info = RegisteredAggregators::<Test>::get(&miner).expect("registered");
		let first_reward_address: [u8; 32] = *first_reward.as_ref();
		assert_eq!(info.reward_address, first_reward_address);
		assert_eq!(info.max_active_jobs, 4);
		assert_eq!(info.bond, 75);
		assert_eq!(Balances::reserved_balance(&miner), 75);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&miner));
	});
}

#[test]
fn aggregator_registration_update_extrinsic_adjusts_bond_and_reward_address() {
	new_test_ext().execute_with(|| {
		let miner = account_id(2);
		let first_reward = account_id(42);
		let second_reward = account_id(43);

		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*first_reward.as_ref(),
			2,
			100
		));
		assert_ok!(MinerAggregation::update_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*second_reward.as_ref(),
			1,
			25
		));

		let info = RegisteredAggregators::<Test>::get(&miner).expect("registered");
		let second_reward_address: [u8; 32] = *second_reward.as_ref();
		assert_eq!(info.reward_address, second_reward_address);
		assert_eq!(info.max_active_jobs, 1);
		assert_eq!(info.bond, 25);
		assert_eq!(info.active_jobs, 0);
		assert_eq!(Balances::reserved_balance(&miner), 25);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&miner));
	});
}

#[test]
fn aggregator_registration_re_register_rejects_active_jobs() {
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
		let L0CandidateStatus::Claimed { bundle_id } = candidate.status else {
			panic!("candidate should be claimed");
		};

		assert_noop!(
			MinerAggregation::register_aggregator(
				RuntimeOrigin::signed(account_id(2)),
				*account_id(42).as_ref(),
				3,
				75
			),
			Error::<Test>::AggregatorHasActiveJobs
		);
		assert_noop!(
			MinerAggregation::update_aggregator(
				RuntimeOrigin::signed(account_id(2)),
				*account_id(42).as_ref(),
				3,
				75
			),
			Error::<Test>::AggregatorHasActiveJobs
		);

		let info = RegisteredAggregators::<Test>::get(account_id(2)).expect("registered");
		let original_reward_address: [u8; 32] = *account_id(2).as_ref();
		assert_eq!(info.reward_address, original_reward_address);
		assert_eq!(info.bond, 100);
		assert_eq!(info.active_jobs, 1);
		assert_eq!(MinerActiveBundles::<Test>::get(account_id(2)).as_slice(), &[bundle_id]);
		assert_eq!(Balances::reserved_balance(&account_id(2)), 100 + MinMinerBond::get());
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));
	});
}

#[test]
fn aggregator_registration_unregister_releases_bond_when_no_active_jobs() {
	new_test_ext().execute_with(|| {
		let miner = account_id(2);
		assert_ok!(MinerAggregation::register_aggregator(
			RuntimeOrigin::signed(miner.clone()),
			*miner.as_ref(),
			2,
			100
		));
		assert_eq!(Balances::reserved_balance(&miner), 100);

		assert_ok!(MinerAggregation::unregister_aggregator(RuntimeOrigin::signed(miner.clone())));

		assert!(!RegisteredAggregators::<Test>::contains_key(&miner));
		assert_eq!(Balances::reserved_balance(&miner), 0);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&miner));
	});
}

#[test]
fn aggregator_registration_unregister_rejects_when_active_bundle_exists() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, group_key) = submit_candidate();
		register_miner();
		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key,
			*account_id(2).as_ref(),
			MinMinerBond::get()
		));

		assert_noop!(
			MinerAggregation::unregister_aggregator(RuntimeOrigin::signed(account_id(2))),
			Error::<Test>::AggregatorHasActiveJobs
		);
		assert!(RegisteredAggregators::<Test>::contains_key(account_id(2)));
		assert_eq!(Balances::reserved_balance(&account_id(2)), 100 + MinMinerBond::get());
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));
	});
}

#[test]
fn claim_bundle_uses_registered_reward_address() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, group_key) = submit_candidate();
		let reward_account = account_id(42);
		register_miner_with_reward(reward_account.clone());

		assert_ok!(MinerAggregation::claim_bundle(
			RuntimeOrigin::signed(account_id(2)),
			group_key,
			*reward_account.as_ref(),
			MinMinerBond::get()
		));

		let bundle_id = MinerActiveBundles::<Test>::get(account_id(2))[0];
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let reward_address: [u8; 32] = *reward_account.as_ref();
		assert_eq!(bundle.aggregator_address, reward_address);
	});
}

#[test]
fn claim_bundle_rejects_mismatched_aggregator_address() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_candidate();
		let reward_account = account_id(42);
		let wrong_reward_account = account_id(43);
		register_miner_with_reward(reward_account);

		assert_noop!(
			MinerAggregation::claim_bundle(
				RuntimeOrigin::signed(account_id(2)),
				group_key.clone(),
				*wrong_reward_account.as_ref(),
				MinMinerBond::get()
			),
			Error::<Test>::AggregatorAddressMismatch
		);

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
fn submit_l1_aggregate_rejects_proof_with_wrong_aggregator_address() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, _nullifiers) = claim_candidate_bundle();
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let wrong_inputs =
			layer1_inputs_for_candidate(candidate_id, bundle_id, *account_id(42).as_ref());

		let err = MinerAggregation::ensure_l1_matches_bundle(&bundle, &wrong_inputs).unwrap_err();
		assert!(matches!(err, Error::<Test>::ProofMismatch));
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
fn active_jobs_timeout_decrements_consistently() {
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
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("registered")
				.active_jobs,
			1
		);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));

		System::set_block_number(BundleProvingPeriod::get() + 2);
		assert_ok!(MinerAggregation::timeout_bundle(
			RuntimeOrigin::signed(account_id(1)),
			bundle_id
		));

		assert!(MinerActiveBundles::<Test>::get(account_id(2)).is_empty());
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("registered")
				.active_jobs,
			0
		);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));
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
fn active_jobs_settlement_decrements_consistently() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, nullifiers) = claim_candidate_bundle();
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let account_data = public_outputs_from_candidate(candidate_id);
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("registered")
				.active_jobs,
			1
		);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));

		assert_ok!(MinerAggregation::settle_verified_l1_bundle(
			bundle_id,
			bundle,
			nullifiers,
			&account_data,
			VolumeFeeRateBps::get()
		));

		assert!(MinerActiveBundles::<Test>::get(account_id(2)).is_empty());
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("registered")
				.active_jobs,
			0
		);
		assert_ok!(MinerAggregation::ensure_aggregator_active_jobs_consistent(&account_id(2)));
	});
}

#[test]
fn submit_l1_aggregate_settlement_failure_preserves_nullifier_locks() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, nullifiers) = claim_candidate_bundle();

		fail_verified_l1_settlement(candidate_id, bundle_id);

		for nullifier in nullifiers {
			assert!(Wormhole::is_nullifier_locked(&nullifier));
			assert!(!Wormhole::is_nullifier_used(&nullifier));
		}
	});
}

#[test]
fn submit_l1_aggregate_settlement_failure_preserves_candidate_status() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, _nullifiers) = claim_candidate_bundle();

		fail_verified_l1_settlement(candidate_id, bundle_id);

		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Claimed { bundle_id });
		assert_eq!(
			Bundles::<Test>::get(bundle_id).expect("bundle stored").status,
			BundleStatus::Claimed
		);
	});
}

#[test]
fn submit_l1_aggregate_settlement_failure_preserves_miner_active_bundle() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, _nullifiers) = claim_candidate_bundle();

		fail_verified_l1_settlement(candidate_id, bundle_id);

		assert_eq!(MinerActiveBundles::<Test>::get(account_id(2)).as_slice(), &[bundle_id]);
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("aggregator registered")
				.active_jobs,
			1
		);
	});
}

#[test]
fn tips_are_paid_to_registered_reward_account() {
	new_test_ext().execute_with(|| {
		let reward_account = account_id(42);
		let (candidate_id, bundle_id, nullifiers) =
			claim_candidate_bundle_with_reward(reward_account.clone());
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let aggregation_tip = candidate.aggregation_tip;
		let account_data = public_outputs_from_candidate(candidate_id);
		let expected_exit_amount = expected_exit_amount_for(&account_data, &reward_account);
		let reward_balance_before = Balances::balance(&reward_account);

		assert_ok!(MinerAggregation::settle_verified_l1_bundle(
			bundle_id,
			bundle,
			nullifiers,
			&account_data,
			VolumeFeeRateBps::get()
		));

		assert_eq!(
			Balances::balance(&reward_account),
			reward_balance_before + aggregation_tip + expected_exit_amount
		);
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Settled { bundle_id });
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
