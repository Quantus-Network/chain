//! Unit tests for pallet-multisig

use crate::{mock::*, Error, Event, GlobalNonce, Multisigs};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use sp_core::crypto::AccountId32;

/// Helper function to get Alice's account ID
fn alice() -> AccountId32 {
	account_id(1)
}

/// Helper function to get Bob's account ID
fn bob() -> AccountId32 {
	account_id(2)
}

/// Helper function to get Charlie's account ID
fn charlie() -> AccountId32 {
	account_id(3)
}

/// Helper function to get Dave's account ID
fn dave() -> AccountId32 {
	account_id(4)
}

/// Helper function to create a simple encoded call
fn make_call(remark: Vec<u8>) -> Vec<u8> {
	let call = RuntimeCall::System(frame_system::Call::remark { remark });
	call.encode()
}

/// Helper function to get the ID of the last proposal created
/// Returns the current proposal_nonce - 1 (last used ID)
fn get_last_proposal_id(multisig_address: &AccountId32) -> u32 {
	let multisig = Multisigs::<Test>::get(multisig_address).expect("Multisig should exist");
	multisig.proposal_nonce.saturating_sub(1)
}

// ==================== MULTISIG CREATION TESTS ====================

#[test]
fn create_multisig_works() {
	new_test_ext().execute_with(|| {
		// Initialize block number for events
		System::set_block_number(1);

		// Setup
		let creator = alice();
		let signers = vec![bob(), charlie(), dave()];
		let threshold = 2;

		// Get initial balance
		let initial_balance = Balances::free_balance(creator.clone());
		let fee = 1000; // MultisigFeeParam
		let deposit = 500; // MultisigDepositParam

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
		));

		// Check balances
		// Deposit is reserved, fee is burned
		assert_eq!(Balances::reserved_balance(creator.clone()), deposit);
		assert_eq!(Balances::free_balance(creator.clone()), initial_balance - fee - deposit);

		// Check that multisig was created
		let global_nonce = GlobalNonce::<Test>::get();
		assert_eq!(global_nonce, 1);

		// Get multisig address
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Check storage
		let multisig_data = Multisigs::<Test>::get(&multisig_address).unwrap();
		assert_eq!(multisig_data.threshold, threshold);
		assert_eq!(multisig_data.nonce, 0);
		assert_eq!(multisig_data.signers.to_vec(), signers);
		assert_eq!(multisig_data.active_proposals, 0);
		assert_eq!(multisig_data.creator, creator.clone());
		assert_eq!(multisig_data.deposit, deposit);

		// Check that event was emitted
		System::assert_last_event(
			Event::MultisigCreated { creator, multisig_address, signers, threshold, nonce: 0 }
				.into(),
		);
	});
}

#[test]
fn create_multisig_fails_with_threshold_zero() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), charlie()];
		let threshold = 0;

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator.clone()), signers, threshold,),
			Error::<Test>::ThresholdZero
		);
	});
}

#[test]
fn create_multisig_fails_with_empty_signers() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![];
		let threshold = 1;

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator.clone()), signers, threshold,),
			Error::<Test>::NotEnoughSigners
		);
	});
}

#[test]
fn create_multisig_fails_with_threshold_too_high() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), charlie()];
		let threshold = 3; // More than number of signers

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator.clone()), signers, threshold,),
			Error::<Test>::ThresholdTooHigh
		);
	});
}

#[test]
fn create_multisig_fails_with_duplicate_signers() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), bob(), charlie()]; // Bob twice
		let threshold = 2;

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator.clone()), signers, threshold,),
			Error::<Test>::DuplicateSigner
		);
	});
}

#[test]
fn create_multiple_multisigs_increments_nonce() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers1 = vec![bob(), charlie()];
		let signers2 = vec![bob(), dave()];

		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers1.clone(),
			2
		));
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers2.clone(),
			2
		));

		// Check both multisigs exist
		let multisig1 = Multisig::derive_multisig_address(&signers1, 0);
		let multisig2 = Multisig::derive_multisig_address(&signers2, 1);

		assert!(Multisigs::<Test>::contains_key(multisig1));
		assert!(Multisigs::<Test>::contains_key(multisig2));
	});
}

// ==================== PROPOSAL CREATION TESTS ====================

#[test]
fn propose_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Propose a transaction
		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;

		let initial_balance = Balances::free_balance(proposer.clone());
		let proposal_deposit = 100; // ProposalDepositParam (Changed in mock)
							  // Fee calculation: Base(1000) + (Base(1000) * 1% * 2 signers) = 1000 + 20 = 1020
		let proposal_fee = 1020;

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address.clone(),
			call.clone(),
			expiry
		));

		// Check balances - deposit reserved, fee sent to treasury
		assert_eq!(Balances::reserved_balance(proposer.clone()), proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer.clone()),
			initial_balance - proposal_deposit - proposal_fee
		);
		// Fee is burned (reduces total issuance)

		// Check event
		let proposal_id = get_last_proposal_id(&multisig_address);
		System::assert_last_event(
			Event::ProposalCreated { multisig_address, proposer, proposal_id }.into(),
		);
	});
}

#[test]
fn propose_fails_if_not_signer() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to propose as non-signer
		let call = make_call(vec![1, 2, 3]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(dave()), multisig_address.clone(), call, 1000),
			Error::<Test>::NotASigner
		);
	});
}

// ==================== APPROVAL TESTS ====================

#[test]
fn approve_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie(), dave()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			3
		)); // Need 3 approvals

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Charlie approves (now 2/3)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Check event
		System::assert_last_event(
			Event::ProposalApproved {
				multisig_address: multisig_address.clone(),
				approver: charlie(),
				proposal_id,
				approvals_count: 2,
			}
			.into(),
		);

		// Proposal should still exist (not executed yet)
		assert!(crate::Proposals::<Test>::contains_key(&multisig_address, proposal_id));
	});
}

#[test]
fn approve_auto_executes_when_threshold_reached() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Charlie approves - threshold reached (2/2), auto-executes and removes
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Check that proposal was executed and immediately removed from storage
		assert!(crate::Proposals::<Test>::get(&multisig_address, proposal_id).is_none());

		// Deposit should be returned immediately
		assert_eq!(Balances::reserved_balance(bob()), 0); // No longer reserved

		// Check event was emitted
		System::assert_has_event(
			Event::ProposalExecuted {
				multisig_address,
				proposal_id,
				proposer: bob(),
				call: call.clone(),
				approvers: vec![bob(), charlie()],
				result: Ok(()),
			}
			.into(),
		);
	});
}

// ==================== CANCELLATION TESTS ====================

#[test]
fn cancel_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Cancel the proposal - immediately removes and returns deposit
		assert_ok!(Multisig::cancel(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be immediately removed from storage
		assert!(crate::Proposals::<Test>::get(&multisig_address, proposal_id).is_none());

		// Deposit should be returned immediately
		assert_eq!(Balances::reserved_balance(proposer.clone()), 0);

		// Check event
		System::assert_last_event(
			Event::ProposalCancelled { multisig_address, proposer, proposal_id }.into(),
		);
	});
}

#[test]
fn cancel_fails_if_already_executed() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Approve to execute (auto-executes and removes proposal)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Try to cancel executed proposal (already removed, so ProposalNotFound)
		assert_noop!(
			Multisig::cancel(RuntimeOrigin::signed(bob()), multisig_address.clone(), proposal_id),
			Error::<Test>::ProposalNotFound
		);
	});
}

// ==================== DEPOSIT RECOVERY TESTS ====================

#[test]
fn remove_expired_works_after_grace_period() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			expiry
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Move past expiry + grace period (100 blocks)
		System::set_block_number(expiry + 101);

		// Any signer can remove after grace period (charlie is a signer)
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be gone
		assert!(!crate::Proposals::<Test>::contains_key(&multisig_address, proposal_id));

		// Deposit should be returned to proposer
		assert_eq!(Balances::reserved_balance(bob()), 0);
	});
}

#[test]
fn executed_proposals_auto_removed() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Execute - should auto-remove proposal and return deposit
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be immediately removed
		assert!(crate::Proposals::<Test>::get(&multisig_address, proposal_id).is_none());

		// Deposit should be immediately returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Trying to remove again should fail (already removed)
		assert_noop!(
			Multisig::remove_expired(
				RuntimeOrigin::signed(charlie()),
				multisig_address.clone(),
				proposal_id
			),
			Error::<Test>::ProposalNotFound
		);
	});
}

#[test]
fn remove_expired_fails_for_non_signer() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			expiry
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Move past expiry
		System::set_block_number(expiry + 1);

		// Dave is not a signer, should fail
		assert_noop!(
			Multisig::remove_expired(
				RuntimeOrigin::signed(dave()),
				multisig_address.clone(),
				proposal_id
			),
			Error::<Test>::NotASigner
		);

		// But charlie (who is a signer) can do it
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));
	});
}

#[test]
fn claim_deposits_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Bob creates 3 proposals
		for i in 0..3 {
			let call = make_call(vec![i as u8; 32]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call,
				100
			));
		}

		// All reserved
		assert_eq!(Balances::reserved_balance(bob()), 300); // 3 * 100

		// Move past expiry + grace period
		System::set_block_number(201);

		// Bob claims all deposits at once
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone()
		));

		// All deposits returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Check event
		System::assert_has_event(
			Event::DepositsClaimed {
				multisig_address,
				claimer: bob(),
				total_returned: 300,
				proposals_removed: 3,
				multisig_removed: false,
			}
			.into(),
		);
	});
}

// ==================== HELPER FUNCTION TESTS ====================

#[test]
fn derive_multisig_address_is_deterministic() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie(), dave()];
		let nonce = 42;

		let address1 = Multisig::derive_multisig_address(&signers, nonce);
		let address2 = Multisig::derive_multisig_address(&signers, nonce);

		assert_eq!(address1, address2);
	});
}

#[test]
fn derive_multisig_address_different_for_different_nonce() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie(), dave()];

		let address1 = Multisig::derive_multisig_address(&signers, 0);
		let address2 = Multisig::derive_multisig_address(&signers, 1);

		assert_ne!(address1, address2);
	});
}

#[test]
fn is_signer_works() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(alice()), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		assert!(Multisig::is_signer(&multisig_address, &bob()));
		assert!(Multisig::is_signer(&multisig_address, &charlie()));
		assert!(!Multisig::is_signer(&multisig_address, &dave()));
	});
}

#[test]
fn too_many_proposals_in_storage_fails() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// MaxTotal = 20, 2 signers = 10 each
		// Executed/Cancelled proposals are auto-removed, so only Active count toward storage
		// Create 10 active proposals from Bob
		for i in 0..10 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				1000
			));
		}
		// Bob has 10 active = 10 total (at per-signer limit)

		// Create 10 active proposals from Charlie
		for i in 10..20 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(charlie()),
				multisig_address.clone(),
				call.clone(),
				1000
			));
		}
		// Charlie has 10 active = 10 total (at per-signer limit)
		// Total: 20 active (AT LIMIT)

		// Try to add 21st - should fail on total limit
		let call = make_call(vec![99]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address.clone(), call, 2000),
			Error::<Test>::TooManyProposalsInStorage
		);
	});
}

#[test]
fn only_active_proposals_remain_in_storage() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Test that only Active proposals remain in storage (Executed/Cancelled auto-removed)

		// Bob creates 10, executes 5, cancels 1 - only 4 active remain
		for i in 0..10 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				1000
			));

			if i < 5 {
				let id = get_last_proposal_id(&multisig_address);
				assert_ok!(Multisig::approve(
					RuntimeOrigin::signed(charlie()),
					multisig_address.clone(),
					id
				));
			} else if i == 5 {
				let id = get_last_proposal_id(&multisig_address);
				assert_ok!(Multisig::cancel(
					RuntimeOrigin::signed(bob()),
					multisig_address.clone(),
					id
				));
			}
		}
		// Bob now has 4 Active in storage (i=6,7,8,9), 5 executed + 1 cancelled were removed

		// Bob can create 6 more to reach his per-signer limit (10)
		for i in 10..16 {
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![i]),
				2000
			));
		}
		// Bob: 10 Active (at per-signer limit)

		// Bob cannot create 11th
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				3000
			),
			Error::<Test>::TooManyProposalsPerSigner
		);
	});
}

#[test]
fn auto_cleanup_allows_new_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Bob creates 10 proposals, all expire at block 100 (at per-signer limit)
		for i in 0..10 {
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![i]),
				100
			));
		}
		// Bob: 10 Active (at per-signer limit)

		// Bob cannot create more (at limit)
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				200
			),
			Error::<Test>::TooManyProposalsPerSigner
		);

		// Move past expiry
		System::set_block_number(101);

		// Now Bob can create new - propose() auto-cleans expired
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			make_call(vec![99]),
			200
		));

		// Verify old proposals were removed
		let count = crate::Proposals::<Test>::iter_prefix(&multisig_address).count();
		assert_eq!(count, 1); // Only the new one remains
	});
}

#[test]
fn propose_fails_with_expiry_in_past() {
	new_test_ext().execute_with(|| {
		System::set_block_number(100);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);

		// Try to create proposal with expiry in the past (< current_block)
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				50
			),
			Error::<Test>::ExpiryInPast
		);

		// Try with expiry equal to current block (not > current_block)
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				100
			),
			Error::<Test>::ExpiryInPast
		);

		// Valid: expiry in the future
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call,
			101
		));
	});
}

#[test]
fn propose_fails_with_expiry_too_far() {
	new_test_ext().execute_with(|| {
		System::set_block_number(100);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);

		// MaxExpiryDurationParam = 10000 blocks (from mock.rs)
		// Current block = 100
		// Max allowed expiry = 100 + 10000 = 10100

		// Try to create proposal with expiry too far in the future
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				10101
			),
			Error::<Test>::ExpiryTooFar
		);

		// Try with expiry way beyond the limit
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				20000
			),
			Error::<Test>::ExpiryTooFar
		);

		// Valid: expiry exactly at max allowed
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			10100
		));

		// Move to next block and try again
		System::set_block_number(101);
		// Now max allowed = 101 + 10000 = 10101
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call,
			10101
		));
	});
}

#[test]
fn propose_charges_correct_fee_with_signer_factor() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		// 3 Signers: Bob, Charlie, Dave
		let signers = vec![bob(), charlie(), dave()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let initial_balance = Balances::free_balance(proposer.clone());

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address,
			call,
			1000
		));

		// ProposalFeeParam = 1000
		// SignerStepFactor = 1%
		// Signers = 3
		// Calculation: 1000 + (1000 * 1% * 3) = 1000 + 30 = 1030
		let expected_fee = 1030;
		let deposit = 100; // ProposalDepositParam

		assert_eq!(
			Balances::free_balance(proposer.clone()),
			initial_balance - deposit - expected_fee
		);
		// Fee is burned (reduces total issuance)
	});
}

#[test]
fn dissolve_multisig_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let creator = alice();
		let signers = vec![bob(), charlie()];
		let deposit = 500;
		let fee = 1000;
		let initial_balance = Balances::free_balance(creator.clone());

		// Create
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		assert_eq!(Balances::reserved_balance(creator.clone()), deposit);

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to dissolve immediately (success)
		assert_ok!(Multisig::dissolve_multisig(
			RuntimeOrigin::signed(creator.clone()),
			multisig_address.clone()
		));

		// Check cleanup
		assert!(!Multisigs::<Test>::contains_key(&multisig_address));
		assert_eq!(Balances::reserved_balance(creator.clone()), 0);
		// Balance returned (minus burned fee)
		assert_eq!(Balances::free_balance(creator.clone()), initial_balance - fee);
	});
}

#[test]
fn dissolve_multisig_fails_with_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Create proposal
		let call = make_call(vec![1]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call,
			100
		));

		// Try to dissolve
		assert_noop!(
			Multisig::dissolve_multisig(
				RuntimeOrigin::signed(creator.clone()),
				multisig_address.clone()
			),
			Error::<Test>::ProposalsExist
		);
	});
}

#[test]
fn per_signer_proposal_limit_enforced() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// MaxActiveProposals = 10, MaxTotalProposalsInStorage = 20
		// With 2 signers, each can have max 20/2 = 10 proposals
		// Only Active proposals count (Executed/Cancelled auto-removed)

		// Bob creates 10 active proposals (at per-signer limit)
		for i in 0..10 {
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![i]),
				1000
			));
		}

		// Bob at limit - tries to create 11th
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				2000
			),
			Error::<Test>::TooManyProposalsPerSigner
		);

		// But Charlie can still create (independent limit)
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			make_call(vec![100]),
			2000
		));
	});
}
