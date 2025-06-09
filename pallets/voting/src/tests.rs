//! Tests for pallet-voting.

use crate::{mock::*, Error, Event, Proposal, UsedNullifiers, Vote};
use frame_support::{assert_noop, assert_ok};

#[test]
fn register_proposal_works() {
    new_test_ext().execute_with(|| {
        let merkle_root = [0u8; 32];
        let creator_account = 1;

        assert_ok!(Voting::register_proposal(
            RuntimeOrigin::signed(creator_account),
            merkle_root
        ));

        let proposal = Voting::proposals(0).unwrap();
        assert_eq!(
            proposal,
            Proposal {
                creator: creator_account,
                merkle_root,
                yes_votes: 0,
                no_votes: 0,
            }
        );

        assert_eq!(Voting::next_proposal_id(), 1);

        System::assert_last_event(
            Event::ProposalRegistered {
                proposal_id: 0,
                creator: creator_account,
                merkle_root,
            }
            .into(),
        );
    });
}

#[test]
fn register_proposal_fails_for_unsigned() {
    new_test_ext().execute_with(|| {
        let merkle_root = [0u8; 32];
        assert_noop!(
            Voting::register_proposal(RuntimeOrigin::none(), merkle_root),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

#[test]
fn vote_works() {
    new_test_ext().execute_with(|| {
        // 1. Register a proposal first
        let merkle_root = [0u8; 32];
        assert_ok!(Voting::register_proposal(
            RuntimeOrigin::signed(1),
            merkle_root
        ));

        // 2. Cast a vote
        let proposal_id = 0;
        let vote = Vote::Yes;
        let nullifier = [1u8; 32];
        let proof = vec![0; 128].try_into().unwrap();

        assert_ok!(Voting::vote(
            RuntimeOrigin::none(),
            proposal_id,
            vote,
            nullifier,
            proof
        ));

        // 3. Verify state changes
        let proposal = Voting::proposals(proposal_id).unwrap();
        assert_eq!(proposal.yes_votes, 1);
        assert_eq!(proposal.no_votes, 0);
        assert!(UsedNullifiers::<Test>::contains_key(proposal_id, nullifier));

        System::assert_last_event(Event::Voted { proposal_id, vote }.into());
    });
}

#[test]
fn vote_fails_if_nullifier_used() {
    new_test_ext().execute_with(|| {
        // 1. Register a proposal and cast one vote
        let merkle_root = [0u8; 32];
        assert_ok!(Voting::register_proposal(
            RuntimeOrigin::signed(1),
            merkle_root
        ));
        let proposal_id = 0;
        let nullifier = [1u8; 32];
        let proof = vec![0; 128].try_into().unwrap();
        assert_ok!(Voting::vote(
            RuntimeOrigin::none(),
            proposal_id,
            Vote::Yes,
            nullifier,
            proof
        ));

        // 2. Attempt to vote again with the same nullifier
        let proof_2 = vec![0; 128].try_into().unwrap();
        assert_noop!(
            Voting::vote(
                RuntimeOrigin::none(),
                proposal_id,
                Vote::No,
                nullifier,
                proof_2
            ),
            Error::<Test>::NullifierAlreadyUsed
        );
    });
}

#[test]
fn vote_fails_if_proposal_not_found() {
    new_test_ext().execute_with(|| {
        let non_existent_proposal_id = 99;
        let vote = Vote::Yes;
        let nullifier = [1u8; 32];
        let proof = vec![0; 128].try_into().unwrap();

        assert_noop!(
            Voting::vote(
                RuntimeOrigin::none(),
                non_existent_proposal_id,
                vote,
                nullifier,
                proof
            ),
            Error::<Test>::ProposalNotFound
        );
    });
}

#[test]
fn vote_fails_if_not_unsigned() {
    new_test_ext().execute_with(|| {
        assert_ok!(Voting::register_proposal(
            RuntimeOrigin::signed(1),
            [0u8; 32]
        ));

        let proposal_id = 0;
        let vote = Vote::Yes;
        let nullifier = [1u8; 32];
        let proof = vec![0; 128].try_into().unwrap();

        assert_noop!(
            Voting::vote(
                RuntimeOrigin::signed(2), // a signed origin
                proposal_id,
                vote,
                nullifier,
                proof
            ),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

// TODO: Add test for `InvalidProof` when `verify_proof` is mockable.
