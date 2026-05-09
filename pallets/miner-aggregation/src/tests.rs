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
const L1_FIXTURE_L0_CANDIDATE_0_HEX: &str = include_str!("../test-data/l0_candidate_0.hex");
const L1_FIXTURE_AGGREGATE_HEX: &str = include_str!("../test-data/l1_aggregate.hex");

fn decode_hex_fixture(contents: &str) -> Vec<u8> {
	hex::decode(contents.trim()).expect("valid proof hex")
}

fn proof_bytes() -> Vec<u8> {
	decode_hex_fixture(AGGREGATED_PROOF_HEX)
}

fn l1_fixture_l0_candidate_0_bytes() -> Vec<u8> {
	decode_hex_fixture(L1_FIXTURE_L0_CANDIDATE_0_HEX)
}

fn l1_fixture_aggregate_bytes() -> Vec<u8> {
	decode_hex_fixture(L1_FIXTURE_AGGREGATE_HEX)
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

fn invalid_l0_proof_bytes() -> Vec<u8> {
	let valid_bytes = proof_bytes();
	setup_matching_block_hash(&valid_bytes);

	for idx in (0..valid_bytes.len()).rev() {
		let mut mutated = valid_bytes.clone();
		mutated[idx] ^= 1;
		let Ok(proof) = Wormhole::deserialize_aggregated_proof(&mutated) else {
			continue;
		};
		if Wormhole::parse_aggregated_inputs_from_proof(&proof).is_err() {
			continue;
		}
		if matches!(
			Wormhole::verify_aggregated_proof_for_candidate(&mutated),
			Err(pallet_wormhole::Error::<Test>::AggregatedVerificationFailed)
		) {
			return mutated;
		}
	}

	panic!("could not derive invalid proof bytes that still parse");
}

fn submit_invalid_candidate() -> ([u8; 32], crate::BundleGroupKey) {
	let proof_bytes = invalid_l0_proof_bytes();
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

fn total_balance(account: &AccountId) -> Balance {
	Balances::balance(account).saturating_add(Balances::reserved_balance(account))
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

fn claim_invalid_candidate_bundle() -> ([u8; 32], [u8; 32], Vec<[u8; 32]>) {
	let (candidate_id, group_key) = submit_invalid_candidate();
	register_miner();
	assert_ok!(MinerAggregation::claim_bundle(
		RuntimeOrigin::signed(account_id(2)),
		group_key,
		*account_id(2).as_ref(),
		MinMinerBond::get()
	));
	let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
	let bundle_id = match candidate.status {
		L0CandidateStatus::Claimed { bundle_id } => bundle_id,
		_ => panic!("candidate should be claimed"),
	};
	(candidate_id, bundle_id, candidate.nullifiers.to_vec())
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

struct ClaimedL1FixtureBundle {
	candidate_id: [u8; 32],
	bundle_id: [u8; 32],
	l1_proof_bytes: Vec<u8>,
}

fn claim_l1_fixture_bundle() -> Option<ClaimedL1FixtureBundle> {
	if pallet_wormhole::get_layer1_verifier().is_err() {
		eprintln!(
			"skipping L1 fixture test; rerun with QP_GENERATE_LAYER1=true \
			QP_NUM_LAYER0_PROOFS=1"
		);
		return None;
	}

	let l0_proof_bytes = l1_fixture_l0_candidate_0_bytes();
	setup_matching_block_hash(&l0_proof_bytes);
	System::set_block_number(1);
	assert_ok!(MinerAggregation::submit_l0_candidate(
		RuntimeOrigin::signed(account_id(1)),
		l0_proof_bytes.clone(),
		5
	));
	let candidate_id = sp_io::hashing::blake2_256(&l0_proof_bytes);
	let group_key = L0Candidates::<Test>::get(candidate_id).expect("candidate stored").group_key;
	register_miner();
	assert_ok!(MinerAggregation::claim_bundle(
		RuntimeOrigin::signed(account_id(2)),
		group_key,
		*account_id(2).as_ref(),
		MinMinerBond::get()
	));
	let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
	let bundle_id = match candidate.status {
		L0CandidateStatus::Claimed { bundle_id } => bundle_id,
		_ => panic!("candidate should be claimed"),
	};

	Some(ClaimedL1FixtureBundle {
		candidate_id,
		bundle_id,
		l1_proof_bytes: l1_fixture_aggregate_bytes(),
	})
}

fn add_synthetic_claimed_candidate_to_bundle(bundle_id: [u8; 32]) -> [u8; 32] {
	let existing_id = Bundles::<Test>::get(bundle_id).expect("bundle stored").ordered_candidates[0];
	let mut candidate = L0Candidates::<Test>::get(existing_id).expect("candidate stored");
	let synthetic_id = [77u8; 32];
	let synthetic_nullifier = [88u8; 32];

	candidate.proof_hash = synthetic_id;
	candidate.public_inputs_hash = [99u8; 32];
	candidate.nullifiers = vec![synthetic_nullifier].try_into().expect("bounded nullifiers");
	candidate.status = L0CandidateStatus::Claimed { bundle_id };
	L0Candidates::<Test>::insert(synthetic_id, candidate);

	Bundles::<Test>::mutate(bundle_id, |bundle| {
		bundle
			.as_mut()
			.expect("bundle stored")
			.ordered_candidates
			.try_push(synthetic_id)
			.expect("bundle has capacity");
	});
	let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
	assert_ok!(Wormhole::lock_nullifiers_for_bundle(
		bundle_id,
		bundle.deadline,
		&[synthetic_nullifier]
	));

	synthetic_id
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

fn account_from_digest(digest: &BytesDigest) -> AccountId {
	let bytes: [u8; 32] = digest.as_ref().try_into().expect("digest is 32 bytes");
	AccountId::new(bytes)
}

fn unique_exit_accounts(account_data: &[PublicInputsByAccount]) -> Vec<AccountId> {
	let mut accounts = Vec::new();
	for exit in account_data {
		let account = account_from_digest(&exit.exit_account);
		if !accounts.contains(&account) {
			accounts.push(account);
		}
	}
	accounts
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

fn claimed_bundle_inputs() -> ([u8; 32], [u8; 32], Layer1AggregatedPublicCircuitInputs) {
	let (candidate_id, bundle_id, _nullifiers) = claim_candidate_bundle();
	let inputs = layer1_inputs_for_candidate(candidate_id, bundle_id, *account_id(2).as_ref());
	(candidate_id, bundle_id, inputs)
}

fn assert_l1_proof_mismatch(bundle_id: [u8; 32], inputs: &Layer1AggregatedPublicCircuitInputs) {
	let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
	let err = MinerAggregation::ensure_l1_matches_bundle(&bundle, inputs).unwrap_err();
	assert!(matches!(err, Error::<Test>::ProofMismatch));
}

fn assert_l1_duplicate_nullifier(
	bundle_id: [u8; 32],
	inputs: &Layer1AggregatedPublicCircuitInputs,
) {
	let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
	let err = MinerAggregation::ensure_l1_matches_bundle(&bundle, inputs).unwrap_err();
	assert!(matches!(err, Error::<Test>::DuplicateNullifier));
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
fn l1_full_effect_comparison_rejects_wrong_exit_amount() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.account_data[0].summed_output_amount =
			inputs.account_data[0].summed_output_amount.wrapping_add(1);

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_wrong_exit_account() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.account_data[0].exit_account = BytesDigest::new_unchecked(*account_id(99).as_ref());

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_extra_exit() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		let mut extra_exit = inputs.account_data[0].clone();
		extra_exit.summed_output_amount = extra_exit.summed_output_amount.wrapping_add(7);
		inputs.account_data.push(extra_exit);
		inputs.total_exit_slots = inputs.account_data.len() as u32;

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_missing_exit() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.account_data.pop();
		inputs.total_exit_slots = inputs.account_data.len() as u32;

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_wrong_nullifier() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.nullifiers[0] = BytesDigest::new_unchecked([0xAB; 32]);

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_duplicate_nullifier() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		let duplicate = inputs.nullifiers[0].clone();
		if inputs.nullifiers.len() == 1 {
			inputs.nullifiers.push(duplicate);
		} else {
			inputs.nullifiers[1] = duplicate;
		}

		assert_l1_duplicate_nullifier(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_reordered_exits() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		let mut swap_pair = None;
		'outer: for first in 0..inputs.account_data.len() {
			for second in (first + 1)..inputs.account_data.len() {
				if inputs.account_data[first] != inputs.account_data[second] {
					swap_pair = Some((first, second));
					break 'outer;
				}
			}
		}
		let (first, second) = swap_pair.expect("fixture has distinct exit entries");
		inputs.account_data.swap(first, second);

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn l1_full_effect_comparison_rejects_exit_slot_count_mismatch() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.total_exit_slots = inputs.total_exit_slots.saturating_add(1);

		assert_l1_proof_mismatch(bundle_id, &inputs);
	});
}

#[test]
fn bundle_root_is_metadata_until_constrained_by_l1_circuit() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, mut inputs) = claimed_bundle_inputs();
		inputs.bundle_root = Some(BytesDigest::new_unchecked([0x42; 32]));
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");

		assert_ok!(MinerAggregation::ensure_l1_matches_bundle(&bundle, &inputs));
	});
}

#[test]
fn submit_l1_aggregate_accepts_valid_fixture_and_settles_bundle() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		let miner = account_id(2);
		let reward_account = account_id(2);
		let proof = Wormhole::deserialize_layer1_proof(&fixture.l1_proof_bytes)
			.expect("L1 fixture proof deserializes");
		let inputs = Wormhole::parse_layer1_inputs_from_proof(&proof)
			.expect("L1 fixture public inputs parse");
		let candidate_before =
			L0Candidates::<Test>::get(fixture.candidate_id).expect("candidate stored");
		let bundle_before = Bundles::<Test>::get(fixture.bundle_id).expect("bundle stored");
		let nullifiers = candidate_before.nullifiers.to_vec();
		let prepared = Wormhole::prepare_public_output_settlement(
			&inputs.account_data,
			inputs.volume_fee_bps,
			pallet_wormhole::SettlementKind::DelegatedL1 {
				aggregation_reward_account: reward_account.clone(),
			},
		)
		.expect("settlement prepares");
		let exit_accounts = unique_exit_accounts(&inputs.account_data);
		let exit_balances_before = exit_accounts
			.iter()
			.map(|account| (account.clone(), Balances::balance(account)))
			.collect::<Vec<_>>();
		let reward_balance_before = Balances::balance(&reward_account);
		let miner_reserved_before = Balances::reserved_balance(&miner);

		assert_eq!(bundle_before.status, BundleStatus::Claimed);
		assert_eq!(MinerActiveBundles::<Test>::get(&miner).as_slice(), &[fixture.bundle_id]);
		assert_eq!(
			RegisteredAggregators::<Test>::get(&miner)
				.expect("registered aggregator")
				.active_jobs,
			1
		);
		for nullifier in &nullifiers {
			assert!(Wormhole::is_nullifier_locked(nullifier));
			assert!(!Wormhole::is_nullifier_used(nullifier));
		}

		assert_ok!(MinerAggregation::submit_l1_aggregate(
			RuntimeOrigin::signed(miner.clone()),
			fixture.bundle_id,
			fixture.l1_proof_bytes
		));

		let settled_bundle = Bundles::<Test>::get(fixture.bundle_id).expect("bundle stored");
		assert_eq!(settled_bundle.status, BundleStatus::Settled);
		for candidate_id in settled_bundle.ordered_candidates.iter() {
			let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
			assert_eq!(
				candidate.status,
				L0CandidateStatus::Settled { bundle_id: fixture.bundle_id }
			);
		}
		for nullifier in &nullifiers {
			assert!(!Wormhole::is_nullifier_locked(nullifier));
			assert!(Wormhole::is_nullifier_used(nullifier));
		}
		for (account, balance_before) in exit_balances_before {
			let mut expected_balance =
				balance_before + expected_exit_amount_for(&inputs.account_data, &account);
			if account == reward_account {
				expected_balance = expected_balance +
					candidate_before.aggregation_tip +
					prepared.aggregation_prover_fee;
			}
			if account == miner {
				expected_balance += bundle_before.miner_bond;
			}
			assert_eq!(Balances::balance(&account), expected_balance);
		}
		if !exit_accounts.contains(&reward_account) {
			let mut expected_reward_balance = reward_balance_before +
				candidate_before.aggregation_tip +
				prepared.aggregation_prover_fee;
			if reward_account == miner {
				expected_reward_balance += bundle_before.miner_bond;
			}
			assert_eq!(Balances::balance(&reward_account), expected_reward_balance);
		}
		assert_eq!(
			Balances::reserved_balance(&miner),
			miner_reserved_before - bundle_before.miner_bond
		);
		assert!(MinerActiveBundles::<Test>::get(&miner).is_empty());
		assert_eq!(
			RegisteredAggregators::<Test>::get(&miner)
				.expect("registered aggregator")
				.active_jobs,
			0
		);
		System::assert_has_event(
			crate::Event::<Test>::BundleSettled { bundle_id: fixture.bundle_id, miner }.into(),
		);
	});
}

#[test]
fn l1_fixture_rejects_wrong_bundle_aggregator_address() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		Bundles::<Test>::mutate(fixture.bundle_id, |bundle| {
			bundle.as_mut().expect("bundle stored").aggregator_address = *account_id(42).as_ref();
		});

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				fixture.bundle_id,
				fixture.l1_proof_bytes
			),
			Error::<Test>::ProofMismatch
		);
	});
}

#[test]
fn l1_fixture_rejects_wrong_bundle_block_number() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		Bundles::<Test>::mutate(fixture.bundle_id, |bundle| {
			bundle.as_mut().expect("bundle stored").group_key.block_number += 1;
		});

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				fixture.bundle_id,
				fixture.l1_proof_bytes
			),
			Error::<Test>::ProofMismatch
		);
	});
}

#[test]
fn l1_fixture_rejects_wrong_bundle_block_hash() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		Bundles::<Test>::mutate(fixture.bundle_id, |bundle| {
			bundle.as_mut().expect("bundle stored").group_key.block_hash = [0x55; 32];
		});

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				fixture.bundle_id,
				fixture.l1_proof_bytes
			),
			Error::<Test>::ProofMismatch
		);
	});
}

#[test]
fn l1_fixture_rejects_wrong_candidate_nullifier_set() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		L0Candidates::<Test>::mutate(fixture.candidate_id, |candidate| {
			candidate.as_mut().expect("candidate stored").nullifiers[0] = [0x66; 32];
		});

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				fixture.bundle_id,
				fixture.l1_proof_bytes
			),
			Error::<Test>::ProofMismatch
		);
	});
}

#[test]
fn l1_fixture_rejects_wrong_candidate_exit_summary() {
	new_test_ext().execute_with(|| {
		let Some(fixture) = claim_l1_fixture_bundle() else {
			return;
		};
		L0Candidates::<Test>::mutate(fixture.candidate_id, |candidate| {
			let candidate = candidate.as_mut().expect("candidate stored");
			candidate.exit_summary[0].summed_output_amount =
				candidate.exit_summary[0].summed_output_amount.wrapping_add(1);
		});

		assert_noop!(
			MinerAggregation::submit_l1_aggregate(
				RuntimeOrigin::signed(account_id(2)),
				fixture.bundle_id,
				fixture.l1_proof_bytes
			),
			Error::<Test>::ProofMismatch
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
fn timeout_partially_slashes_miner_bond() {
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
		let miner_total_before = total_balance(&account_id(2));
		let expected_slash = MinerTimeoutSlash::get() * MinMinerBond::get();

		System::set_block_number(BundleProvingPeriod::get() + 2);
		assert_ok!(MinerAggregation::timeout_bundle(
			RuntimeOrigin::signed(account_id(1)),
			bundle_id
		));

		assert_eq!(total_balance(&account_id(2)), miner_total_before - expected_slash);
		assert_eq!(Balances::reserved_balance(&account_id(2)), 100);
		assert_eq!(Bundles::<Test>::get(bundle_id).expect("bundle stored").miner_bond, 0);
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
fn invalid_l1_full_verification_failure_slashes_assigned_miner() {
	new_test_ext().execute_with(|| {
		let (_candidate_id, bundle_id, _nullifiers) = claim_candidate_bundle();
		let mut bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let miner_total_before = total_balance(&account_id(2));
		let expected_slash = InvalidL1ProofSlash::get() * MinMinerBond::get();

		let slashed =
			MinerAggregation::record_invalid_l1_verification_failure(bundle_id, &mut bundle)
				.expect("slashing succeeds");
		let remaining_miner_bond = bundle.miner_bond;
		Bundles::<Test>::insert(bundle_id, bundle);

		assert_eq!(slashed, expected_slash);
		assert_eq!(remaining_miner_bond, MinMinerBond::get() - expected_slash);
		assert_eq!(total_balance(&account_id(2)), miner_total_before - expected_slash);
		assert_eq!(
			Bundles::<Test>::get(bundle_id).expect("bundle stored").status,
			BundleStatus::Claimed
		);
		assert_eq!(MinerActiveBundles::<Test>::get(account_id(2)).as_slice(), &[bundle_id]);
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
		System::set_block_number(1);
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let aggregation_tip = candidate.aggregation_tip;
		let account_data = public_outputs_from_candidate(candidate_id);
		let expected_exit_amount = expected_exit_amount_for(&account_data, &reward_account);
		let expected_fee_share = pallet_wormhole::Pallet::<Test>::prepare_public_output_settlement(
			&account_data,
			VolumeFeeRateBps::get(),
			pallet_wormhole::SettlementKind::DelegatedL1 {
				aggregation_reward_account: reward_account.clone(),
			},
		)
		.expect("settlement prepares")
		.aggregation_prover_fee;
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
			reward_balance_before + aggregation_tip + expected_exit_amount + expected_fee_share
		);
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		assert_eq!(candidate.status, L0CandidateStatus::Settled { bundle_id });
	});
}

#[test]
fn delegated_l1_pays_aggregation_prover_fee_share_to_registered_reward_account() {
	new_test_ext().execute_with(|| {
		let reward_account = account_id(42);
		let (candidate_id, bundle_id, nullifiers) =
			claim_candidate_bundle_with_reward(reward_account.clone());
		System::set_block_number(1);
		let bundle = Bundles::<Test>::get(bundle_id).expect("bundle stored");
		let candidate = L0Candidates::<Test>::get(candidate_id).expect("candidate stored");
		let account_data = public_outputs_from_candidate(candidate_id);
		let prepared = pallet_wormhole::Pallet::<Test>::prepare_public_output_settlement(
			&account_data,
			VolumeFeeRateBps::get(),
			pallet_wormhole::SettlementKind::DelegatedL1 {
				aggregation_reward_account: reward_account.clone(),
			},
		)
		.expect("settlement prepares");
		let fee_share = prepared.aggregation_prover_fee;
		let expected_exit_amount = expected_exit_amount_for(&account_data, &reward_account);
		let reward_balance_before = Balances::balance(&reward_account);

		assert!(fee_share > 0);
		assert_ok!(MinerAggregation::settle_verified_l1_bundle(
			bundle_id,
			bundle,
			nullifiers,
			&account_data,
			VolumeFeeRateBps::get()
		));

		assert_eq!(
			Balances::balance(&reward_account),
			reward_balance_before + candidate.aggregation_tip + expected_exit_amount + fee_share
		);
		System::assert_has_event(
			crate::Event::<Test>::AggregationRewardPaid {
				bundle_id,
				reward_account,
				tips_paid: candidate.aggregation_tip,
				fee_share_paid: fee_share,
			}
			.into(),
		);
	});
}

#[test]
fn pending_invalid_candidate_challenge_rewards_challenger() {
	new_test_ext().execute_with(|| {
		let (candidate_id, group_key) = submit_invalid_candidate();
		let challenger = account_id(2);
		let submitter = account_id(1);
		let challenger_balance_before = Balances::balance(&challenger);
		let submitter_total_before = total_balance(&submitter);
		let expected_reward = InvalidCandidateChallengeReward::get() * ValidityBond::get();

		assert_ok!(MinerAggregation::challenge_invalid_l0_candidate(
			RuntimeOrigin::signed(challenger.clone()),
			candidate_id
		));

		assert_eq!(Balances::balance(&challenger), challenger_balance_before + expected_reward);
		assert_eq!(total_balance(&submitter), submitter_total_before - ValidityBond::get());
		assert_eq!(Balances::reserved_balance(&submitter), 0);
		assert!(PendingQueues::<Test>::get(&group_key).is_empty());
		assert_eq!(
			L0Candidates::<Test>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::ChallengedInvalid
		);
	});
}

#[test]
fn pending_valid_candidate_challenge_does_not_slash() {
	new_test_ext().execute_with(|| {
		let (candidate_id, _group_key) = submit_candidate();
		let reserved_before = Balances::reserved_balance(&account_id(1));
		let total_before = total_balance(&account_id(1));

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
		assert_eq!(total_balance(&account_id(1)), total_before);
	});
}

#[test]
fn claimed_invalid_candidate_challenge_unlocks_nullifiers() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, nullifiers) = claim_invalid_candidate_bundle();
		for nullifier in &nullifiers {
			assert!(Wormhole::is_nullifier_locked(nullifier));
		}

		assert_ok!(MinerAggregation::challenge_invalid_l0_in_bundle(
			RuntimeOrigin::signed(account_id(3)),
			bundle_id,
			candidate_id
		));

		for nullifier in &nullifiers {
			assert!(!Wormhole::is_nullifier_locked(nullifier));
			assert!(!Wormhole::is_nullifier_used(nullifier));
		}
		assert_eq!(
			L0Candidates::<Test>::get(candidate_id).expect("candidate stored").status,
			L0CandidateStatus::ChallengedInvalid
		);
		assert_eq!(
			Bundles::<Test>::get(bundle_id).expect("bundle stored").status,
			BundleStatus::Challenged
		);
	});
}

#[test]
fn claimed_invalid_candidate_challenge_requeues_other_candidates() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, _nullifiers) = claim_invalid_candidate_bundle();
		let synthetic_id = add_synthetic_claimed_candidate_to_bundle(bundle_id);
		let synthetic_nullifier =
			L0Candidates::<Test>::get(synthetic_id).expect("candidate stored").nullifiers[0];
		assert!(Wormhole::is_nullifier_locked(&synthetic_nullifier));

		assert_ok!(MinerAggregation::challenge_invalid_l0_in_bundle(
			RuntimeOrigin::signed(account_id(3)),
			bundle_id,
			candidate_id
		));

		let synthetic = L0Candidates::<Test>::get(synthetic_id).expect("candidate stored");
		assert_eq!(synthetic.status, L0CandidateStatus::Pending);
		assert!(!Wormhole::is_nullifier_locked(&synthetic_nullifier));
		assert_eq!(PendingQueues::<Test>::get(&synthetic.group_key).as_slice(), &[synthetic_id]);
	});
}

#[test]
fn claimed_invalid_candidate_challenge_decrements_active_jobs() {
	new_test_ext().execute_with(|| {
		let (candidate_id, bundle_id, _nullifiers) = claim_invalid_candidate_bundle();
		let miner_total_before = total_balance(&account_id(2));
		let challenger_balance_before = Balances::balance(&account_id(3));
		let expected_miner_slash = InvalidClaimSlash::get() * MinMinerBond::get();
		let expected_challenge_reward =
			InvalidCandidateChallengeReward::get() * ValidityBond::get();

		assert_ok!(MinerAggregation::challenge_invalid_l0_in_bundle(
			RuntimeOrigin::signed(account_id(3)),
			bundle_id,
			candidate_id
		));

		assert!(MinerActiveBundles::<Test>::get(account_id(2)).is_empty());
		assert_eq!(
			RegisteredAggregators::<Test>::get(account_id(2))
				.expect("aggregator registered")
				.active_jobs,
			0
		);
		assert_eq!(Balances::reserved_balance(&account_id(2)), 100);
		assert_eq!(total_balance(&account_id(2)), miner_total_before - expected_miner_slash);
		assert_eq!(
			Balances::balance(&account_id(3)),
			challenger_balance_before + expected_challenge_reward
		);
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
