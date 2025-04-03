#![cfg(test)]

use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};

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