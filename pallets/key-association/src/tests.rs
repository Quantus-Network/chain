//! Unit tests for pallet-key-association.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{mock::*, Error, Event, KeyIndex, Associations, ClassicalKey, ClassicalSignature, KeyType};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use sp_core::{ecdsa, ed25519, Pair, H256};

/// Build the challenge message (mirrors the pallet's internal function).
fn build_challenge_message(
	account: &AccountId,
	classical_key: &ClassicalKey,
	block_hash: &<Test as frame_system::Config>::Hash,
) -> Vec<u8> {
	let mut msg = b"Quantus Key Association\n".to_vec();
	msg.extend_from_slice(b"Account: ");
	msg.extend_from_slice(&account.encode());
	msg.extend_from_slice(b"\nKey: ");
	msg.extend_from_slice(&classical_key.encode());
	msg.extend_from_slice(b"\nBlock: ");
	msg.extend_from_slice(block_hash.as_ref());
	msg
}

/// Set up test environment with a known block hash at block 5.
/// Returns the block hash that can be used in tests.
fn setup_test_blocks() -> H256 {
	// Set current block to 10 so block 5 is within the validity window
	System::set_block_number(10);
	
	// Manually insert a known block hash at block 5
	let test_block_hash = H256::from([0x42u8; 32]);
	frame_system::BlockHash::<Test>::insert(5u64, test_block_hash);
	
	test_block_hash
}

// ==================== ECDSA TESTS ====================

#[test]
fn associate_ecdsa_key_works() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);
		
		// Generate ECDSA keypair
		let ecdsa_pair = ecdsa::Pair::generate().0;
		let ecdsa_public = ecdsa_pair.public();
		let classical_key = ClassicalKey::Ecdsa(ecdsa_public);

		// Build and sign the challenge message
		let message = build_challenge_message(&quantus_account, &classical_key, &block_hash);
		let signature = ecdsa_pair.sign(&message);
		let classical_sig = ClassicalSignature::Ecdsa(signature);

		// Associate the key
		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(quantus_account.clone()),
			classical_key.clone(),
			classical_sig,
			block_number,
			block_hash,
		));

		// Verify storage
		let associations = Associations::<Test>::get(&quantus_account);
		assert_eq!(associations.len(), 1);
		assert_eq!(associations[0].0, classical_key);
		assert_eq!(associations[0].1.created_at, 10); // Current block

		// Verify reverse index
		let key_hash = KeyAssociation::compute_key_hash(&classical_key);
		assert_eq!(KeyIndex::<Test>::get(key_hash), Some(quantus_account.clone()));

		// Verify event
		System::assert_last_event(
			Event::KeyAssociated {
				account: quantus_account,
				key_type: KeyType::Ecdsa,
				key_hash,
			}
			.into(),
		);
	});
}

// ==================== ED25519 TESTS ====================

#[test]
fn associate_ed25519_key_works() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(2);
		
		// Generate Ed25519 keypair
		let ed_pair = ed25519::Pair::generate().0;
		let ed_public = ed_pair.public();
		let classical_key = ClassicalKey::Ed25519(ed_public);

		let message = build_challenge_message(&quantus_account, &classical_key, &block_hash);
		let signature = ed_pair.sign(&message);
		let classical_sig = ClassicalSignature::Ed25519(signature);

		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(quantus_account.clone()),
			classical_key.clone(),
			classical_sig,
			block_number,
			block_hash,
		));

		// Verify storage
		let associations = Associations::<Test>::get(&quantus_account);
		assert_eq!(associations.len(), 1);
		assert_eq!(associations[0].0, classical_key);

		// Verify event
		let key_hash = KeyAssociation::compute_key_hash(&classical_key);
		System::assert_last_event(
			Event::KeyAssociated {
				account: quantus_account,
				key_type: KeyType::Ed25519,
				key_hash,
			}
			.into(),
		);
	});
}

// ==================== MULTIPLE KEYS TESTS ====================

#[test]
fn associate_multiple_keys_works() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);

		// Associate an ECDSA key
		let ecdsa_pair = ecdsa::Pair::generate().0;
		let ecdsa_key = ClassicalKey::Ecdsa(ecdsa_pair.public());
		let msg1 = build_challenge_message(&quantus_account, &ecdsa_key, &block_hash);
		let sig1 = ClassicalSignature::Ecdsa(ecdsa_pair.sign(&msg1));

		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(quantus_account.clone()),
			ecdsa_key.clone(),
			sig1,
			block_number,
			block_hash,
		));

		// Associate an Ed25519 key
		let ed_pair = ed25519::Pair::generate().0;
		let ed_key = ClassicalKey::Ed25519(ed_pair.public());
		let msg2 = build_challenge_message(&quantus_account, &ed_key, &block_hash);
		let sig2 = ClassicalSignature::Ed25519(ed_pair.sign(&msg2));

		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(quantus_account.clone()),
			ed_key.clone(),
			sig2,
			block_number,
			block_hash,
		));

		// Verify both are stored
		let associations = Associations::<Test>::get(&quantus_account);
		assert_eq!(associations.len(), 2);
	});
}

#[test]
fn max_associations_enforced() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);

		// Associate MaxAssociations (8) keys
		for _ in 0..8u8 {
			// Use generate() with a seed to get deterministic but unique keys
			let pair = ecdsa::Pair::generate().0;
			let key = ClassicalKey::Ecdsa(pair.public());
			let msg = build_challenge_message(&quantus_account, &key, &block_hash);
			let sig = ClassicalSignature::Ecdsa(pair.sign(&msg));

			assert_ok!(KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account.clone()),
				key,
				sig,
				block_number,
				block_hash,
			));
		}

		// 9th key should fail
		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let msg = build_challenge_message(&quantus_account, &key, &block_hash);
		let sig = ClassicalSignature::Ecdsa(pair.sign(&msg));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account),
				key,
				sig,
				block_number,
				block_hash,
			),
			Error::<Test>::TooManyAssociations
		);
	});
}

// ==================== ERROR CASES ====================

#[test]
fn invalid_signature_rejected() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);

		// Generate key but sign wrong message
		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let wrong_message = b"wrong message";
		let sig = ClassicalSignature::Ecdsa(pair.sign(wrong_message));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account),
				key,
				sig,
				block_number,
				block_hash,
			),
			Error::<Test>::InvalidSignature
		);
	});
}

#[test]
fn signature_key_mismatch_rejected() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);

		// ECDSA key with Ed25519 signature
		let ecdsa_pair = ecdsa::Pair::generate().0;
		let ecdsa_key = ClassicalKey::Ecdsa(ecdsa_pair.public());

		let ed_pair = ed25519::Pair::generate().0;
		let msg = build_challenge_message(&quantus_account, &ecdsa_key, &block_hash);
		let wrong_sig = ClassicalSignature::Ed25519(ed_pair.sign(&msg));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account),
				ecdsa_key,
				wrong_sig,
				block_number,
				block_hash,
			),
			Error::<Test>::SignatureKeyMismatch
		);
	});
}

#[test]
fn key_already_associated_rejected() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let account1 = account_id(1);
		let account2 = account_id(2);

		// Account 1 associates a key
		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let msg1 = build_challenge_message(&account1, &key, &block_hash);
		let sig1 = ClassicalSignature::Ecdsa(pair.sign(&msg1));

		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(account1),
			key.clone(),
			sig1,
			block_number,
			block_hash,
		));

		// Account 2 tries to associate the same key
		let msg2 = build_challenge_message(&account2, &key, &block_hash);
		let sig2 = ClassicalSignature::Ecdsa(pair.sign(&msg2));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(account2),
				key,
				sig2,
				block_number,
				block_hash,
			),
			Error::<Test>::KeyAlreadyAssociated
		);
	});
}

#[test]
fn block_hash_mismatch_rejected() {
	new_test_ext().execute_with(|| {
		let _correct_hash = setup_test_blocks();
		let block_number = 5u64;
		
		// Use a fake block hash that doesn't match block 5
		let fake_block_hash = H256::from([0xffu8; 32]);

		let quantus_account = account_id(1);

		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let msg = build_challenge_message(&quantus_account, &key, &fake_block_hash);
		let sig = ClassicalSignature::Ecdsa(pair.sign(&msg));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account),
				key,
				sig,
				block_number,
				fake_block_hash,
			),
			Error::<Test>::BlockHashMismatch
		);
	});
}

#[test]
fn signature_expired_rejected() {
	new_test_ext().execute_with(|| {
		// Set up block hash at block 5, but set current block very far ahead
		let test_block_hash = H256::from([0x42u8; 32]);
		frame_system::BlockHash::<Test>::insert(5u64, test_block_hash);
		
		// Set current block to 1000 (way past the 256 block validity window)
		System::set_block_number(1000);

		let quantus_account = account_id(1);

		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let msg = build_challenge_message(&quantus_account, &key, &test_block_hash);
		let sig = ClassicalSignature::Ecdsa(pair.sign(&msg));

		assert_noop!(
			KeyAssociation::associate(
				RuntimeOrigin::signed(quantus_account),
				key,
				sig,
				5u64,
				test_block_hash,
			),
			Error::<Test>::SignatureExpired
		);
	});
}

// ==================== READ API TESTS ====================

#[test]
fn read_apis_work() {
	new_test_ext().execute_with(|| {
		let block_hash = setup_test_blocks();
		let block_number = 5u64;

		let quantus_account = account_id(1);

		let pair = ecdsa::Pair::generate().0;
		let key = ClassicalKey::Ecdsa(pair.public());
		let msg = build_challenge_message(&quantus_account, &key, &block_hash);
		let sig = ClassicalSignature::Ecdsa(pair.sign(&msg));

		// Before association
		assert!(!KeyAssociation::is_key_associated(&key));
		assert_eq!(KeyAssociation::account_for_key(&key), None);
		assert!(KeyAssociation::associations_for(&quantus_account).is_empty());

		// Associate
		assert_ok!(KeyAssociation::associate(
			RuntimeOrigin::signed(quantus_account.clone()),
			key.clone(),
			sig,
			block_number,
			block_hash,
		));

		// After association
		assert!(KeyAssociation::is_key_associated(&key));
		assert_eq!(KeyAssociation::account_for_key(&key), Some(quantus_account.clone()));
		
		let associations = KeyAssociation::associations_for(&quantus_account);
		assert_eq!(associations.len(), 1);
		assert_eq!(associations[0].0, key);
	});
}
