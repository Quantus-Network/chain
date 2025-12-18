#![allow(clippy::unit_cmp)]

use crate::{mock::*, Error, Event};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, BoundedVec};
use sp_core::blake2_256;
use sp_runtime::TokenError;

fn bounded_proof(proof: Vec<[u8; 32]>) -> BoundedVec<[u8; 32], MaxProofs> {
	proof.try_into().expect("Proof exceeds maximum size")
}

// Helper function to calculate a leaf hash for testing
// Uses tuple encode to match pallet implementation
fn calculate_leaf_hash(account: &u64, amount: u64) -> [u8; 32] {
	let bytes = (account, amount).encode(); // Tuple encode - matches pallet!
	blake2_256(&bytes)
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
		// Note: Custom vesting holds tokens in pallet account, not locked on user account

		// User doesn't get tokens immediately - they're in vesting schedule
		assert_eq!(Balances::free_balance(2), 0);
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

		// User doesn't get tokens immediately - they're in vesting schedule
		assert_eq!(Balances::free_balance(2), initial_account_balance);
		// Tokens are transferred from airdrop to vesting pallet
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
		// Tokens are in vesting schedule, not user account
		assert_eq!(Balances::free_balance(2), 0);

		// User 2 claims
		let proof2 = bounded_proof(vec![leaf1, leaf3]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 3, 3000, proof2));
		assert_eq!(Balances::free_balance(3), 0);

		// User 3 claims
		let proof3 = bounded_proof(vec![parent1]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 4, 2000, proof3));
		assert_eq!(Balances::free_balance(4), 0);

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

#[test]
fn cannot_use_proof_from_different_airdrop() {
	// SECURITY: Prevents proof replay attacks across different airdrops
	// Attack scenario: Attacker sees valid claim in airdrop A,
	// tries to reuse same proof in airdrop B
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let amount1: u64 = 1000;

		// Create two different airdrops with different merkle roots
		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&3, 500);
		let merkle_root_a = calculate_parent_hash(&leaf1, &leaf2);
		let merkle_root_b = [0xff; 32]; // Completely different root

		// Airdrop A
		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root_a,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 2000));

		// Airdrop B
		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root_b,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 1, 2000));

		// Valid proof for airdrop A
		let proof_a = bounded_proof(vec![leaf2]);

		// Claim from A should work
		assert_ok!(MerkleAirdrop::claim(
			RuntimeOrigin::none(),
			0,
			account1,
			amount1,
			proof_a.clone()
		));

		// SECURITY: Try to reuse same proof in airdrop B - should FAIL
		assert_noop!(
			MerkleAirdrop::claim(
				RuntimeOrigin::none(),
				1, // Different airdrop!
				account1,
				amount1,
				proof_a
			),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn cannot_modify_amount_with_valid_proof() {
	// SECURITY: Attacker tries to claim more by modifying amount
	// but keeping the same proof - should fail
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let correct_amount: u64 = 1000;
		let inflated_amount: u64 = 10000; // 10x more!

		let leaf1 = calculate_leaf_hash(&account1, correct_amount);
		let leaf2 = calculate_leaf_hash(&3, 500);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 20000));

		let proof = bounded_proof(vec![leaf2]);

		// Try to claim with inflated amount but same proof
		assert_noop!(
			MerkleAirdrop::claim(
				RuntimeOrigin::none(),
				0,
				account1,
				inflated_amount, // Wrong amount!
				proof
			),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn cannot_claim_with_siblings_proof() {
	// SECURITY: Attacker tries to use someone else's proof
	// to claim their allocation - should fail
	new_test_ext().execute_with(|| {
		let alice: u64 = 2;
		let bob: u64 = 3;
		let alice_amount: u64 = 1000;
		let bob_amount: u64 = 500;

		let leaf_alice = calculate_leaf_hash(&alice, alice_amount);
		let leaf_bob = calculate_leaf_hash(&bob, bob_amount);
		let merkle_root = calculate_parent_hash(&leaf_alice, &leaf_bob);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 2000));

		// Bob's proof
		let bob_proof = bounded_proof(vec![leaf_alice]);

		// Alice tries to use Bob's proof - should FAIL
		assert_noop!(
			MerkleAirdrop::claim(
				RuntimeOrigin::none(),
				0,
				alice,      // Alice trying...
				bob_amount, // ...with Bob's amount...
				bob_proof   // ...and Bob's proof
			),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn sum_of_claims_never_exceeds_airdrop_balance() {
	// INVARIANT: Total claimed ≤ Total funded
	// This is critical for solvency
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let account2: u64 = 3;
		let account3: u64 = 4;
		let amount_each: u64 = 1000;

		// Create 3-user merkle tree
		let leaf1 = calculate_leaf_hash(&account1, amount_each);
		let leaf2 = calculate_leaf_hash(&account2, amount_each);
		let leaf3 = calculate_leaf_hash(&account3, amount_each);
		let parent1 = calculate_parent_hash(&leaf1, &leaf2);
		let merkle_root = calculate_parent_hash(&parent1, &leaf3);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));

		// Fund LESS than total allocations (only 2500 instead of 3000)
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 2500));

		let initial_balance = MerkleAirdrop::airdrop_info(0).unwrap().balance;

		// First claim - should work
		let proof1 = bounded_proof(vec![leaf2, leaf3]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount_each, proof1));

		let balance_after_1 = MerkleAirdrop::airdrop_info(0).unwrap().balance;
		assert_eq!(balance_after_1, initial_balance - amount_each);

		// Second claim - should work
		let proof2 = bounded_proof(vec![leaf1, leaf3]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account2, amount_each, proof2));

		let balance_after_2 = MerkleAirdrop::airdrop_info(0).unwrap().balance;
		assert_eq!(balance_after_2, initial_balance - 2 * amount_each);

		// Third claim - should FAIL (insufficient balance)
		let proof3 = bounded_proof(vec![parent1]);
		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account3, amount_each, proof3),
			Error::<Test>::InsufficientAirdropBalance
		);

		// Invariant: balance remains consistent (500 left from 2500 - 2*1000)
		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 500);
	});
}

#[test]
fn single_leaf_merkle_tree_works() {
	// EDGE CASE: Airdrop with only 1 recipient (no siblings)
	// Proof should be empty array
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let amount: u64 = 1000;

		// Single leaf = leaf hash is also the root
		let merkle_root = calculate_leaf_hash(&account1, amount);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		// Empty proof for single leaf tree
		let proof = bounded_proof(vec![]);

		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount, proof));

		// Verify claim was successful
		assert_eq!(MerkleAirdrop::is_claimed(0, account1), ());
		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 0);
	});
}

#[test]
fn maximum_depth_merkle_tree_works() {
	// EDGE CASE: Deep merkle tree with many proof elements
	// Tests gas limits and storage constraints
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let amount: u64 = 1000;

		let leaf = calculate_leaf_hash(&account1, amount);

		// Build proof with multiple levels (testing with 10 levels)
		let mut proof_elements = Vec::new();
		let mut current_hash = leaf;

		for i in 0..10 {
			let sibling = [i as u8; 32]; // Fake siblings for testing
			proof_elements.push(sibling);
			current_hash = calculate_parent_hash(&current_hash, &sibling);
		}

		let merkle_root = current_hash;

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		let proof = bounded_proof(proof_elements);

		// Should handle deep tree without issues
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount, proof));

		assert_eq!(MerkleAirdrop::is_claimed(0, account1), ());
	});
}

#[test]
fn claim_with_zero_amount_should_be_rejected() {
	// EDGE CASE: Attempting to claim 0 tokens
	// Even with valid proof, this should be rejected or create invalid vesting
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let zero_amount: u64 = 0;

		let leaf1 = calculate_leaf_hash(&account1, zero_amount);
		let leaf2 = calculate_leaf_hash(&3, 1000);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		let proof = bounded_proof(vec![leaf2]);

		// Zero amount claim will either:
		// 1. Fail at vesting creation (expected)
		// 2. Fail at proof verification (if amount affects hash)
		// 3. Succeed but create meaningless vesting (edge case)
		let result = MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, zero_amount, proof);

		// We expect this to fail somehow (either InvalidProof or vesting error)
		assert!(result.is_err(), "Zero amount claim should not succeed");
	});
}

#[test]
fn last_claim_exactly_zeroes_balance() {
	// EDGE CASE: Last user claims exactly remaining balance
	// No dust should remain
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let account2: u64 = 3;
		let amount1: u64 = 700;
		let amount2: u64 = 300;

		let leaf1 = calculate_leaf_hash(&account1, amount1);
		let leaf2 = calculate_leaf_hash(&account2, amount2);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));

		// Fund exactly the sum of allocations
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount1 + amount2));

		// First claim
		let proof1 = bounded_proof(vec![leaf2]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount1, proof1));

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, amount2);

		// Last claim should zero the balance
		let proof2 = bounded_proof(vec![leaf1]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account2, amount2, proof2));

		// Balance should be EXACTLY zero (no dust)
		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 0);
	});
}

#[test]
fn can_fund_after_partial_claims() {
	// SCENARIO: Airdrop is partially claimed, then more funds added
	// New claims should work with updated balance
	new_test_ext().execute_with(|| {
		let account1: u64 = 2;
		let account2: u64 = 3;
		let amount_each: u64 = 1000;

		let leaf1 = calculate_leaf_hash(&account1, amount_each);
		let leaf2 = calculate_leaf_hash(&account2, amount_each);
		let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

		assert_ok!(MerkleAirdrop::create_airdrop(
			RuntimeOrigin::signed(1),
			merkle_root,
			None,
			None
		));

		// Initial funding (only 1000)
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		// First claim - succeeds
		let proof1 = bounded_proof(vec![leaf2]);
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account1, amount_each, proof1));

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 0);

		// Second claim would fail due to insufficient balance
		let proof2 = bounded_proof(vec![leaf1]);
		assert_noop!(
			MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account2, amount_each, proof2.clone()),
			Error::<Test>::InsufficientAirdropBalance
		);

		// Top up the airdrop
		assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

		// Now second claim should succeed
		assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, account2, amount_each, proof2));

		assert_eq!(MerkleAirdrop::airdrop_info(0).unwrap().balance, 0);
	});
}
