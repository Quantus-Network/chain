#![cfg(test)]

use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use codec::Encode;

#[test]
fn create_airdrop_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let merkle_root = [0u8; 32];
        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));

        // Check that the event was emitted
        System::assert_last_event(Event::AirdropCreated {
            airdrop_id: 0,
            merkle_root,
        }.into());

        // Check that the airdrop was created
        assert_eq!(MerkleAirdrop::airdrop_merkle_roots(0), Some(merkle_root));
    });
}

#[test]
fn fund_airdrop_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let merkle_root = [0u8; 32];
        let amount = 1000;
        
        // Create an airdrop first
        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));

        // Fund the airdrop
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

        // Check that the event was emitted
        System::assert_last_event(Event::AirdropFunded {
            airdrop_id: 0,
            amount,
        }.into());

        // Check that the airdrop balance was updated
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(amount));
        
        // Check that the balance was transferred
        assert_eq!(Balances::free_balance(1), 9000); // Initial balance was 10000
        assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), amount);
    });
}

#[test]
fn claim_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let merkle_root = [0u8; 32];
        let amount = 1000;

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

        // Create a merkle proof
        let merkle_proof = vec![[0u8; 32]];

        // Claim tokens
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::signed(2), 0, 500, merkle_proof.clone()));

        // Check that the event was emitted
        System::assert_last_event(Event::Claimed {
            airdrop_id: 0,
            account: 2,
            amount: 500,
        }.into());

        // Check that the claim was recorded
        assert_eq!(MerkleAirdrop::is_claimed(0, 2), true);
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(500));
    });
}

#[test]
fn create_airdrop_requires_signed_origin() {
    new_test_ext().execute_with(|| {
        let merkle_root = [0u8; 32];

        // Try to create an airdrop with an unsigned origin
        assert_noop!(
            MerkleAirdrop::create_airdrop(RuntimeOrigin::none(), merkle_root),
            frame_support::error::BadOrigin
        );
    });
}

#[test]
fn fund_airdrop_fails_for_nonexistent_airdrop() {
    new_test_ext().execute_with(|| {
        // Try to fund a nonexistent airdrop
        assert_noop!(
            MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 999, 1000),
            Error::<Test>::AirdropNotFound
        );
    });
}

#[test]
fn claim_fails_for_nonexistent_airdrop() {
    new_test_ext().execute_with(|| {
        let merkle_proof = vec![[0u8; 32]];

        // Try to claim from a nonexistent airdrop
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::signed(1), 999, 500, merkle_proof),
            Error::<Test>::AirdropNotFound
        );
    });
}

#[test]
fn claim_already_claimed() {
    new_test_ext().execute_with(|| {
        let merkle_root = [0u8; 32];
        let amount = 1000;

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

        // Create a merkle proof
        let merkle_proof = vec![[0u8; 32]];

        // Claim tokens
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::signed(2), 0, 500, merkle_proof.clone()));

        // Try to claim again
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::signed(2), 0, 500, merkle_proof.clone()),
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
        
        // Test with invalid amount
        assert!(
            !MerkleAirdrop::verify_merkle_proof(
                &account1,
                400, // Wrong amount
                &merkle_root,
                &proof_for_account1
            ),
            "Proof with wrong amount should be invalid"
        );
        
        // Test with invalid proof
        let wrong_proof = vec![[1u8; 32]];
        assert!(
            !MerkleAirdrop::verify_merkle_proof(
                &account1,
                amount1,
                &merkle_root,
                &wrong_proof
            ),
            "Wrong proof should be invalid"
        );
        
        // Test with wrong account
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

// Helper function to calculate a leaf hash (account + amount)
fn calculate_leaf_hash(account: &u64, amount: u64) -> [u8; 32] {
    let account_bytes = account.encode();
    let amount_bytes = amount.encode();
    
    let combined = [&account_bytes[..], &amount_bytes[..]].concat();
    let hash = sp_core::blake2_256(&combined);
    
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}

// Helper function to calculate a parent hash from two child hashes
fn calculate_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    // Sort the hashes to ensure consistent ordering
    let combined = if left < right {
        [&left[..], &right[..]].concat()
    } else {
        [&right[..], &left[..]].concat()
    };
    
    let hash = sp_core::blake2_256(&combined);
    
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}