#![allow(clippy::unit_cmp)]

use crate::{mock::*, Error, Event};
use codec::Encode;
use frame_support::{
	assert_noop, assert_ok,
	traits::{InspectLockableCurrency, LockIdentifier},
	BoundedVec,
};
use sp_core::blake2_256;
use sp_runtime::TokenError;

fn bounded_proof(proof: Vec<[u8; 32]>) -> BoundedVec<[u8; 32], MaxProofs> {
	proof.try_into().expect("Proof exceeds maximum size")
}

// Helper function to calculate a leaf hash for testing
fn calculate_leaf_hash(account: &u64, amount: u64) -> [u8; 32] {
	let account_bytes = account.encode();
	let amount_bytes = amount.encode();
	let leaf_data = [&account_bytes[..], &amount_bytes[..]].concat();

	blake2_256(&leaf_data)
}

// Helper function to calculate a parent hash for testing
fn calculate_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
	let combined = if left < right {
		[&left[..], &right[..]].concat()
	} else {
		[&right[..], &left[..]].concat()
	};

	blake2_256(&combined)
}

const VESTING_ID: LockIdentifier = *b"vesting ";

#[test]
fn create_airdrop_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let merkle_root = [0u8; 32];
		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(100),
			Some(10)
		));

		let airdrop_metadata = crate::AirdropMetadata {
			merkle_root,
			creator: 1,
			balance: 0,
			vesting_period: Some(100),
			vesting_delay: Some(10),
		};

		System::assert_last_event(
			Event::AirdropCreated { airdrop_id: 0, airdrop_metadata: airdrop_metadata.clone() }
				.into(),
		);

		assert_eq!(MerkleAirdrop::airdrop_info(0), Some(airdrop_metadata));
	});
}

#[test]
fn fund_airdrop_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let merkle_root = [0u8; 32];
		let amount = 100;

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(10),
			Some(10)
		));

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 0);

		// fund airdrop with insufficient balance should fail
		assert_noop!(
			MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(123456), 0, amount * 10000),
			TokenError::FundsUnavailable,
		);

		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

		System::assert_last_event(Event::AirdropFunded { airdrop_id: 0, amount }.into());

		// Check that the airdrop balance was updated
		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, amount);

		// Check that the balance was transferred
		assert_eq!(Balances::free_balance(1), 9999900); // 10000000 - 100
		assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 101);

		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, amount * 2);
		assert_eq!(Balances::free_balance(1), 9999800); // 9999900 - 100
		assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 201); // locked for vesting
	});
}

#[test]
fn claim_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let account1: u64 = 2; // Account that will claim
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(100),
			Some(2)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		// Create proof for account1d
		let merkle_proof = bounded_proof(vec![leaf2]);

		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 500, merkle_proof.clone()));

		System::assert_last_event(Event::Claimed { airdrop_id: 0, account: 2, amount: 500 }.into());

		assert_eq!(MerkleAirdrop::is_claimed(0, 2), ());
		assert_eq!(Balances::balance_locked(VESTING_ID, &2), 500); // Unlocked

		assert_eq!(Balances::free_balance(2), 500);
		assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 501); // 1 (initial) + 1000
		                                                                // (funded) - 500 (claimed)
	});
}

#[test]
fn create_airdrop_requires_signed_origin() {
	new_test_ext().execute_with(|| {
		let merkle_root = [0u8; 32];

		assert_noop!(
			MerkleAirdrop::create_airdrop(RuntimeOrigin::none(), merkle_root, None, None),
			frame_support::error::BadOrigin
		);
	});
}

#[test]
fn fund_airdrop_fails_for_nonexistent_airdrop() {
	new_test_ext().execute_with(|| {
		assert_noop!(
			MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 999, 1000),
			Error::<Test>::AirdropNotFound
		);
	});
}

#[test]
fn claim_fails_for_nonexistent_airdrop() {
	new_test_ext().execute_with(|| {
		let merkle_proof = bounded_proof(vec![[0u8; 32]]);

		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 999, 1, 500, merkle_proof),
			Error::<Test>::AirdropNotFound
		);
	});
}

#[test]
fn claim_already_claimed() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let account1: u64 = 2; // Account that will claim
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(100),
			Some(10)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		let merkle_proof = bounded_proof(vec![leaf2]);

		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 500, merkle_proof.clone()));

		// Try to claim again
		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 500, merkle_proof.clone()),
			Error::<Test>::AlreadyClaimed
		);
	});
}

#[test]
fn verify_merkle_proof_works() {
	new_test_ext().execute_with(|| {
		// Create test accounts and amounts
		let account1: u64 = 1;
		let amount1: u64 = 500;
		let account2: u64 = 2;
		let amount2: u64 = 300;

		// Calculate leaf hashes
		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);

		// Calculate the Merkle root (hash of the two leaves)
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		// Create proofs
		let proof_for_account1 = vec![leaf2];
		let proof_for_account2 = vec![leaf1];

		// Test the verify_merkle_proof function directly
		assert!(
			MerkleAirdrop::verify_merkle_proof(
				&account1,
				amount1,
				&merkle_root,
				&proof_for_account1
			),
			"Proof for account1 should be valid"
		);

		assert!(
			MerkleAirdrop::verify_merkle_proof(
				&account2,
				amount2,
				&merkle_root,
				&proof_for_account2
			),
			"Proof for account2 should be valid"
		);

		assert!(
			!MerkleAirdrop::verify_merkle_proof(
				&account1,
				400, // Wrong amount
				&merkle_root,
				&proof_for_account1
			),
			"Proof with wrong amount should be invalid"
		);

		let wrong_proof = vec![[1u8; 32]];
		assert!(
			!MerkleAirdrop::verify_merkle_proof(&account1, amount1, &merkle_root, &wrong_proof),
			"Wrong proof should be invalid"
		);

		assert!(
			!MerkleAirdrop::verify_merkle_proof(
				&3, // Wrong account
				amount1,
				&merkle_root,
				&proof_for_account1
			),
			"Proof with wrong account should be invalid"
		);
	});
}

#[test]
fn claim_invalid_proof_fails() {
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(100),
			Some(10)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		let invalid_proof = bounded_proof(vec![[1u8; 32]]); // Different from the actual leaf2

		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 500, invalid_proof),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn claim_insufficient_airdrop_balance_fails() {
	new_test_ext().execute_with(|| {
		// Create a valid merkle tree
		let account1: u64 = 2;
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(1000),
			Some(100)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 400)); // Fund less than claim amount

		let merkle_proof = bounded_proof(vec![leaf2]);

		// Attempt to claim more than available
		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 500, merkle_proof),
			Error::<Test>::InsufficientAirdropBalance
		);
	});
}

#[test]
fn claim_nonexistent_airdrop_fails() {
	new_test_ext().execute_with(|| {
		// Attempt to claim from a nonexistent airdrop
		assert_noop!(
			MerkleAirdrop::claim(
				RuntimeOrigin::none(),
				999,
				2,
				500,
				bounded_proof(vec![[0u8; 32]])
			),
			Error::<Test>::AirdropNotFound
		);
	});
}

#[test]
fn claim_updates_balances_correctly() {
	new_test_ext().execute_with(|| {
		// Create a valid merkle tree
		let account1: u64 = 2;
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(100),
			Some(10)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		let initial_account_balance = Balances::free_balance(2);
		let initial_pallet_balance = Balances::free_balance(MerkleAirdrop::account_id());

		assert_ok!(MerkleAirdrop::claim(
			RuntimeOrigin::none(),
			0,
			2,
			500,
			bounded_proof(vec![leaf2])
		));

		assert_eq!(Balances::free_balance(2), initial_account_balance + 500);
		assert_eq!(
			Balances::free_balance(MerkleAirdrop::account_id()),
			initial_pallet_balance - 500
		);

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 500);
		assert_eq!(MerkleAirdrop::is_claimed(0, 2), ());
	});
}

#[test]
fn multiple_users_can_claim() {
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let amount1: u64 = 5000;
		let account2: u64 = 3;
		let amount2: u64 = 3000;
		let account3: u64 = 4;
		let amount3: u64 = 2000;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let leaf3 = calculate_leaf_hash(&account3, amount3);
		let parent1 = calculate_parent_hash(&leaf1, &leaf2);
		let merkle_root = calculate_parent_hash(&parent1, &leaf3);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			Some(1000),
			Some(10)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 10001));

		// User 1 claims
		let proof1 = bounded_proof(vec![leaf2, leaf3]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 2, 5000, proof1));
		assert_eq!(Balances::free_balance(2), 5000); // free balance but it's locked for vesting
		assert_eq!(Balances::balance_locked(VESTING_ID, &2), 5000);

		// User 2 claims
		let proof2 = bounded_proof(vec![leaf1, leaf3]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 3, 3000, proof2));
		assert_eq!(Balances::free_balance(3), 3000);
		assert_eq!(Balances::balance_locked(VESTING_ID, &3), 3000);

		// User 3 claims
		let proof3 = bounded_proof(vec![parent1]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 4, 2000, proof3));
		assert_eq!(Balances::free_balance(4), 2000);
		assert_eq!(Balances::balance_locked(VESTING_ID, &4), 2000);

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 1);

		assert_eq!(MerkleAirdrop::is_claimed(0, 2), ());
		assert_eq!(MerkleAirdrop::is_claimed(0, 3), ());
		assert_eq!(MerkleAirdrop::is_claimed(0, 4), ());
	});
}

#[test]
fn delete_airdrop_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let merkle_root = [0u8; 32];
		let creator = 1;

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(creator),
			merkle_root,
			Some(100),
			Some(10)
		));

		let airdrop_info = MerkleAirdrop::airdrop_info(0).unwrap();

		assert_eq!(airdrop_info.creator, creator);

		// Delete the airdrop (balance is zero)
		assert_ok!(MerkleAirdrop::delete_airdrop(RuntimeOrigin::signed(creator), 0));

		System::assert_last_event(Event::AirdropDeleted { airdrop_id: 0 }.into());

		// Check that the airdrop no longer exists
		assert!(MerkleAirdrop::airdrop_info(0).is_none());
	});
}

#[test]
fn delete_airdrop_with_balance_refunds_creator() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let merkle_root = [0u8; 32];
		let creator = 1;
		let initial_creator_balance = Balances::free_balance(creator);
		let fund_amount = 100;

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(creator),
			merkle_root,
			Some(100),
			Some(10)
		));

		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(creator), 0, fund_amount));

		// Creator's balance should be reduced by fund_amount
		assert_eq!(Balances::free_balance(creator), initial_creator_balance - fund_amount);

		assert_ok!(MerkleAirdrop::delete_airdrop(RuntimeOrigin::signed(creator), 0));

		// Check that the funds were returned to the creator
		assert_eq!(Balances::free_balance(creator), initial_creator_balance);

		System::assert_last_event(Event::AirdropDeleted { airdrop_id: 0 }.into());
	});
}

#[test]
fn delete_airdrop_non_creator_fails() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let merkle_root = [0u8; 32];
		let creator = 1;
		let non_creator = 2;

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(creator),
			merkle_root,
			Some(100),
			Some(10)
		));

		assert_noop!(
			MerkleAirdrop::delete_airdrop(RuntimeOrigin::signed(non_creator), 0),
			Error::<Test>::NotAirdropCreator
		);
	});
}

#[test]
fn delete_airdrop_nonexistent_fails() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		assert_noop!(
			MerkleAirdrop::delete_airdrop(RuntimeOrigin::signed(1), 999),
			Error::<Test>::AirdropNotFound
		);
	});
}

#[test]
fn delete_airdrop_after_claims_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator: u64 = 1;
		let initial_creator_balance = Balances::free_balance(creator);
		let account1: u64 = 2;
		let amount1: u64 = 500;
		let account2: u64 = 3;
		let amount2: u64 = 300;
		let total_fund = 1000;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(creator),
			merkle_root,
			Some(100),
			Some(10)
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(creator), 0, total_fund));

		// Let only one account claim (partial claiming)
		let proof1 = bounded_proof(vec![leaf2]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount1, proof1));

		// Check that some balance remains
		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, total_fund - amount1);

		// Now the creator deletes the airdrop with remaining balance
		assert_ok!(MerkleAirdrop::delete_airdrop(RuntimeOrigin::signed(creator), 0));

		// Check creator was refunded the unclaimed amount
		assert_eq!(
			Balances::free_balance(creator),
			initial_creator_balance - total_fund + (total_fund - amount1)
		);
	});
}
