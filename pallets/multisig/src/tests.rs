//! Unit tests for pallet-multisig

use crate::{mock::*, Error, Event, Multisigs, ProposalStatus, Proposals};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, traits::fungible::Mutate, traits::Currency};
use qp_high_security::HighSecurityInspector;
use sp_core::crypto::AccountId32;
use sp_runtime::DispatchError;

/// Mock implementation for HighSecurityInspector
pub struct MockHighSecurity;
impl HighSecurityInspector<AccountId32, RuntimeCall> for MockHighSecurity {
	fn is_high_security(who: &AccountId32) -> bool {
		// For testing, account 100 is high security
		if who == &account_id(100) {
			return true;
		}
		// So that bench_propose_high_security passes (mock has no ReversibleTransfers genesis)
		#[cfg(feature = "runtime-benchmarks")]
		if who == &crate::benchmarking::propose_high_security_benchmark_multisig_address::<Test>() {
			return true;
		}
		false
	}
	fn is_whitelisted(call: &RuntimeCall) -> bool {
		match call {
			RuntimeCall::System(frame_system::Call::remark { remark }) =>
				remark.as_slice() == b"safe",
			RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
				..
			}) => true,
			_ => false,
		}
	}
	fn guardian(who: &AccountId32) -> Option<AccountId32> {
		if who == &account_id(100) {
			Some(account_id(200)) // Guardian is account 200
		} else {
			None
		}
	}
}

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
fn make_call(remark: Vec<u8>) -> crate::BoundedCallOf<Test> {
	let call = RuntimeCall::System(frame_system::Call::remark { remark });
	call.encode().try_into().expect("Test call should fit in MaxCallSize")
}

/// Helper function to get the ID of the last proposal created
/// Returns the current proposal_nonce - 1 (last used ID)
fn get_last_proposal_id(multisig_address: &AccountId32) -> u32 {
	let multisig = Multisigs::<Test>::get(multisig_address).expect("Multisig should exist");
	multisig.proposal_nonce.saturating_sub(1)
}

/// Assert that a DispatchResultWithPostInfo is Err with the expected error variant,
/// ignoring the PostDispatchInfo (actual_weight).
fn assert_err_ignore_postinfo(
	result: sp_runtime::DispatchResultWithInfo<frame_support::dispatch::PostDispatchInfo>,
	expected: DispatchError,
) {
	match result {
		Err(err) => assert_eq!(err.error, expected),
		Ok(_) => panic!("Expected Err({:?}), got Ok", expected),
	}
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

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0, // nonce
		));

		// Check balances - fee is burned, no deposit reserved
		assert_eq!(Balances::reserved_balance(creator.clone()), 0);
		assert_eq!(Balances::free_balance(creator.clone()), initial_balance - fee);

		// Check that multisig was created
		// Get multisig address
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Check storage
		let multisig_data = Multisigs::<Test>::get(&multisig_address).unwrap();
		assert_eq!(multisig_data.threshold, threshold);
		assert_eq!(multisig_data.signers.to_vec(), signers);

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
			Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers,
				threshold,
				0
			),
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
			Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers,
				threshold,
				0
			),
			Error::<Test>::NotEnoughSigners
		);
	});
}

#[test]
fn create_multisig_fails_with_single_signer() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		// Single signer is not allowed - use a regular account instead
		let signers = vec![alice()];
		let threshold = 1;

		assert_noop!(
			Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers,
				threshold,
				0
			),
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
			Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers,
				threshold,
				0
			),
			Error::<Test>::ThresholdTooHigh
		);
	});
}

#[test]
fn create_multisig_deduplicates_signers() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let creator = alice();
		let signers = vec![bob(), bob(), charlie()]; // Bob twice
		let threshold = 2;

		// Should succeed - duplicates are silently removed
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers,
			threshold,
			0
		));

		// The multisig should have only 2 unique signers (bob, charlie)
		let normalized_signers = vec![bob(), charlie()];
		let mut sorted = normalized_signers.clone();
		sorted.sort();
		let multisig_address = Multisig::derive_multisig_address(&sorted, threshold, 0);

		let multisig_data = Multisigs::<Test>::get(&multisig_address).unwrap();
		assert_eq!(multisig_data.signers.len(), 2);
	});
}

#[test]
fn create_multiple_multisigs_increments_nonce() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers1 = vec![bob(), charlie()];
		let signers2 = vec![bob(), dave()];

		// Create first multisig with nonce=0
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers1.clone(),
			2,
			0 // nonce
		));

		// Create second multisig with nonce=1
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers2.clone(),
			2,
			1 // nonce - user must provide different nonce
		));

		// Check both multisigs exist with their respective nonces
		let multisig1 = Multisig::derive_multisig_address(&signers1, 2, 0);
		let multisig2 = Multisig::derive_multisig_address(&signers2, 2, 1);

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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Propose a transaction
		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;

		let initial_balance = Balances::free_balance(proposer.clone());
		let proposal_deposit = 100; // ProposalDepositParam
							  // Fee calculation: Base(999) + floor(1% * 999 * 2 signers) = 999 + floor(19.98) = 999 + 19
							  // = 1018
		let proposal_fee = 1018;

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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Try to propose as non-signer
		let call = make_call(vec![1, 2, 3]);
		assert_err_ignore_postinfo(
			Multisig::propose(RuntimeOrigin::signed(dave()), multisig_address.clone(), call, 1000),
			Error::<Test>::NotASigner.into(),
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
		let threshold = 3;
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0
		)); // Need 3 approvals

		let multisig_address = Multisig::derive_multisig_address(&signers, threshold, 0);

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
			Event::SignerApproved {
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
fn approve_sets_approved_when_threshold_reached() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Charlie approves - threshold reached (2/2), status becomes Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should still exist with Approved status
		let proposal = crate::Proposals::<Test>::get(&multisig_address, proposal_id).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		// Deposit should still be reserved (not returned until execute)
		assert!(Balances::reserved_balance(bob()) > 0);

		// Check ProposalReadyToExecute event
		System::assert_has_event(
			Event::ProposalReadyToExecute {
				multisig_address: multisig_address.clone(),
				proposal_id,
				approvals_count: 2,
			}
			.into(),
		);

		// Now any signer can execute
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Now proposal is removed
		assert!(crate::Proposals::<Test>::get(&multisig_address, proposal_id).is_none());

		// Deposit returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Check execution event
		System::assert_has_event(
			Event::ProposalExecuted {
				multisig_address,
				proposal_id,
				proposer: bob(),
				call: call.to_vec(),
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Approve (reaches threshold → Approved)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Execute (removes proposal from storage)
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Try to cancel executed proposal (already removed, so ProposalNotFound)
		assert_err_ignore_postinfo(
			Multisig::cancel(RuntimeOrigin::signed(bob()), multisig_address.clone(), proposal_id),
			Error::<Test>::ProposalNotFound.into(),
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
fn executed_proposals_removed_from_storage() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			1000
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Approve → Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Execute → removed from storage, deposit returned
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be removed
		assert!(crate::Proposals::<Test>::get(&multisig_address, proposal_id).is_none());

		// Deposit should be returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Trying to remove again should fail
		assert_err_ignore_postinfo(
			Multisig::remove_expired(
				RuntimeOrigin::signed(charlie()),
				multisig_address.clone(),
				proposal_id,
			),
			Error::<Test>::ProposalNotFound.into(),
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
		assert_err_ignore_postinfo(
			Multisig::remove_expired(
				RuntimeOrigin::signed(dave()),
				multisig_address.clone(),
				proposal_id,
			),
			Error::<Test>::NotASigner.into(),
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
fn remove_expired_works_for_approved_expired_proposal() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call.clone(),
			expiry
		));

		let proposal_id = get_last_proposal_id(&multisig_address);

		// Charlie approves → status becomes Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		let proposal = Proposals::<Test>::get(&multisig_address, proposal_id).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		// Move past expiry - proposal can no longer be executed
		System::set_block_number(expiry + 1);

		// Any signer (charlie, not proposer) can remove expired Approved proposal
		// This unblocks deposits and enables multisig dissolution when proposer unavailable
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be gone
		assert!(!Proposals::<Test>::contains_key(&multisig_address, proposal_id));

		// Deposit returned to proposer (bob)
		assert_eq!(Balances::reserved_balance(bob()), 0);
	});
}

#[test]
fn claim_deposits_works_for_approved_expired_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Bob creates 2 proposals
		for i in 0..2 {
			let call = make_call(vec![i as u8; 32]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call,
				100
			));
		}

		// Charlie approves both → Approved
		for proposal_id in 0..=1 {
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(charlie()),
				multisig_address.clone(),
				proposal_id
			));
		}

		// Move past expiry
		System::set_block_number(201);

		// Bob (proposer) claims deposits from expired Approved proposals
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone()
		));

		// All deposits returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Proposals removed
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_none());
		assert!(Proposals::<Test>::get(&multisig_address, 1).is_none());
	});
}

#[test]
fn remove_expired_unblocks_undecodable_approved_proposal() {
	// Calls are now decoded at propose time for ALL multisigs (not just high-security).
	// This test verifies that invalid call bytes are rejected at propose time.
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Invalid call bytes - will fail decode at propose time (not execute)
		let undecodable_call: crate::BoundedCallOf<Test> = vec![0xffu8; 32].try_into().unwrap();
		let expiry = 100;
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				undecodable_call,
				expiry,
			),
			Error::<Test>::InvalidCall.into(),
		);

		// No proposal was created
		assert!(!Proposals::<Test>::contains_key(&multisig_address, 0));

		// No deposit was reserved (proposal failed before that)
		assert_eq!(Balances::reserved_balance(bob()), 0);
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
			}
			.into(),
		);
	});
}

#[test]
fn claim_deposits_fails_for_non_signer() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			make_call(vec![1, 2, 3]),
			100
		));

		System::set_block_number(201);

		assert_err_ignore_postinfo(
			Multisig::claim_deposits(RuntimeOrigin::signed(dave()), multisig_address.clone()),
			Error::<Test>::NotASigner.into(),
		);

		assert!(Proposals::<Test>::contains_key(&multisig_address, 0));
		assert_eq!(Balances::reserved_balance(bob()), 100);
	});
}

// ==================== HELPER FUNCTION TESTS ====================

#[test]
fn derive_multisig_address_is_deterministic() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie(), dave()];
		let threshold = 2;
		let nonce = 42;

		let address1 = Multisig::derive_multisig_address(&signers, threshold, nonce);
		let address2 = Multisig::derive_multisig_address(&signers, threshold, nonce);

		assert_eq!(address1, address2);
	});
}

#[test]
fn derive_multisig_address_different_for_different_nonce() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie(), dave()];
		let threshold = 2;

		let address1 = Multisig::derive_multisig_address(&signers, threshold, 0);
		let address2 = Multisig::derive_multisig_address(&signers, threshold, 1);

		assert_ne!(address1, address2);
	});
}

#[test]
fn is_signer_works() {
	new_test_ext().execute_with(|| {
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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
		assert_err_ignore_postinfo(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address.clone(), call, 2000),
			Error::<Test>::TooManyProposalsInStorage.into(),
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
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Test that only Active/Approved proposals remain in storage
		// (Executed/Cancelled are removed)

		// Bob creates 10, approves+executes 5, cancels 1 - only 4 active remain
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
				// Approve → Approved
				assert_ok!(Multisig::approve(
					RuntimeOrigin::signed(charlie()),
					multisig_address.clone(),
					id
				));
				// Execute → removed
				assert_ok!(Multisig::execute(
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
		// Bob: 10 Active (at per-signer limit: 20 total / 2 signers = 10 per signer)

		// Bob cannot create 11th (exceeds per-signer limit)
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				3000,
			),
			Error::<Test>::TooManyProposalsPerSigner.into(),
		);
	});
}

#[test]
fn per_signer_limit_blocks_new_proposals_until_cleanup() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Bob creates 10 proposals, all expire at block 100 (at per-signer limit)
		for i in 0..10 {
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![i]),
				100
			));
		}
		// Bob: 10 Active (at per-signer limit: 20 total / 2 signers = 10 per signer)

		// Bob cannot create more (at limit)
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				200,
			),
			Error::<Test>::TooManyProposalsPerSigner.into(),
		);

		// Move past expiry
		System::set_block_number(101);

		// propose() no longer auto-cleans, so Bob is still blocked
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				200,
			),
			Error::<Test>::TooManyProposalsPerSigner.into(),
		);

		// Bob must explicitly claim deposits to free space
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
		));

		// Now Bob can create new
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			make_call(vec![99]),
			200
		));

		// Verify: old expired removed by claim_deposits, plus the new one
		let count = crate::Proposals::<Test>::iter_prefix(&multisig_address).count();
		assert_eq!(count, 1);
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);

		// Try to create proposal with expiry in the past (< current_block)
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				50,
			),
			Error::<Test>::ExpiryInPast.into(),
		);

		// Try with expiry equal to current block (not > current_block)
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				100,
			),
			Error::<Test>::ExpiryInPast.into(),
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let call = make_call(vec![1, 2, 3]);

		// MaxExpiryDurationParam = 10000 blocks (from mock.rs)
		// Current block = 100
		// Max allowed expiry = 100 + 10000 = 10100

		// Try to create proposal with expiry too far in the future
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				10101,
			),
			Error::<Test>::ExpiryTooFar.into(),
		);

		// Try with expiry way beyond the limit
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				call.clone(),
				20000,
			),
			Error::<Test>::ExpiryTooFar.into(),
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let initial_balance = Balances::free_balance(proposer.clone());

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address,
			call,
			1000
		));

		// ProposalFeeParam = 999
		// SignerStepFactor = 1%
		// Signers = 3
		// Calculation: 999 + floor(1% * 999 * 3) = 999 + floor(29.97) = 999 + 29 = 1028
		let expected_fee = 1028;
		let deposit = 100; // ProposalDepositParam

		assert_eq!(
			Balances::free_balance(proposer.clone()),
			initial_balance - deposit - expected_fee
		);
		// Fee is burned (reduces total issuance)
	});
}

#[test]
fn fee_calculation_order_of_operations_is_correct() {
	// This test verifies that the fee calculation uses the correct order of operations:
	// Fee = Base + floor(StepFactor * Base * SignerCount)
	//
	// The WRONG formula would be:
	// Fee = Base + floor(StepFactor * Base) * SignerCount
	//
	// The difference matters when floor(StepFactor * Base) truncates.
	// Example with base=99, factor=1%, signers=100:
	//   Wrong:   99 + floor(0.99) * 100 = 99 + 0 * 100 = 99  (no increase!)
	//   Correct: 99 + floor(0.99 * 100) = 99 + floor(99) = 198
	use sp_runtime::Permill;

	// Test case where early floor truncation would cause loss of precision
	let base_fee: u128 = 99;
	let step_factor = Permill::from_percent(1); // 1%
	let signers_count: u128 = 100;

	// WRONG way (early floor truncation):
	let wrong_per_signer = step_factor.mul_floor(base_fee); // floor(0.99) = 0
	let wrong_total_increase = wrong_per_signer.saturating_mul(signers_count); // 0 * 100 = 0
	let wrong_fee = base_fee.saturating_add(wrong_total_increase); // 99 + 0 = 99

	// CORRECT way (multiply first, then floor):
	let multiplier = base_fee.saturating_mul(signers_count); // 99 * 100 = 9900
	let correct_total_increase = step_factor.mul_floor(multiplier); // floor(1% * 9900) = 99
	let correct_fee = base_fee.saturating_add(correct_total_increase); // 99 + 99 = 198

	// Verify the formulas produce different results
	assert_eq!(wrong_fee, 99, "Wrong formula should give 99");
	assert_eq!(correct_fee, 198, "Correct formula should give 198");
	assert_ne!(wrong_fee, correct_fee, "The two formulas should differ for this input");
}

#[test]
fn propose_charges_correct_fee_with_max_signers() {
	// Integration test with max signers to verify fee scaling works correctly
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		// Use max signers (10) to maximize the fee increase
		let signers = vec![
			bob(),
			charlie(),
			dave(),
			account_id(5),
			account_id(6),
			account_id(7),
			account_id(8),
			account_id(9),
			account_id(10),
			account_id(11),
		];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			5,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 5, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let initial_balance = Balances::free_balance(proposer.clone());

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer.clone()),
			multisig_address,
			call,
			1000
		));

		// ProposalFeeParam = 999
		// SignerStepFactor = 1% (Permill::from_parts(10_000))
		// Signers = 10
		//
		// Correct calculation: 999 + floor(1% * 999 * 10) = 999 + floor(99.9) = 999 + 99 = 1098
		let expected_fee = 1098;
		let deposit = 100; // ProposalDepositParam

		assert_eq!(
			Balances::free_balance(proposer.clone()),
			initial_balance - deposit - expected_fee
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
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// MaxTotalProposalsInStorage = 20
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
		assert_err_ignore_postinfo(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				2000,
			),
			Error::<Test>::TooManyProposalsPerSigner.into(),
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

#[test]
fn propose_with_threshold_one_sets_approved() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![alice(), bob(), charlie()];
		let threshold = 1; // Only 1 approval needed

		// Create multisig with threshold=1
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, threshold, 0);

		// Fund multisig account for balance transfer
		<pallet_balances::Pallet<Test> as Mutate<_>>::mint_into(&multisig_address, 50000).unwrap();

		let initial_dave_balance = Balances::free_balance(dave());

		// Alice proposes a transfer - threshold=1, so immediately Approved
		let transfer_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: dave(),
			value: 1000,
		});

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			transfer_call.encode().try_into().unwrap(),
			100
		));

		let proposal_id = 0;

		// Proposal should be Approved (not executed yet)
		let proposal = Proposals::<Test>::get(&multisig_address, proposal_id).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		// Transfer hasn't happened yet
		assert_eq!(Balances::free_balance(dave()), initial_dave_balance);

		// Check ProposalReadyToExecute event
		System::assert_has_event(
			Event::ProposalReadyToExecute {
				multisig_address: multisig_address.clone(),
				proposal_id,
				approvals_count: 1,
			}
			.into(),
		);

		// Any signer can now execute
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			proposal_id
		));

		// Now the transfer happened
		assert_eq!(Balances::free_balance(dave()), initial_dave_balance + 1000);

		// Proposal removed, deposit returned
		assert!(Proposals::<Test>::get(&multisig_address, proposal_id).is_none());
		let alice_reserved = Balances::reserved_balance(alice());
		assert_eq!(alice_reserved, 0); // ProposalDeposit returned after execution
	});
}

#[test]
fn propose_with_threshold_two_waits_for_approval() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![alice(), bob(), charlie()];
		let threshold = 2; // Need 2 approvals

		// Create multisig with threshold=2
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Fund multisig account
		<pallet_balances::Pallet<Test> as Mutate<_>>::mint_into(&multisig_address, 50000).unwrap();

		let initial_dave_balance = Balances::free_balance(dave());

		// Alice proposes a transfer - should NOT execute yet
		let transfer_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: dave(),
			value: 1000,
		});

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			transfer_call.encode().try_into().unwrap(),
			100
		));

		let proposal_id = 0;

		// Verify the proposal still exists (waiting for more approvals)
		let proposal = Proposals::<Test>::get(&multisig_address, proposal_id).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Active);
		assert_eq!(proposal.approvals.len(), 1); // Only Alice so far

		// Verify the transfer did NOT happen yet
		assert_eq!(Balances::free_balance(dave()), initial_dave_balance);

		// Bob approves - threshold=2 reached → Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			proposal_id
		));

		// Proposal should be Approved but NOT removed
		let proposal = Proposals::<Test>::get(&multisig_address, proposal_id).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		// Transfer NOT yet happened
		assert_eq!(Balances::free_balance(dave()), initial_dave_balance);

		// Now execute
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			proposal_id
		));

		// Now proposal removed and transfer happened
		assert!(Proposals::<Test>::get(&multisig_address, proposal_id).is_none());
		assert_eq!(Balances::free_balance(dave()), initial_dave_balance + 1000);
	});
}

#[test]
fn no_auto_cleanup_on_propose_approve_cancel() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![alice(), bob(), charlie()];
		let threshold = 3; // Need all 3 signers - prevents auto-execution during test

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 3, 0);

		// Create two proposals
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			make_call(vec![1]),
			100 // expires at block 100
		));

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			make_call(vec![2]),
			200 // expires at block 200
		));

		// Verify both proposals exist
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_some());
		assert!(Proposals::<Test>::get(&multisig_address, 1).is_some());

		// Move time forward past first proposal expiry
		System::set_block_number(101);

		// approve() does NOT auto-cleanup
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			1
		));
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_some()); // expired but still there

		// propose() does NOT auto-cleanup either
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			make_call(vec![3]),
			150
		));
		// Proposal #0 still exists - not auto-cleaned
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_some());
		assert!(Proposals::<Test>::get(&multisig_address, 1).is_some());
		assert!(Proposals::<Test>::get(&multisig_address, 2).is_some());

		// cancel() does NOT auto-cleanup
		System::set_block_number(151);
		assert_ok!(Multisig::cancel(RuntimeOrigin::signed(bob()), multisig_address.clone(), 1));
		assert!(Proposals::<Test>::get(&multisig_address, 1).is_none()); // cancelled
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_some()); // expired, still there
		assert!(Proposals::<Test>::get(&multisig_address, 2).is_some()); // expired, still there

		// Only explicit cleanup works: claim_deposits or remove_expired
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
		));
		// Alice's expired proposals (#0, #2) now cleaned
		assert!(Proposals::<Test>::get(&multisig_address, 0).is_none());
		assert!(Proposals::<Test>::get(&multisig_address, 2).is_none());
	});
}

// ==================== HIGH SECURITY TESTS ====================

#[test]
fn high_security_propose_fails_for_non_whitelisted_call() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create a multisig with account_id(100) as one of signers
		// We'll manually insert it as high-security multisig
		let multisig_address = account_id(100);
		let signers = vec![alice(), bob()];

		Multisigs::<Test>::insert(
			&multisig_address,
			crate::MultisigData {
				creator: alice(),
				signers: signers.try_into().unwrap(),
				threshold: 2,
				proposal_nonce: 0,
				proposals_per_signer: Default::default(),
			},
		);

		// Try to propose a non-whitelisted call (remark without "safe")
		let call = make_call(b"unsafe".to_vec());
		assert_err_ignore_postinfo(
			Multisig::propose(RuntimeOrigin::signed(alice()), multisig_address.clone(), call, 1000),
			Error::<Test>::CallNotAllowedForHighSecurityMultisig.into(),
		);

		// Try to propose a whitelisted call (remark with "safe") - should work
		let call = make_call(b"safe".to_vec());
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			call,
			1000
		));
	});
}

#[test]
fn normal_multisig_allows_any_call() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create a normal multisig (not high-security)
		let signers = vec![alice(), bob(), charlie()];
		let threshold = 2;
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			threshold,
			0 // nonce
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Any call should work for normal multisig
		let call = make_call(b"anything".to_vec());
		assert_ok!(Multisig::propose(RuntimeOrigin::signed(alice()), multisig_address, call, 1000));
	});
}

// ============================================================================
// Audit Test Coverage (EQ-QNT-MULTISIG-O-09 / Appendix A)
// ============================================================================

/// Test 1: Approving an already-approved proposal should still work and emit correct events
#[test]
fn approve_on_already_approved_proposal_emits_signer_approved_only() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create 2-of-3 multisig
		let signers = vec![alice(), bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Alice proposes
		let call = make_call(b"test".to_vec());
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			call,
			1000
		));

		// Bob approves - this reaches threshold (2), status becomes Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			0
		));

		// Verify proposal is Approved
		let proposal = Proposals::<Test>::get(&multisig_address, 0).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		// Clear events
		System::reset_events();

		// Charlie approves an already-approved proposal
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone(),
			0
		));

		// Should emit SignerApproved but NOT ProposalReadyToExecute (already approved)
		let events = System::events();
		let signer_approved_count = events
			.iter()
			.filter(|e| matches!(e.event, RuntimeEvent::Multisig(Event::SignerApproved { .. })))
			.count();
		let ready_to_execute_count = events
			.iter()
			.filter(|e| {
				matches!(e.event, RuntimeEvent::Multisig(Event::ProposalReadyToExecute { .. }))
			})
			.count();

		assert_eq!(signer_approved_count, 1, "Should emit SignerApproved");
		assert_eq!(ready_to_execute_count, 0, "Should NOT emit ProposalReadyToExecute again");
	});
}

/// Test 2: proposal_nonce overflow returns explicit error
#[test]
fn proposal_nonce_overflow_returns_error() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Manually set proposal_nonce to u32::MAX
		Multisigs::<Test>::mutate(&multisig_address, |maybe_data| {
			if let Some(ref mut data) = maybe_data {
				data.proposal_nonce = u32::MAX;
			}
		});

		// Attempt to propose should fail with ProposalNonceExhausted
		let call = make_call(b"test".to_vec());
		assert_err_ignore_postinfo(
			Multisig::propose(RuntimeOrigin::signed(alice()), multisig_address, call, 1000),
			Error::<Test>::ProposalNonceExhausted.into()
		);
	});
}

/// Test 3: Execute a proposal that dispatches back into multisig (reentrancy)
#[test]
fn execute_proposal_that_calls_back_into_multisig() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create 1-of-2 multisig so proposals are immediately approved
		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			1,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 1, 0);

		// Fund the multisig so it can pay for create_multisig
		Balances::make_free_balance_be(&multisig_address, 100_000);

		// Create a call that will create another multisig (calls back into pallet)
		let inner_call = RuntimeCall::Multisig(crate::Call::create_multisig {
			signers: vec![charlie(), dave()],
			threshold: 2,
			nonce: 99,
		});
		let encoded_call: crate::BoundedCallOf<Test> = inner_call.encode().try_into().unwrap();

		// Propose (immediately Approved since threshold=1)
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			encoded_call,
			1000
		));

		// Execute - this should work without reentrancy issues
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			0
		));

		// Verify the inner call succeeded - new multisig should exist
		let new_multisig_address =
			Multisig::derive_multisig_address(&vec![charlie(), dave()], 2, 99);
		assert!(Multisigs::<Test>::contains_key(&new_multisig_address));
	});
}

/// Test 5: claim_deposits with zero expired proposals succeeds with zero returned
#[test]
fn claim_deposits_with_zero_expired_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Create a proposal that is NOT expired
		let call = make_call(b"test".to_vec());
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			call,
			10000 // expires far in the future
		));

		// claim_deposits should succeed but return 0
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone()
		));

		// Check event shows 0 returned
		System::assert_last_event(
			Event::DepositsClaimed {
				multisig_address,
				claimer: alice(),
				total_returned: 0,
				proposals_removed: 0,
			}
			.into(),
		);

		// Proposal should still exist
		assert!(Proposals::<Test>::contains_key(
			&Multisig::derive_multisig_address(&signers, 2, 0),
			0
		));
	});
}

/// Test 8: propose does not burn fee if deposit reservation fails
#[test]
fn propose_does_not_burn_fee_if_deposit_fails() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Give alice exactly enough for fee but not deposit
		// Fee = 999 base + (999 * 2 signers * 1%) = 999 + 19 = 1018 (using mock values)
		// Deposit = 100 (ProposalDepositParam)
		// Total needed = 1118
		// Give her only 1050 (enough for fee, not enough for fee + deposit)
		let fee_only = 1050u128;
		Balances::make_free_balance_be(&alice(), fee_only);

		let initial_balance = Balances::free_balance(alice());

		// Propose should fail
		let call = make_call(b"test".to_vec());
		assert_err_ignore_postinfo(
			Multisig::propose(RuntimeOrigin::signed(alice()), multisig_address, call, 1000),
			Error::<Test>::InsufficientBalance.into()
		);

		// Check that balance is unchanged (fee was NOT burned)
		// Note: This test will FAIL if the bug exists (fee burned before deposit check)
		assert_eq!(
			Balances::free_balance(alice()),
			initial_balance,
			"Balance should be unchanged - fee should not be burned if deposit fails"
		);
	});
}

/// Test 9: claim_deposits for threshold-1 multisig (1-of-2)
#[test]
fn claim_deposits_threshold_one_multisig() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create 1-of-2 multisig (threshold=1 requires at least 2 signers)
		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			1,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 1, 0);

		// Create an expired proposal
		let call = make_call(b"test".to_vec());
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			call,
			5 // expires at block 5
		));

		// Advance past expiry
		System::set_block_number(10);

		let deposit = 100u128; // ProposalDepositParam
		let balance_before = Balances::free_balance(alice());

		// claim_deposits should work
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone()
		));

		// Deposit should be returned
		assert_eq!(Balances::free_balance(alice()), balance_before + deposit);

		// Proposal should be removed
		assert!(!Proposals::<Test>::contains_key(&multisig_address, 0));
	});
}

/// Test 10: DepositsClaimed.total_returned accurately reflects actual deposits
#[test]
fn deposits_claimed_total_returned_is_accurate() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Create 3 proposals that will expire
		for i in 0..3 {
			let call = make_call(format!("test{}", i).into_bytes());
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				call,
				5 // expires at block 5
			));
		}

		// Advance past expiry
		System::set_block_number(10);

		let deposit_per_proposal = 100u128; // ProposalDepositParam
		let expected_total = deposit_per_proposal * 3;

		// claim_deposits
		assert_ok!(Multisig::claim_deposits(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone()
		));

		// Verify event has correct total
		System::assert_last_event(
			Event::DepositsClaimed {
				multisig_address,
				claimer: alice(),
				total_returned: expected_total,
				proposals_removed: 3,
			}
			.into(),
		);
	});
}

/// Test 11: cancel on already-approved proposal works
#[test]
fn cancel_works_on_approved_proposal() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let signers = vec![alice(), bob(), charlie()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			2,
			0
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Alice proposes
		let call = make_call(b"test".to_vec());
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			call,
			1000
		));

		// Bob approves - reaches threshold, status becomes Approved
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			0
		));

		// Verify it's approved
		let proposal = Proposals::<Test>::get(&multisig_address, 0).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		let deposit = 100u128; // ProposalDepositParam
		let balance_before = Balances::free_balance(alice());

		// Alice (proposer) cancels the approved proposal
		assert_ok!(Multisig::cancel(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			0
		));

		// Proposal should be removed
		assert!(!Proposals::<Test>::contains_key(&multisig_address, 0));

		// Deposit should be returned to proposer
		assert_eq!(Balances::free_balance(alice()), balance_before + deposit);

		// Event emitted
		System::assert_last_event(
			Event::ProposalCancelled { multisig_address, proposer: alice(), proposal_id: 0 }.into(),
		);
	});
}

/// Test that execute succeeds (removes proposal) even when the inner call fails.
/// This is critical because FRAME rolls back on Err, so we must return Ok regardless
/// of inner call outcome to ensure proposal removal is persisted.
#[test]
fn execute_succeeds_even_when_inner_call_fails() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let signers = vec![alice(), bob()];
		// Create a 1-of-2 multisig (threshold 1, so proposal is immediately approved)
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			1,
			0, // nonce
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 1, 0);

		// Fund the multisig with a small amount (not enough for the transfer)
		let _ = Balances::mint_into(&multisig_address, 100);

		// Create a transfer call that will fail (trying to transfer more than balance)
		let transfer_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: charlie(),
			value: 1_000_000, // Way more than the multisig has
		});

		// Propose - should immediately be approved since threshold is 1
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			transfer_call.encode().try_into().unwrap(),
			100,
		));

		// Verify proposal is approved
		let proposal = Proposals::<Test>::get(&multisig_address, 0).unwrap();
		assert_eq!(proposal.status, ProposalStatus::Approved);

		let proposer_balance_before = Balances::free_balance(alice());
		let deposit = proposal.deposit;

		// Execute - the inner call will fail due to insufficient balance
		// But execute itself should succeed (return Ok)
		let result = Multisig::execute(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			0,
		);

		// The extrinsic itself should succeed
		assert_ok!(result);

		// Proposal should be removed from storage
		assert!(
			!Proposals::<Test>::contains_key(&multisig_address, 0),
			"Proposal must be removed even when inner call fails"
		);

		// Deposit should be returned to proposer
		assert_eq!(
			Balances::free_balance(alice()),
			proposer_balance_before + deposit,
			"Deposit must be returned even when inner call fails"
		);

		// ProposalExecuted event should be emitted with the inner call's error
		System::assert_has_event(
			Event::ProposalExecuted {
				multisig_address,
				proposal_id: 0,
				proposer: alice(),
				call: transfer_call.encode(),
				approvers: vec![alice()],
				result: Err(DispatchError::Token(sp_runtime::TokenError::FundsUnavailable)),
			}
			.into(),
		);
	});
}
