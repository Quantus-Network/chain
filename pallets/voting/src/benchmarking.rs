//! Benchmarking setup for pallet-voting

use super::*;

#[allow(unused)]
use crate::Pallet as Voting;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
extern crate alloc;
use alloc::{vec, vec::Vec};
use frame_support::BoundedVec;

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn register_proposal() {
        let caller: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];

        #[extrinsic_call]
        register_proposal(
            RawOrigin::Signed(caller), // Changed from Root
            merkle_root,
        );

        // Verify that a proposal was created
        assert!(Proposals::<T>::get(0).is_some());
    }

    #[benchmark]
    fn vote() {
        // --- Setup ---
        // 1. Create a proposal to vote on.
        let creator: T::AccountId = whitelisted_caller();
        let merkle_root = [0u8; 32];
        let proposal_id = NextProposalId::<T>::get();

        let proposal = Proposal {
            creator,
            merkle_root,
            yes_votes: 0,
            no_votes: 0,
        };
        Proposals::<T>::insert(proposal_id, proposal);
        NextProposalId::<T>::put(proposal_id + 1);

        // 2. Prepare vote parameters
        let vote_decision = Vote::Yes;
        let nullifier = [1u8; 32];
        let proof_vec: Vec<u8> = vec![0; 128]; // dummy proof
        let proof: BoundedVec<u8, T::MaxProofLength> =
            proof_vec.try_into().expect("Proof should be within bounds");

        // --- Measurement ---
        #[extrinsic_call]
        vote(
            RawOrigin::None,
            proposal_id,
            vote_decision,
            nullifier,
            proof,
        );

        // --- Verification ---
        assert_eq!(Proposals::<T>::get(proposal_id).unwrap().yes_votes, 1);
        assert!(UsedNullifiers::<T>::contains_key(proposal_id, nullifier));
    }

    impl_benchmark_test_suite!(Voting, crate::mock::new_test_ext(), crate::mock::Test);
}
