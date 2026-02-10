//! Unit tests for pallet-multisig

use crate::{mock::*, Error, Event, Multisigs, ProposalStatus, Proposals};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, traits::fungible::Mutate};
use qp_high_security::HighSecurityInspector;
use sp_core::crypto::AccountId32;
use sp_runtime::DispatchError;

/// Mock implementation for HighSecurityInspector
pub struct MockHighSecurity;
impl HighSecurityInspector<AccountId32, RuntimeCall> for MockHighSecurity {
	fn is_high_security(who: &AccountId32) -> bool {
		// For testing, account 100 is high security
		who == &account_id(100)
	}
	fn is_whitelisted(call: &RuntimeCall) -> bool {
		// For testing, only remarks with "safe" are whitelisted
		match call {
			RuntimeCall::System(frame_system::Call::remark { remark }) => remark == b"safe",
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
		let deposit = 500; // MultisigDepositParam

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			threshold,
			0, // nonce
		));

		// Check balances
		// Deposit is reserved, fee is burned
		assert_eq!(Balances::reserved_balance(creator.clone()), deposit);
		assert_eq!(Balances::free_balance(creator.clone()), initial_balance - fee - deposit);

		// Check that multisig was created
		// Get multisig address
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Check storage
		let multisig_data = Multisigs::<Test>::get(&multisig_address).unwrap();
		assert_eq!(multisig_data.threshold, threshold);
		assert_eq!(multisig_data.signers.to_vec(), signers);
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
fn create_multisig_fails_with_duplicate_signers() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), bob(), charlie()]; // Bob twice
		let threshold = 2;

		assert_noop!(
			Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers,
				threshold,
				0
			),
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

		// propose() no longer auto-cleans, so Bob is still blocked
		assert_noop!(
			Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				make_call(vec![99]),
				200
			),
			Error::<Test>::TooManyProposalsPerSigner
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
			2,
			0
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

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

		// Create
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator.clone()),
			signers.clone(),
			2, // threshold
			0  // nonce
		));
		assert_eq!(Balances::reserved_balance(creator.clone()), deposit);

		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Approve dissolve by Bob (1st approval)
		assert_ok!(Multisig::approve_dissolve(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone()
		));

		// Still exists (threshold not reached)
		assert!(Multisigs::<Test>::contains_key(&multisig_address));

		// Approve dissolve by Charlie (2nd approval - threshold reached!)
		assert_ok!(Multisig::approve_dissolve(
			RuntimeOrigin::signed(charlie()),
			multisig_address.clone()
		));

		// Check cleanup - multisig removed
		assert!(!Multisigs::<Test>::contains_key(&multisig_address));
		// Deposit was returned to creator (unreserved)
		assert_eq!(Balances::reserved_balance(creator.clone()), 0);
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
			2, // threshold
			0  // nonce
		));
		let multisig_address = Multisig::derive_multisig_address(&signers, 2, 0);

		// Create proposal
		let call = make_call(vec![1]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address.clone(),
			call,
			100
		));

		// Try to approve dissolve - should fail because proposals exist
		assert_noop!(
			Multisig::approve_dissolve(RuntimeOrigin::signed(bob()), multisig_address.clone()),
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
			transfer_call.encode(),
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
		assert_eq!(alice_reserved, 500); // Only MultisigDeposit, no ProposalDeposit
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
			transfer_call.encode(),
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
				deposit: 500,
				active_proposals: 0,
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
