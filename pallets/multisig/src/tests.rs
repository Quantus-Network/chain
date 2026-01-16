//! Unit tests for pallet-multisig

use crate::{mock::*, Error, Event, GlobalNonce, Multisigs};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use sp_runtime::traits::Hash;

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
		let deposit = 100; // MultisigDepositParam
		let fee = 50; // MultisigFeeParam

		// Create multisig
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator),
			signers.clone(),
			threshold,
		));

		// Check that deposit was reserved and fee was burned
		assert_eq!(Balances::reserved_balance(creator), deposit);
		assert_eq!(Balances::free_balance(creator), initial_balance - deposit - fee);

		// Check that multisig was created
		let global_nonce = GlobalNonce::<Test>::get();
		assert_eq!(global_nonce, 1);

		// Get multisig address
		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Check storage
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();
		assert_eq!(multisig_data.threshold, threshold);
		assert_eq!(multisig_data.nonce, 0);
		assert_eq!(multisig_data.deposit, deposit);
		assert_eq!(multisig_data.signers.to_vec(), signers);
		assert_eq!(multisig_data.active_proposals, 0);

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
		let threshold = 3; // More than the number of signers

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
		let signers = vec![bob(), charlie(), bob()]; // Bob appears twice
		let threshold = 2;

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
			Error::<Test>::DuplicateSigner
		);
	});
}

#[test]
fn create_multisig_fails_with_too_many_signers() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		// MaxSignersParam is 10, so 11 should fail
		let signers: Vec<u64> = (1..=11).collect();
		let threshold = 2;

		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(creator), signers, threshold,),
			Error::<Test>::TooManySigners
		);
	});
}

#[test]
fn create_multisig_fails_with_insufficient_balance() {
	new_test_ext().execute_with(|| {
		// Create account with insufficient balance
		let poor_account = 99;
		let signers = vec![bob(), charlie()];
		let threshold = 2;

		// This account has 0 balance, can't pay deposit
		assert_noop!(
			Multisig::create_multisig(RuntimeOrigin::signed(poor_account), signers, threshold,),
			Error::<Test>::InsufficientBalance
		);
	});
}

#[test]
fn create_multiple_multisigs_works() {
	new_test_ext().execute_with(|| {
		// Initialize block number for events
		System::set_block_number(1);

		let creator = alice();

		// Create first multisig
		let signers1 = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers1.clone(), 2,));

		// Create second multisig with different signers
		let signers2 = vec![charlie(), dave()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers2.clone(), 2,));

		// Check global nonce incremented
		assert_eq!(GlobalNonce::<Test>::get(), 2);

		// Check both multisigs exist
		let multisig1 = Multisig::derive_multisig_address(&signers1, 0);
		let multisig2 = Multisig::derive_multisig_address(&signers2, 1);

		assert!(Multisigs::<Test>::contains_key(multisig1));
		assert!(Multisigs::<Test>::contains_key(multisig2));

		// Charlie can be in unlimited multisigs (no artificial limit)
		// Both multisigs should exist independently
	});
}

#[test]
fn max_active_proposals_limit_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// MaxActiveProposalsParam = 10 in mock
		// Create 10 proposals (should work)
		for i in 0..10 {
			let call = make_call(vec![i as u8, 2, 3]);
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(bob()),
				multisig_address,
				call,
				1000
			));
		}

		// Check counter
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();
		assert_eq!(multisig_data.active_proposals, 10);

		// Try to create 11th proposal (should fail)
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(bob()), multisig_address, vec![99, 2, 3], 1000),
			Error::<Test>::TooManyActiveProposals
		);

		// Execute one proposal to free up space
		use frame_support::BoundedVec;
		let call1 = make_call(vec![0, 2, 3]);
		let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
			call1.try_into().unwrap();
		let hash1 = <Test as frame_system::Config>::Hashing::hash_of(&bounded);

		assert_ok!(Multisig::approve(RuntimeOrigin::signed(charlie()), multisig_address, hash1));
		assert_ok!(Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, hash1));

		// Check counter decreased
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();
		assert_eq!(multisig_data.active_proposals, 9);

		// Now we can create a new proposal
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			vec![100, 2, 3],
			1000
		));

		// Counter back to 10
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();
		assert_eq!(multisig_data.active_proposals, 10);
	});
}

#[test]
fn create_multisig_with_single_signer_works() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob()];
		let threshold = 1;

		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator),
			signers.clone(),
			threshold,
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);
		let multisig_data = Multisigs::<Test>::get(multisig_address).unwrap();

		assert_eq!(multisig_data.threshold, 1);
		assert_eq!(multisig_data.signers.len(), 1);
	});
}

#[test]
fn is_signer_works() {
	new_test_ext().execute_with(|| {
		let creator = alice();
		let signers = vec![bob(), charlie(), dave()];
		let threshold = 2;

		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator),
			signers.clone(),
			threshold,
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Check signers
		assert!(Multisig::is_signer(&multisig_address, &bob()));
		assert!(Multisig::is_signer(&multisig_address, &charlie()));
		assert!(Multisig::is_signer(&multisig_address, &dave()));

		// Check non-signers
		assert!(!Multisig::is_signer(&multisig_address, &alice()));
		assert!(!Multisig::is_signer(&multisig_address, &99));
	});
}

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
fn derive_multisig_address_different_for_different_signers() {
	new_test_ext().execute_with(|| {
		let signers1 = vec![bob(), charlie()];
		let signers2 = vec![bob(), dave()];
		let nonce = 0;

		let address1 = Multisig::derive_multisig_address(&signers1, nonce);
		let address2 = Multisig::derive_multisig_address(&signers2, nonce);

		assert_ne!(address1, address2);
	});
}

#[test]
fn signer_order_does_not_matter_for_address() {
	new_test_ext().execute_with(|| {
		// Signers are sorted internally, so order doesn't matter
		let signers1 = vec![bob(), charlie()];
		let signers2 = vec![charlie(), bob()];

		// Sort both to simulate what happens in create_multisig
		let mut sorted1 = signers1.clone();
		let mut sorted2 = signers2.clone();
		sorted1.sort();
		sorted2.sort();

		let nonce = 0;
		let address1 = Multisig::derive_multisig_address(&sorted1, nonce);
		let address2 = Multisig::derive_multisig_address(&sorted2, nonce);

		// Same signers, same nonce = same address (order doesn't matter)
		assert_eq!(address1, address2);
	});
}

// ==================== PROPOSAL TESTS ====================

#[test]
fn propose_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create multisig
		let creator = alice();
		let signers = vec![bob(), charlie(), dave()];
		let threshold = 2;
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(creator),
			signers.clone(),
			threshold
		));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Propose a transaction
		let proposer = bob();
		let call = vec![1, 2, 3, 4];
		let expiry = 1000;
		let initial_balance = Balances::free_balance(proposer);
		let proposal_deposit = 10; // ProposalDepositParam (refundable)
		let proposal_fee = 5; // ProposalFeeParam (non-refundable)

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			expiry
		));

		// Check: fee was withdrawn (lost forever) + deposit was reserved
		assert_eq!(Balances::reserved_balance(proposer), proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - proposal_deposit - proposal_fee
		);

		// Check proposal exists
		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		assert!(crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn propose_fails_if_not_signer() {
	new_test_ext().execute_with(|| {
		// Create multisig
		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to propose as non-signer
		let non_signer = dave();
		let call = make_call(vec![1, 2, 3]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(non_signer), multisig_address, call, 1000),
			Error::<Test>::NotASigner
		);
	});
}

#[test]
fn propose_fails_with_insufficient_balance() {
	new_test_ext().execute_with(|| {
		// Create multisig with poor account as signer
		let creator = alice();
		let poor_account = 99; // No balance
		let signers = vec![bob(), charlie(), poor_account];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Try to propose with insufficient balance
		let call = make_call(vec![1, 2, 3]);
		assert_noop!(
			Multisig::propose(RuntimeOrigin::signed(poor_account), multisig_address, call, 1000),
			Error::<Test>::InsufficientBalance
		);
	});
}

#[test]
fn approve_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		// Create multisig
		let creator = alice();
		let signers = vec![bob(), charlie(), dave()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		// Propose
		let call = make_call(vec![1, 2, 3, 4]);
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Approve
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Check approval was added
		let proposal = crate::Proposals::<Test>::get(multisig_address, proposal_hash).unwrap();
		assert_eq!(proposal.approvals.len(), 2); // bob + charlie
		assert!(proposal.approvals.contains(&charlie()));
	});
}

#[test]
fn approve_fails_if_already_approved() {
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

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Try to approve twice
		assert_noop!(
			Multisig::approve(RuntimeOrigin::signed(bob()), multisig_address, proposal_hash),
			Error::<Test>::AlreadyApproved
		);
	});
}

#[test]
fn execute_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let initial_balance = Balances::free_balance(proposer);
		let _proposal_deposit = 10; // Refundable
		let proposal_fee = 5; // Non-refundable

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Approve to reach threshold
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Execute
		assert_ok!(Multisig::execute(
			RuntimeOrigin::signed(alice()),
			multisig_address,
			proposal_hash
		));

		// Check deposit was returned, but fee was NOT returned
		assert_eq!(Balances::reserved_balance(proposer), 0);
		assert_eq!(Balances::free_balance(proposer), initial_balance - proposal_fee); // Only fee lost

		// Check proposal was removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn execute_fails_if_expired_even_if_threshold_met() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let call = make_call(vec![1, 2, 3]);
		let expiry = 10;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call.clone(),
			expiry
		));

		// Match pallet hashing: hash_of(bounded_call)
		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};
		let proposal_hash = get_hash(call);

		// Reach threshold before expiry (bob auto-approves in propose)
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Move past expiry and attempt to execute
		System::set_block_number(expiry + 1);
		assert_noop!(
			Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, proposal_hash),
			Error::<Test>::ProposalExpired
		);
	});
}

#[test]
fn execute_fails_without_threshold() {
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

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Only 2 approvals (bob + charlie), need 3
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			proposal_hash
		));

		// Try to execute without threshold
		assert_noop!(
			Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, proposal_hash),
			Error::<Test>::NotEnoughApprovals
		);
	});
}

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
		let initial_balance = Balances::free_balance(proposer);
		let proposal_fee = 5; // Non-refundable, even on cancel!

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			1000
		));

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Cancel
		assert_ok!(Multisig::cancel(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			proposal_hash
		));

		// Check deposit was returned, but fee was NOT returned (even on cancel!)
		assert_eq!(Balances::reserved_balance(proposer), 0);
		assert_eq!(Balances::free_balance(proposer), initial_balance - proposal_fee); // Fee still lost

		// Check proposal was removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn cancel_fails_if_not_proposer() {
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

		let proposal_hash = <Test as frame_system::Config>::Hashing::hash_of(&call);

		// Try to cancel as different user
		assert_noop!(
			Multisig::cancel(RuntimeOrigin::signed(charlie()), multisig_address, proposal_hash),
			Error::<Test>::NotProposer
		);
	});
}

#[test]
fn proposal_fee_is_never_returned() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let initial_balance = Balances::free_balance(proposer);
		let proposal_deposit = 10; // Refundable
		let proposal_fee = 5; // Non-refundable

		// Helper to get proposal hash (must convert to BoundedVec first)
		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};

		// Create 3 proposals
		let call1 = make_call(vec![0, 2, 3]);
		let call2 = make_call(vec![1, 2, 3]);
		let call3 = make_call(vec![2, 2, 3]);

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call1.clone(),
			1000
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call2.clone(),
			1000
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call3.clone(),
			1000
		));

		// After 3 proposals: 3 deposits reserved + 3 fees lost
		assert_eq!(Balances::reserved_balance(proposer), 3 * proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - 3 * proposal_deposit - 3 * proposal_fee
		);

		// Cancel one proposal
		let hash1 = get_hash(call1);
		assert_ok!(Multisig::cancel(RuntimeOrigin::signed(proposer), multisig_address, hash1));

		// After cancel: 2 deposits reserved + 3 fees still lost
		assert_eq!(Balances::reserved_balance(proposer), 2 * proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - 2 * proposal_deposit - 3 * proposal_fee
		);

		// Execute another proposal
		let hash2 = get_hash(call2);
		assert_ok!(Multisig::approve(RuntimeOrigin::signed(charlie()), multisig_address, hash2));
		assert_ok!(Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, hash2));

		// After execute: 1 deposit reserved + 3 fees still lost
		assert_eq!(Balances::reserved_balance(proposer), proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - proposal_deposit - 3 * proposal_fee
		);

		// Lesson: Fees are NEVER returned, regardless of outcome!
	});
}

// ==================== EXPIRED PROPOSAL CLEANUP TESTS ====================

#[test]
fn remove_expired_fails_if_not_expired() {
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

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};
		let proposal_hash = get_hash(call);

		// Try to remove before expiry (at block 500)
		System::set_block_number(500);
		assert_noop!(
			Multisig::remove_expired(
				RuntimeOrigin::signed(alice()),
				multisig_address,
				proposal_hash
			),
			Error::<Test>::ProposalNotExpired
		);
	});
}

#[test]
fn remove_expired_within_grace_period_only_by_proposer() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;
		let initial_balance = Balances::free_balance(proposer);
		let _proposal_deposit = 10;
		let proposal_fee = 5;

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			expiry
		));

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};
		let proposal_hash = get_hash(call);

		// Move to grace period (expiry + 50 < expiry + grace_period(100))
		System::set_block_number(expiry + 50);

		// Non-proposer cannot remove within grace period
		assert_noop!(
			Multisig::remove_expired(
				RuntimeOrigin::signed(charlie()),
				multisig_address,
				proposal_hash
			),
			Error::<Test>::NotProposer
		);

		// Proposer CAN remove within grace period
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			proposal_hash
		));

		// Check deposit was returned, fee still lost
		assert_eq!(Balances::reserved_balance(proposer), 0);
		assert_eq!(Balances::free_balance(proposer), initial_balance - proposal_fee);

		// Check proposal was removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn remove_expired_after_grace_period_by_anyone() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let call = make_call(vec![1, 2, 3]);
		let expiry = 1000;
		let initial_balance = Balances::free_balance(proposer);
		let _proposal_deposit = 10;
		let proposal_fee = 5;

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call.clone(),
			expiry
		));

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};
		let proposal_hash = get_hash(call);

		// Move past grace period (expiry + grace_period(100) + 1)
		let grace_period = 100; // GracePeriodParam
		System::set_block_number(expiry + grace_period + 1);

		// Anyone can remove after grace period (dave is not even a signer)
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(dave()),
			multisig_address,
			proposal_hash
		));

		// Check deposit was returned to proposer (not dave!)
		assert_eq!(Balances::reserved_balance(proposer), 0);
		assert_eq!(Balances::free_balance(proposer), initial_balance - proposal_fee);

		// Check proposal was removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, proposal_hash));
	});
}

#[test]
fn remove_expired_multiple_proposals_cleanup() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};

		// Create 3 proposals with different expiries
		let call1 = make_call(vec![1, 2, 3]);
		let call2 = make_call(vec![4, 5, 6]);
		let call3 = make_call(vec![7, 8, 9]);

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call1.clone(),
			100 // expires at 100
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call2.clone(),
			200 // expires at 200
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call3.clone(),
			300 // expires at 300
		));

		let hash1 = get_hash(call1);
		let hash2 = get_hash(call2);
		let hash3 = get_hash(call3);

		// Move past all expiries + grace period
		System::set_block_number(500);

		// Cleanup all 3 (anyone can do it)
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(dave()),
			multisig_address,
			hash1
		));
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(dave()),
			multisig_address,
			hash2
		));
		assert_ok!(Multisig::remove_expired(
			RuntimeOrigin::signed(dave()),
			multisig_address,
			hash3
		));

		// All removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, hash1));
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, hash2));
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, hash3));

		// Bob got all 3 deposits back (3 × 10 = 30)
		let _proposal_deposit = 10;
		let _proposal_fee = 5;
		assert_eq!(Balances::reserved_balance(bob()), 0);
		// Initial 1000 - (3 deposits still reserved before cleanup) - (3 fees lost) = back to
		// initial - 3 fees After cleanup: initial - 3 fees
	});
}

// ==================== CLAIM DEPOSITS TESTS ====================

#[test]
fn claim_deposits_removes_expired_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let proposer = bob();
		let initial_balance = Balances::free_balance(proposer);
		let proposal_deposit = 10;
		let proposal_fee = 5;

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};

		// Create 3 proposals with same expiry
		let call1 = make_call(vec![1, 2, 3]);
		let call2 = make_call(vec![4, 5, 6]);
		let call3 = make_call(vec![7, 8, 9]);
		let expiry = 100;

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call1.clone(),
			expiry
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call2.clone(),
			expiry
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(proposer),
			multisig_address,
			call3.clone(),
			expiry
		));

		// After proposals: 3 deposits reserved, 3 fees lost
		assert_eq!(Balances::reserved_balance(proposer), 3 * proposal_deposit);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - 3 * proposal_deposit - 3 * proposal_fee
		);

		// Move past expiry + grace period
		let grace_period = 100;
		System::set_block_number(expiry + grace_period + 1);

		// Claim all deposits at once
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(proposer), multisig_address));

		// All deposits returned
		assert_eq!(Balances::reserved_balance(proposer), 0);
		assert_eq!(
			Balances::free_balance(proposer),
			initial_balance - 3 * proposal_fee // Only fees lost
		);

		// All proposals removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call1)));
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call2)));
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call3)));
	});
}

#[test]
fn claim_deposits_only_cleans_own_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let expiry = 100;

		// Bob creates 2 proposals
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			vec![1, 2, 3],
			expiry
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			vec![4, 5, 6],
			expiry
		));

		// Charlie creates 1 proposal
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(charlie()),
			multisig_address,
			vec![7, 8, 9],
			expiry
		));

		// Move past grace period
		let grace_period = 100;
		System::set_block_number(expiry + grace_period + 1);

		let _bob_initial = Balances::free_balance(bob());
		let _charlie_initial = Balances::free_balance(charlie());

		// Bob claims - should only get his 2 deposits back
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(bob()), multisig_address));

		// Bob: 2 deposits returned
		assert_eq!(Balances::reserved_balance(bob()), 0);

		// Charlie: still has 1 deposit reserved
		assert_eq!(Balances::reserved_balance(charlie()), 10);

		// Charlie claims his
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(charlie()), multisig_address));

		// Charlie: deposit returned
		assert_eq!(Balances::reserved_balance(charlie()), 0);
	});
}

#[test]
fn claim_deposits_respects_grace_period() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let expiry = 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			vec![1, 2, 3],
			expiry
		));

		// Move to within grace period
		System::set_block_number(expiry + 50); // grace is 100

		// Claim during grace - should not remove anything
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(bob()), multisig_address));

		// Deposit still reserved (nothing was cleaned)
		assert_eq!(Balances::reserved_balance(bob()), 10);

		// Move past grace period
		System::set_block_number(expiry + 101);

		// Claim after grace - should work
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(bob()), multisig_address));

		// Deposit returned
		assert_eq!(Balances::reserved_balance(bob()), 0);
	});
}

#[test]
fn claim_deposits_works_for_mixed_proposals() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);

		let creator = alice();
		let signers = vec![bob(), charlie()];
		assert_ok!(Multisig::create_multisig(RuntimeOrigin::signed(creator), signers.clone(), 2));

		let multisig_address = Multisig::derive_multisig_address(&signers, 0);

		let get_hash = |call: Vec<u8>| {
			use frame_support::BoundedVec;
			let bounded: BoundedVec<u8, <Test as crate::Config>::MaxCallSize> =
				call.try_into().unwrap();
			<Test as frame_system::Config>::Hashing::hash_of(&bounded)
		};

		// Create proposals with different expiries
		let call1 = make_call(vec![1, 2, 3]);
		let call2 = make_call(vec![4, 5, 6]);
		let call3 = make_call(vec![7, 8, 9]);

		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call1.clone(),
			100 // expires early
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call2.clone(),
			500 // expires late
		));
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(bob()),
			multisig_address,
			call3.clone(),
			100 // expires early
		));

		// Move past first expiry + grace (100 + 100 = 200)
		System::set_block_number(201);

		// Claim - should only clean expired proposals
		assert_ok!(Multisig::claim_deposits(RuntimeOrigin::signed(bob()), multisig_address));

		// 2 deposits returned (call1, call3 expired)
		assert_eq!(Balances::reserved_balance(bob()), 10); // 1 still reserved

		// call2 still exists (not expired yet)
		assert!(crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call2)));

		// call1, call3 removed
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call1)));
		assert!(!crate::Proposals::<Test>::contains_key(multisig_address, get_hash(call3)));
	});
}
