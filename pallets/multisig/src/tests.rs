//! Unit tests for pallet-multisig

use crate::{mock::*, Error, Event, GlobalNonce, Multisigs, ProposalStatus};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
/// Helper function to get Alice's account ID
fn alice() -> u64 {
	1
}

/// Helper function to get Bob's account ID
fn bob() -> u64 {
	2
}

/// Helper function to get Charlie's account ID
fn charlie() -> u64 {
	3
}

/// Helper function to get Dave's account ID
fn dave() -> u64 {
	4
}

/// Helper function to create a simple encoded call
fn make_call(remark: Vec<u8>) -> Vec<u8> {
	let call = RuntimeCall::System(frame_system::Call::remark { remark });
	call.encode()
}

/// Helper function to calculate proposal hash for testing
/// Note: This calculates hash for the LAST proposal (uses current proposal_nonce - 1)
/// because propose() increments nonce before calculating hash
fn calculate_last_proposal_hash(
	multisig_address: u64,
	call: &[u8],
) -> <Test as frame_system::Config>::Hash {
	let multisig = Multisigs::<Test>::get(multisig_address).expect("Multisig should exist");
	// The last proposal used (proposal_nonce - 1) because propose() increments it
	let nonce_used = multisig.proposal_nonce.saturating_sub(1);
	Multisig::calculate_proposal_hash(call, nonce_used)
}

/// Helper function to calculate proposal hash for a specific nonce
fn calculate_proposal_hash_with_nonce(
	call: &[u8],
	nonce: u32,
) -> <Test as frame_system::Config>::Hash {
	Multisig::calculate_proposal_hash(call, nonce)
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
		let initial_balance = Balances::free_balance(creator);
		let fee = 1000; // MultisigFeeParam
		let deposit = 500; // MultisigDepositParam

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator),
			signers.clone(),
			threshold,
		));

		// Check balances
		// Deposit is reserved, fee is burned
		assert_eq!(Balances::reserved_balance(creator), deposit);
		assert_eq!(Balances::free_balance(creator), initial_balance - fee - deposit);

		// Check that multisig was created
		let global_nonce = GlobalNonce::<Test>::get();
		assert_eq!(global_nonce, 1);

		// Get multisig address
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Check storage
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();
		assert_eq!(multisig_data.threshold, threshold);
		assert_eq!(multisig_data.nonce, 0);
		assert_eq!(multisig_data.signers.to_vec(), signers);
		assert_eq!(multisig_data.active_proposals, 0);
		assert_eq!(multisig_data.creator, creator);
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
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
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
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
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
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
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
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
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

		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers1.clone(), 2));
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers2.clone(), 2));

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
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Propose a transaction
		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;

		let initial_balance = Balances::free_balance(proposer);
		let proposal_deposit = 100; // ProposalDepositParam (Changed in mock)
							  // Fee calculation: Base(1000) + (Base(1000) * 1% * 2 signers) = 1000 + 20 = 1020
		let proposal_fee = 1020;

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			expiry
		));

		// Check balances - deposit reserved, fee sent to treasury
		assert_eq!(Balances::reserved_balance(proposer), proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - proposal_deposit - proposal_fee
		);
		// Fee is burned (reduces total issuance)

		// Check event
		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);
		System::assert_last_event(
			Event::ProposalCreated { multisig_address, proposer, proposal_hash }.into(),
		);
	});
}

#[test]
fn propose_fails_if_not_signer() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to propose as non-signer
		let call = make_call(vec![1, 2, 3]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(dave()), multisig_address, call, 1000),
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
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 3)); // Need 3 approvals

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Charlie approves (now 2/3)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Check event
		System::assert_last_event(
			Event::ProposalApproved {
				multisig_address,
				approver: charlie(),
				proposal_hash,
				approvals_count: 2,
			}
			.into(),
		);

		// Proposal should still exist (not executed yet)
		assert!(crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn approve_auto_executes_when_threshold_reached() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Charlie approves - threshold reached (2/2)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Check that proposal was executed (status changed, but still in storage)
		let proposal = crate::Proposals::<Test>::get(multisig_address, proposal_hash).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Executed);

		// Deposit is still locked (not returned yet)
		assert_eq!(Balances::reserved_balance(bob()), 100); // Still reserved

		// Check event was emitted
		System::assert_has_event(
			Event::ProposalExecuted {
				multisig_address,
				proposal_hash,
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
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Cancel the proposal
		assert_ok!(Multisig::cancel(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			proposal_hash
		));

		// Proposal should still exist but marked as cancelled
		let proposal = crate::Proposals::<Test>::get(multisig_address, proposal_hash).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Cancelled);

		// Deposit is still locked (not returned yet)
		assert_eq!(Balances::reserved_balance(proposer), 100);

		// Check event
		System::assert_last_event(
			Event::ProposalCancelled { multisig_address, proposer, proposal_hash }.into(),
		);
	});
}

#[test]
fn cancel_fails_if_already_executed() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Approve to execute
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Try to cancel executed proposal
		assert_noop!(
			Multisig::cancel(RuntimeOrigin::signed(bob()), multisig_address, proposal_hash),
			Error::<Test>::ProposalNotActive
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
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			expiry
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Move past expiry + grace period (100 blocks)
		System::set_block_number(expiry + 101);

		// Any signer can remove after grace period (charlie is a signer)
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Proposal should be gone
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));

		// Deposit should be returned to proposer
		assert_eq!(Balances::reserved_balance(bob()), 0);
	});
}

#[test]
fn remove_expired_works_for_executed_proposal_after_grace_period() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Execute
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Move past grace period from execution
		System::set_block_number(102); // 1 (execution) + 100 (grace) + 1

		// Remove executed proposal (charlie is a signer)
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Deposit returned
		assert_eq!(Balances::reserved_balance(bob()), 0);
	});
}

#[test]
fn remove_expired_fails_for_non_signer() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			expiry
		));

		let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);

		// Move past expiry
		System::set_block_number(expiry + 1);

		// Dave is not a signer, should fail
		assert_noop!(
			Multisig::remove_expired(
				RuntimeOrigin::signed(dave()),
				multisig_address,
				proposal_hash
			),
			Error::<Test>::NotASigner
		);

		// But charlie (who is a signer) can do it
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));
	});
}

#[test]
fn claim_deposits_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Bob creates 3 proposals
		for i in 0..3 {
			let call = make_call(vec![i as u8; 32]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				100
			));
		}

		// All reserved
		assert_eq!(Balances::reserved_balance(bob()), 300); // 3 * 100

		// Move past expiry + grace period
		System::set_block_number(201);

		// Bob claims all deposits at once
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(bob()), multisig_address));

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
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// MaxActiveProposalsParam = 10, MaxTotalProposalsInStorageParam = 20
		// Strategy: Keep active < 10, but total = 20
		// Create cycles: propose -> execute/cancel to keep active low but total high

		// Cycle 1: Create 10, execute all 10 (active=0, total=10 executed)
		for i in 0..10 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call.clone(),
				1000
			));
			// Calculate hash after propose (uses incremented nonce)
			let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);
			// Immediately execute to keep active low
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(charlie()),
				multisig_address,
				proposal_hash
			));
		}

		// Cycle 2: Create 9 more (active=9, total=19)
		for i in 10..19 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				2000
			));
		}

		// Now: 9 Active, 10 Executed = 19 total in storage
		// One more to reach limit
		let call = make_call(vec![19]);
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 2000));

		// Now: 10 Active, 10 Executed = 20 total (AT LIMIT)
		// Try to create 21st proposal - should fail with TooManyProposalsInStorage
		// Active check: 10 < 10 = false, but let's execute one first
		let call = make_call(vec![10]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 10);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Now: 9 Active, 11 Executed = 20 total
		// Active check will pass (9 < 10), but total check will fail
		let call = make_call(vec![99]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 3000),
			Error::<Test>::TooManyProposalsInStorage
		);
	});
}

#[test]
fn total_proposals_counts_executed_and_cancelled() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Create 10 active proposals
		for i in 0..10 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				1000
			));
		}

		// Execute 5 of them (they become Executed status, still in storage)
		for i in 0..5 {
			let call = make_call(vec![i as u8]);
			let proposal_hash = calculate_proposal_hash_with_nonce(&call, i);
			// Auto-execute by reaching threshold
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(charlie()),
				multisig_address,
				proposal_hash
			));
		}

		// Cancel 3 more (they become Cancelled status, still in storage)
		for i in 5..8 {
			let call = make_call(vec![i as u8]);
			let proposal_hash = calculate_proposal_hash_with_nonce(&call, i);
			assert_ok!(Multisig::cancel(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				proposal_hash
			));
		}

		// Now we have: 2 Active + 5 Executed + 3 Cancelled = 10 total
		// MaxActiveProposals = 10, MaxTotalProposalsInStorage = 20
		// We can add 8 more active (to reach 10 active) and 10 more total (to reach 20 total)

		// Add 8 more active proposals - should work (2+8=10 active, 10+8=18 total)
		for i in 20..28 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				2000
			));
		}

		// Execute one to make room for active (now 9 active, 19 total)
		let call = make_call(vec![8]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 8);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Add one more (10 active, 20 total = AT LIMIT)
		let call = make_call(vec![30]);
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 2000));

		// Now: 10 Active (9,20-28) + 6 Executed (0-4,8) + 3 Cancelled (5-7) = 19 total
		// Execute one more to free up active but keep total at 19
		let call = make_call(vec![9]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 9);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Now: 9 Active (20-28) + 7 Executed (0-4,8,9) + 3 Cancelled (5-7) = 19 total
		// Add one more to reach 20 total
		let call = make_call(vec![31]);
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 3000));

		// Now: 10 Active (20-28,31) + 7 Executed + 3 Cancelled = 20 total
		// Execute one to make room for active check
		let call = make_call(vec![20]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 10);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Now: 9 Active (21-28,31) + 8 Executed + 3 Cancelled = 20 total
		// Active check will pass (9 < 10), but total check will fail
		let call = make_call(vec![99]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 4000),
			Error::<Test>::TooManyProposalsInStorage
		);
	});
}

#[test]
fn cleanup_allows_new_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Create 10 proposals
		for i in 0..10 {
			let call = make_call(vec![i as u8]);
			let expiry = 100; // All expire at block 100
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				expiry
			));
		}

		// Execute first 5 to make room (no longer active, but still in storage)
		for i in 0..5 {
			let call = make_call(vec![i as u8]);
			let proposal_hash = calculate_proposal_hash_with_nonce(&call, i);
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(charlie()),
				multisig_address,
				proposal_hash
			));
		}

		// Move past expiry for the remaining 5
		System::set_block_number(101);

		// Now: 5 Active(expired) + 5 Executed = 10 total
		// Create 10 more proposals (cycling execute to keep active low)
		for i in 10..20 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call.clone(),
				200
			));
			// Calculate hash after propose
			let proposal_hash = calculate_last_proposal_hash(multisig_address, &call);
			// Execute immediately if i < 15 to keep active count low
			if i < 15 {
				assert_ok!(Multisig::approve(
					RuntimeOrigin::signed(charlie()),
					multisig_address,
					proposal_hash
				));
			}
		}

		// Now: 5 Active(expired) + 5 Active(fresh) + 10 Executed = 20 total
		// Active check: 10 < 10 = false, let's execute one
		// vec![15] was created in for i in 10..20, it was 6th iteration (nonce=15)
		let call = make_call(vec![15]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 15);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Now: 5 Active(expired) + 4 Active(fresh) + 11 Executed = 20 total
		// Active: 9 < 10 ✓, Total: 20 = 20 ✗
		let call = make_call(vec![99]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 200),
			Error::<Test>::TooManyProposalsInStorage
		);

		// Cleanup the 5 expired ones
		for i in 5..10 {
			let call = make_call(vec![i as u8]);
			let proposal_hash = calculate_proposal_hash_with_nonce(&call, i);
			assert_ok!(Multisig::remove_expired(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				proposal_hash
			));
		}

		// Now: 4 Active + 11 Executed = 15 total. Can add 5 more!
		for i in 20..25 {
			let call = make_call(vec![i as u8]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				300
			));
		}

		// Now: 9 Active + 11 Executed = 20 total (AT LIMIT)
		// Execute one more to make room for active check
		// vec![20] was created in for i in 20..25, first iteration (nonce=20)
		let call = make_call(vec![20]);
		let proposal_hash = calculate_proposal_hash_with_nonce(&call, 20);
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Now: 8 Active + 12 Executed = 20 total
		// Active: 8 < 10 ✓, Total: 20 = 20 ✗
		let call = make_call(vec![98]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 300),
			Error::<Test>::TooManyProposalsInStorage
		);
	});
}

#[test]
fn propose_fails_with_expiry_in_past() {
	new_test_ext().execute_with(|| {
		System::set_block_number(100);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);

		// Try to create proposal with expiry in the past (< current_block)
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call.clone(), 50),
			Error::<Test>::ExpiryInPast
		);

		// Try with expiry equal to current block (not > current_block)
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call.clone(), 100),
			Error::<Test>::ExpiryInPast
		);

		// Valid: expiry in the future
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 101));
	});
}

#[test]
fn propose_fails_with_expiry_too_far() {
	new_test_ext().execute_with(|| {
		System::set_block_number(100);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);

		// MaxExpiryDurationParam = 10000 blocks (from mock.rs)
		// Current block = 100
		// Max allowed expiry = 100 + 10000 = 10100

		// Try to create proposal with expiry too far in the future
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call.clone(), 10101),
			Error::<Test>::ExpiryTooFar
		);

		// Try with expiry way beyond the limit
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call.clone(), 20000),
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
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 10101));
	});
}

#[test]
fn propose_charges_correct_fee_with_signer_factor() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		// 3 Signers: Bob, Charlie, Dave
		let signers = vec![bob(), charlie(), dave()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let initial_balance = Balances::free_balance(proposer);

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
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

		assert_eq!(Balances::free_balance(proposer), initial_balance - deposit - expected_fee);
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
		let initial_balance = Balances::free_balance(creator);

		// Create
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));
		assert_eq!(Balances::reserved_balance(creator), deposit);

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to dissolve immediately (success)
		assert_ok!(Multisig::dissolve_multisig(RuntimeOrigin::signed(creator), multisig_address));

		// Check cleanup
		assert!(!Multisigs::<Test>::contains_key(multisig_address));
		assert_eq!(Balances::reserved_balance(creator), 0);
		// Balance returned (minus burned fee)
		assert_eq!(Balances::free_balance(creator), initial_balance - fee);
	});
}

#[test]
fn dissolve_multisig_fails_with_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Create proposal
		let call = make_call(vec![1]);
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, call, 100));

		// Try to dissolve
		assert_noop!(
			Multisig::dissolve_multisig(RuntimeOrigin::signed(creator), multisig_address),
			Error::<Test>::ProposalsExist
		);
	});
}
