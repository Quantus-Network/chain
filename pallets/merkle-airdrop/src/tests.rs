#![cfg(test)]

use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use codec::Encode;
use poseidon_resonance::PoseidonHasher;
use sp_core::blake2_256;
use sp_runtime::traits::Hash;
use sp_core::crypto::AccountId32;
use sp_core::crypto::Ss58Codec;

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

        // Initialize balances
        initialize_balances();

        let merkle_root = [0u8; 32];
        let amount = 100;

        // Create an airdrop first
        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));

        // Check initial balance - it might be Some(0) instead of None
        let initial_balance = MerkleAirdrop::airdrop_balances(0);
        assert!(initial_balance.is_none() || initial_balance == Some(0));

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
        assert_eq!(Balances::free_balance(1), 9900); // 10000 - 100
        assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 101); // 1 (initial) + 100 (funded)

        // Fund the airdrop again
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, amount));

        // Check that the balance was updated correctly
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(amount * 2));
        assert_eq!(Balances::free_balance(1), 9800); // 9900 - 100
        assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 201); // 101 + 100
    });
}

#[test]
fn claim_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        // Initialize balances
        initialize_balances();

        // Create a test merkle tree with two accounts
        let account1: u64 = 2; // Account that will claim
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;

        // Calculate leaf hashes
        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);

        // Calculate the Merkle root (hash of the two leaves)
        let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

        // Create the airdrop with our calculated root
        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

        // Create proof for account1
        let merkle_proof = vec![leaf2];

        // Claim tokens
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, merkle_proof.clone()));

        // Check that the event was emitted
        System::assert_last_event(Event::Claimed {
            airdrop_id: 0,
            account: 2,
            amount: 500,
        }.into());

        // Check that the claim was recorded
        assert_eq!(MerkleAirdrop::is_claimed(0, 2), true);
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(500)); // 1000 - 500

        // Check balances
        assert_eq!(Balances::free_balance(2), 500);
        assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), 501); // 1 (initial) + 1000 (funded) - 500 (claimed)
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
            MerkleAirdrop::claim(RuntimeOrigin::none(), 999, 500, merkle_proof),
            Error::<Test>::AirdropNotFound
        );
    });
}

#[test]
fn claim_already_claimed() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        // Initialize balances
        initialize_balances();

        // Create a test merkle tree with two accounts
        let account1: u64 = 2; // Account that will claim
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;

        // Calculate leaf hashes
        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);

        // Calculate the Merkle root (hash of the two leaves)
        let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

        // Create the airdrop with our calculated root
        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

        // Create proof for account1
        let merkle_proof = vec![leaf2];

        // Claim tokens
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, merkle_proof.clone()));

        // Try to claim again
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, merkle_proof.clone()),
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

// Helper function to convert hex string to bytes
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim_start_matches("0x");
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i+2], 16).expect("Invalid hex string");
        bytes.push(byte);
    }
    bytes
}

// #[test]
// fn verify_merkle_proof_consistency() {
//     new_test_ext().execute_with(|| {
//         // Create test accounts and amounts from sample-claims.json
//         let account1 = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
//         let amount1 = 1000000000000u64;
//         let account2 = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";
//         let amount2 = 2000000000000u64;
//         let account3 = "5DAAnrj7VHTznn2AWBemMuyBwZWs6FNFjdyVXUeYum3PTXFy";
//         let amount3 = 3000000000000u64;
//
//         // Convert SS58 addresses to AccountId32 - this is how the CLI does it
//         let account_id1 = AccountId32::from_ss58check(account1).expect("Invalid SS58 address");
//         let account_id2 = AccountId32::from_ss58check(account2).expect("Invalid SS58 address");
//         let account_id3 = AccountId32::from_ss58check(account3).expect("Invalid SS58 address");
//
//         // Calculate leaf hashes
//         let leaf1 = MerkleAirdrop::calculate_leaf_hash_blake2(&account_id1, amount1);
//         let leaf2 = MerkleAirdrop::calculate_leaf_hash_blake2(&account_id2, amount2);
//         let leaf3 = MerkleAirdrop::calculate_leaf_hash_blake2(&account_id3, amount3);
//
//         // Build the tree as per the provided structure
//         // First level: leaf1 + leaf2, leaf3
//         let parent1 = calculate_parent_hash(&leaf1, &leaf2);
//
//         // Second level: parent1 + leaf3
//         let merkle_root = calculate_parent_hash(&parent1, &leaf3);
//
//         // Create proofs for each account
//         let proof1 = vec![leaf2, leaf3];
//         let proof2 = vec![leaf1, leaf3];
//         let proof3 = vec![parent1];
//
//         // Verify the proofs multiple times to ensure consistency
//         for i in 0..3 {
//             // Verify proof for account1
//             let is_valid1 = MerkleAirdrop::verify_merkle_proof(
//                 &account_id1,
//                 amount1,
//                 &merkle_root,
//                 &proof1
//             );
//
//             assert!(
//                 is_valid1,
//                 "Proof verification for account1 failed on attempt {}",
//                 i + 1
//             );
//
//             // Verify proof for account2
//             let is_valid2 = MerkleAirdrop::verify_merkle_proof(
//                 &account2,
//                 amount2,
//                 &merkle_root,
//                 &proof2
//             );
//
//             assert!(
//                 is_valid2,
//                 "Proof verification for account2 failed on attempt {}",
//                 i + 1
//             );
//
//             // Verify proof for account3
//             let is_valid3 = MerkleAirdrop::verify_merkle_proof(
//                 &account3,
//                 amount3,
//                 &merkle_root,
//                 &proof3
//             );
//
//             assert!(
//                 is_valid3,
//                 "Proof verification for account3 failed on attempt {}",
//                 i + 1
//             );
//         }
//
//         // Print debug information
//         println!("Account1: {}", account1);
//         println!("Amount1: {}", amount1);
//         println!("Leaf1: {:?}", leaf1);
//         println!("Proof1: {:?}", proof1);
//
//         println!("Account2: {}", account2);
//         println!("Amount2: {}", amount2);
//         println!("Leaf2: {:?}", leaf2);
//         println!("Proof2: {:?}", proof2);
//
//         println!("Account3: {}", account3);
//         println!("Amount3: {}", amount3);
//         println!("Leaf3: {:?}", leaf3);
//         println!("Proof3: {:?}", proof3);
//
//         println!("Merkle Root: {:?}", merkle_root);
//
//         // Compare with the provided Merkle root
//         let expected_root_hex = "0xd58b0382abf7d9c776870327a4ef5a1121c9f11aaab98a35ea290e273f484975";
//         let expected_root_bytes = hex_to_bytes(expected_root_hex);
//         let expected_root: [u8; 32] = expected_root_bytes.try_into().expect("Invalid length");
//
//         println!("Expected Root: {:?}", expected_root);
//         println!("Calculated Root: {:?}", merkle_root);
//
//         // Compare the calculated root with the expected root
//         assert_eq!(
//             merkle_root, expected_root,
//             "Calculated Merkle root does not match the expected root"
//         );
//
//         // Compare the calculated proofs with the provided proofs
//         let expected_proof1_hex = [
//             "0xc4c6de5a4da087ed4788df7c75be26be1773623f3ab04c9a7a64abf7286fcf0a",
//             "0x124e7cbacdc1247065d1046a6d1457372b7598977d62117bdf67672af4393926"
//         ];
//         let expected_proof2_hex = [
//             "0x3fdd468bf65171b1dd76c288b44bce6bcab66fcd70bac797d0c78f43b140750d",
//             "0x124e7cbacdc1247065d1046a6d1457372b7598977d62117bdf67672af4393926"
//         ];
//         let expected_proof3_hex = [
//             "0x3c8a274edb57c2a2588abf2543786f8f16f3d6171a997fd0b797748d67dfa1fc"
//         ];
//
//         let expected_proof1: Vec<[u8; 32]> = expected_proof1_hex.iter().map(|hex| {
//             let bytes = hex_to_bytes(hex);
//             bytes.try_into().expect("Invalid length")
//         }).collect();
//
//         let expected_proof2: Vec<[u8; 32]> = expected_proof2_hex.iter().map(|hex| {
//             let bytes = hex_to_bytes(hex);
//             bytes.try_into().expect("Invalid length")
//         }).collect();
//
//         let expected_proof3: Vec<[u8; 32]> = expected_proof3_hex.iter().map(|hex| {
//             let bytes = hex_to_bytes(hex);
//             bytes.try_into().expect("Invalid length")
//         }).collect();
//
//         println!("Expected Proof1: {:?}", expected_proof1);
//         println!("Calculated Proof1: {:?}", proof1);
//
//         println!("Expected Proof2: {:?}", expected_proof2);
//         println!("Calculated Proof2: {:?}", proof2);
//
//         println!("Expected Proof3: {:?}", expected_proof3);
//         println!("Calculated Proof3: {:?}", proof3);
//
//         // Compare the calculated proofs with the expected proofs
//         assert_eq!(
//             proof1, expected_proof1,
//             "Calculated proof1 does not match the expected proof1"
//         );
//
//         assert_eq!(
//             proof2, expected_proof2,
//             "Calculated proof2 does not match the expected proof2"
//         );
//
//         assert_eq!(
//             proof3, expected_proof3,
//             "Calculated proof3 does not match the expected proof3"
//         );
//     });
// }

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

    // Use PoseidonHasher
    let mut output = [0u8; 32];
    output.copy_from_slice(
        &PoseidonHasher::hash(&combined)[..]
    );
    output
}

#[test]
fn claim_invalid_proof_fails() {
    new_test_ext().execute_with(|| {
        initialize_balances();

        // Create a valid merkle tree
        let account1: u64 = 2;
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;

        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);
        let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

        // Create an invalid proof
        let invalid_proof = vec![[1u8; 32]]; // Different from the actual leaf2

        // Attempt to claim with invalid proof
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, invalid_proof),
            Error::<Test>::InvalidProof
        );
    });
}

#[test]
fn claim_insufficient_airdrop_balance_fails() {
    new_test_ext().execute_with(|| {
        initialize_balances();

        // Create a valid merkle tree
        let account1: u64 = 2;
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;

        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);
        let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 400)); // Fund less than claim amount

        // Create a valid proof
        let merkle_proof = vec![leaf2];

        // Attempt to claim more than available
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, merkle_proof),
            Error::<Test>::InsufficientAirdropBalance
        );
    });
}

#[test]
fn claim_nonexistent_airdrop_fails() {
    new_test_ext().execute_with(|| {
        initialize_balances();

        // Attempt to claim from a nonexistent airdrop
        assert_noop!(
            MerkleAirdrop::claim(RuntimeOrigin::none(), 999, 500, vec![[0u8; 32]]),
            Error::<Test>::AirdropNotFound
        );
    });
}

#[test]
fn claim_updates_balances_correctly() {
    new_test_ext().execute_with(|| {
        initialize_balances();

        // Create a valid merkle tree
        let account1: u64 = 2;
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;

        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);
        let merkle_root = calculate_parent_hash(&leaf1, &leaf2);

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

        // Initial balances
        let initial_account_balance = Balances::free_balance(2);
        let initial_pallet_balance = Balances::free_balance(MerkleAirdrop::account_id());

        // Claim tokens
        let merkle_proof = vec![leaf2];
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, merkle_proof));

        // Check balances after claim
        assert_eq!(Balances::free_balance(2), initial_account_balance + 500);
        assert_eq!(Balances::free_balance(MerkleAirdrop::account_id()), initial_pallet_balance - 500);

        // Check airdrop balance is updated
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(500)); // 1000 - 500

        // Check claim is recorded
        assert_eq!(MerkleAirdrop::is_claimed(0, 2), true);
    });
}

#[test]
fn multiple_users_can_claim() {
    new_test_ext().execute_with(|| {
        initialize_balances();

        // Create a valid merkle tree with 3 users
        let account1: u64 = 2;
        let amount1: u64 = 500;
        let account2: u64 = 3;
        let amount2: u64 = 300;
        let account3: u64 = 4;
        let amount3: u64 = 200;

        let leaf1 = calculate_leaf_hash(&account1, amount1);
        let leaf2 = calculate_leaf_hash(&account2, amount2);
        let leaf3 = calculate_leaf_hash(&account3, amount3);

        // Create a simple 3-leaf merkle tree
        let parent1 = calculate_parent_hash(&leaf1, &leaf2);
        let merkle_root = calculate_parent_hash(&parent1, &leaf3);

        assert_ok!(MerkleAirdrop::create_airdrop(RuntimeOrigin::signed(1), merkle_root));
        assert_ok!(MerkleAirdrop::fund_airdrop(RuntimeOrigin::signed(1), 0, 1000));

        // User 1 claims
        let proof1 = vec![leaf2, leaf3];
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 500, proof1));
        assert_eq!(Balances::free_balance(2), 500);

        // User 2 claims
        let proof2 = vec![leaf1, leaf3];
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 300, proof2));
        assert_eq!(Balances::free_balance(3), 300);

        // User 3 claims
        let proof3 = vec![parent1];
        assert_ok!(MerkleAirdrop::claim(RuntimeOrigin::none(), 0, 200, proof3));
        assert_eq!(Balances::free_balance(4), 200);

        // Check final airdrop balance
        assert_eq!(MerkleAirdrop::airdrop_balances(0), Some(0)); // 1000 - 500 - 300 - 200

        // Check all claims are recorded
        assert_eq!(MerkleAirdrop::is_claimed(0, 2), true);
        assert_eq!(MerkleAirdrop::is_claimed(0, 3), true);
        assert_eq!(MerkleAirdrop::is_claimed(0, 4), true);
    });
}