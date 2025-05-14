#[path = "common.rs"]
mod common;

#[cfg(test)]
mod tests {
    use crate::common::{account_id, new_test_ext, run_to_block};
    use codec::Encode;
    use frame_support::assert_ok;
    use frame_support::traits::Currency;
    use pallet_conviction_voting::AccountVote::Standard;
    use pallet_conviction_voting::Vote;
    use pallet_referenda::TracksInfo;
    use resonance_runtime::{AccountId, Balances, ConvictionVoting, OriginCaller, Preimage, Referenda, Runtime, RuntimeCall, RuntimeOrigin, UNIT};
    use sp_runtime::traits::{Hash, TransactionExtension};
    use sp_runtime::MultiAddress;

    #[test]
    fn test_add_member_referendum() {
        new_test_ext().execute_with(|| {
            let proposer = account_id(1);
            let voter = account_id(2);
            let new_member = account_id(3);

            // Ensure voters have enough balance
            Balances::make_free_balance_be(&voter, 1000 * UNIT);

            // Prepare the proposal
            let call = RuntimeCall::TechCollective(pallet_membership::Call::add_member {
                who: MultiAddress::Id(new_member.clone()),
            });

            // Encode and store preimage
            let encoded_call = call.encode();
            let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(proposer.clone()),
                encoded_call.clone()
            ));

            let bounded_call = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash,
                len: encoded_call.len() as u32,
            };

            // Submit referendum as a member (track 0)
            assert_ok!(Referenda::submit(
                RuntimeOrigin::signed(proposer.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let referendum_index = 0;

            // Place decision deposit to start deciding phase
            assert_ok!(Referenda::place_decision_deposit(
                RuntimeOrigin::signed(proposer.clone()),
                referendum_index
            ));

            // Vote FOR with high conviction
            assert_ok!(ConvictionVoting::vote(
                RuntimeOrigin::signed(voter.clone()),
                referendum_index,
                Standard {
                    vote: Vote {
                        aye: true,
                        conviction: pallet_conviction_voting::Conviction::Locked6x,
                    },
                    balance: 800 * UNIT,
                }
            ));

            // Get track info for proper period calculation
            let track_info = <Runtime as pallet_referenda::Config>::Tracks::info(0).unwrap();
            let prepare_period = track_info.prepare_period;
            let decision_period = track_info.decision_period;
            let confirm_period = track_info.confirm_period;

            // Advance to deciding phase
            run_to_block(prepare_period + 1);

            // Verify referendum is in deciding phase
            let info =
                pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
            if let pallet_referenda::ReferendumInfo::Ongoing(status) = info {
                assert!(
                    status.deciding.is_some(),
                    "Referendum should be in deciding phase"
                );
                assert!(
                    status.tally.ayes > status.tally.nays,
                    "Ayes should be winning"
                );
            } else {
                panic!("Referendum should be ongoing");
            }

            // Advance through decision and confirmation periods
            run_to_block(prepare_period + decision_period + confirm_period + 2);

            // Verify referendum passed
            let info =
                pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
            assert!(
                matches!(info, pallet_referenda::ReferendumInfo::Approved(_, _, _)),
                "Referendum should be approved"
            );

            // Verify the member was added
            assert!(
                pallet_membership::Members::<Runtime>::get().contains(&new_member),
                "New member should be added to the membership list"
            );
        });
    }

    // Helper functions (place inside mod tests in governance_tech_community.rs)

    /// Creates and passes a referendum to completion, voting "aye" with the provided vote.
    fn create_and_pass_referendum(
        proposer: AccountId,
        voter: AccountId, // Who votes for the referendum (must have permissions for track 0)
        call_to_propose: RuntimeCall,
        track_id: u16, // Usually 0 for TechCollective operations
    ) -> u32 { // Returns referendum_index
        // Encode and store preimage
        let encoded_call = call_to_propose.encode();
        let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);

        assert_ok!(Preimage::note_preimage(
            RuntimeOrigin::signed(proposer.clone()),
            encoded_call.clone()
        ));

        let bounded_call = frame_support::traits::Bounded::Lookup {
            hash: preimage_hash,
            len: encoded_call.len() as u32,
        };

        // Determine proposal origin based on track_id
        let proposal_origin = match track_id {
            0 => Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
            // Other tracks can be added if needed
            _ => panic!("Unsupported track_id for proposal_origin in helper: {}", track_id),
        };

        // The transaction submitter can be anyone with sufficient funds,
        // but for track 0, if `proposal_origin` is Root, `TechCollectiveExtension` will check
        // if `proposer` (as `ensure_signed`) is a member or root.
        // We assume `proposer` in this helper has permissions to create on the given track.
        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(proposer.clone()),
            proposal_origin,
            bounded_call,
            frame_support::traits::schedule::DispatchTime::After(0u32.into())
        ));

        let referendum_index = pallet_referenda::ReferendumCount::<Runtime>::get() - 1; // Last added referendum

        // Place decision deposit
        assert_ok!(Referenda::place_decision_deposit(
            RuntimeOrigin::signed(proposer.clone()),
            referendum_index
        ));

        // Vote FOR
        assert_ok!(ConvictionVoting::vote(
            RuntimeOrigin::signed(voter.clone()), // Voter must have permissions for track 0
            referendum_index,
            Standard {
                vote: Vote {
                    aye: true,
                    conviction: pallet_conviction_voting::Conviction::Locked6x,
                },
                balance: 800 * UNIT, // Enough to pass
            },
        ));

        // Fast-forward time for the referendum to pass
        let track_info = <Runtime as pallet_referenda::Config>::Tracks::info(track_id).unwrap();
        let prepare_period = track_info.prepare_period;
        let decision_period = track_info.decision_period;
        let confirm_period = track_info.confirm_period;
        let min_enactment_period = track_info.min_enactment_period;

        run_to_block(prepare_period + decision_period + confirm_period + min_enactment_period + 5); // +5 for certainty

        // Check if the referendum was approved
        let info = pallet_referenda::ReferendumInfoFor::<Runtime>::get(referendum_index).unwrap();
        assert!(
            matches!(info, pallet_referenda::ReferendumInfo::Approved(_, _, _)),
            "Referendum {} should be approved, but is: {:?}", referendum_index, info
        );

        referendum_index
    }

    /// Helper for validation using TechCollectiveExtension
    fn validate_with_extension(
        extension: &resonance_runtime::transaction_extensions::TechCollectiveExtension<Runtime>,
        tx_origin: RuntimeOrigin,
        call: &RuntimeCall,
    ) -> Result<(frame_support::pallet_prelude::ValidTransaction, (), RuntimeOrigin), frame_support::pallet_prelude::TransactionValidityError> {
        extension.validate(
            tx_origin,
            call,
            &Default::default(),
            0,
            (),
            &sp_runtime::traits::TxBaseImplication::<()>(()),
            frame_support::pallet_prelude::TransactionSource::External,
        )
    }

    // Test Case 1: Remove Member via Referendum
    #[test]
    fn test_remove_member_via_referendum_and_check_permissions() {
        new_test_ext().execute_with(|| {
            let admin_proposer = account_id(1); // Assume admin_proposer is root or a member
            let member_to_remove = account_id(2);
            let another_member_voter = account_id(3); // For voting

            // Add admin and voter to TechCollective so they can operate on track 0
            assert_ok!(resonance_runtime::TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::Id(admin_proposer.clone())));
            assert_ok!(resonance_runtime::TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::Id(another_member_voter.clone())));
            Balances::make_free_balance_be(&admin_proposer, 1000 * UNIT);
            Balances::make_free_balance_be(&another_member_voter, 1000 * UNIT);
            Balances::make_free_balance_be(&member_to_remove, 1000 * UNIT);


            // Add the member we are going to remove
            assert_ok!(resonance_runtime::TechCollective::add_member(
                RuntimeOrigin::root(), // Always use Root for direct additions in setup
                MultiAddress::Id(member_to_remove.clone())
            ));
            assert!(pallet_membership::Members::<Runtime>::get().contains(&member_to_remove));

            // Prepare call to remove member
            let remove_call = RuntimeCall::TechCollective(pallet_membership::Call::remove_member {
                who: MultiAddress::Id(member_to_remove.clone()),
            });

            // Create and pass referendum on track 0
            create_and_pass_referendum(admin_proposer.clone(), another_member_voter.clone(), remove_call, 0);

            // Check if member was removed
            assert!(!pallet_membership::Members::<Runtime>::get().contains(&member_to_remove), "Member should have been removed");

            // Prepare to create another referendum (for testing voting)
            let remark_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1]});
            let encoded_remark_call = remark_call.encode();
            let remark_preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_remark_call);
            assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(admin_proposer.clone()), encoded_remark_call.clone()));
            let remark_bounded_call = frame_support::traits::Bounded::Lookup { hash: remark_preimage_hash, len: encoded_remark_call.len() as u32 };
            assert_ok!(Referenda::submit(
                RuntimeOrigin::signed(admin_proposer.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                remark_bounded_call.clone(),
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));
            let some_other_referendum_idx = pallet_referenda::ReferendumCount::<Runtime>::get() - 1;
             assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(admin_proposer.clone()), some_other_referendum_idx));


            let vote_call = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote {
                poll_index: some_other_referendum_idx,
                vote: pallet_conviction_voting::AccountVote::Standard {
                    vote: Vote { aye: true, conviction: pallet_conviction_voting::Conviction::None },
                    balance: 1 * UNIT,
                },
            });
            let extension = resonance_runtime::transaction_extensions::TechCollectiveExtension::<Runtime>::new();
            let validation_result_vote = validate_with_extension(
                &extension,
                RuntimeOrigin::signed(member_to_remove.clone()),
                &vote_call
            );
            assert!(validation_result_vote.is_err(), "Removed member should not be able to vote on track 0");
            if let Err(frame_support::pallet_prelude::TransactionValidityError::Invalid(frame_support::pallet_prelude::InvalidTransaction::Custom(code))) = validation_result_vote {
                assert_eq!(code, 42, "Expected error code 42 for voting by removed member");
            } else {
                panic!("Expected InvalidTransaction::Custom(42) for voting, got: {:?}", validation_result_vote);
            }


            // Check if removed member can create a new referendum on track 0
            let submit_call_track_0 = RuntimeCall::Referenda(pallet_referenda::Call::submit {
                proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                proposal: remark_bounded_call.clone(), // Reuse the same proposal for simplicity
                enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()),
            });
            let validation_result_submit = validate_with_extension(
                &extension,
                RuntimeOrigin::signed(member_to_remove.clone()),
                &submit_call_track_0
            );
            assert!(validation_result_submit.is_err(), "Removed member should not be able to create referendum on track 0");
            if let Err(frame_support::pallet_prelude::TransactionValidityError::Invalid(frame_support::pallet_prelude::InvalidTransaction::Custom(code))) = validation_result_submit {
                assert_eq!(code, 43, "Expected error code 43 for creating referendum by removed member");
            } else {
                panic!("Expected InvalidTransaction::Custom(43) for creation, got: {:?}", validation_result_submit);
            }
        });
    }

    // Test Case 3: Swap Member via Referendum
    #[test]
    fn test_swap_member_via_referendum_and_check_permissions() {
        new_test_ext().execute_with(|| {
            let admin_proposer = account_id(1);
            let old_member = account_id(2);
            let new_member_for_swap = account_id(3); // Renamed to avoid conflict
            let voter = account_id(4); // For voting

            assert_ok!(resonance_runtime::TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::Id(admin_proposer.clone())));
            assert_ok!(resonance_runtime::TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::Id(voter.clone())));
            Balances::make_free_balance_be(&admin_proposer, 1000 * UNIT);
            Balances::make_free_balance_be(&old_member, 1000 * UNIT);
            Balances::make_free_balance_be(&new_member_for_swap, 1000 * UNIT);
            Balances::make_free_balance_be(&voter, 1000 * UNIT);


            // Add old_member
            assert_ok!(resonance_runtime::TechCollective::add_member(
                RuntimeOrigin::root(), // Always use Root for direct additions in setup
                MultiAddress::Id(old_member.clone())
            ));
            assert!(pallet_membership::Members::<Runtime>::get().contains(&old_member));
            assert!(!pallet_membership::Members::<Runtime>::get().contains(&new_member_for_swap));

            // Prepare call to swap members
            let swap_call = RuntimeCall::TechCollective(pallet_membership::Call::swap_member {
                remove: MultiAddress::Id(old_member.clone()),
                add: MultiAddress::Id(new_member_for_swap.clone()),
            });

            create_and_pass_referendum(admin_proposer.clone(), voter.clone(), swap_call, 0);

            // Check membership
            assert!(!pallet_membership::Members::<Runtime>::get().contains(&old_member), "Old member should have been removed");
            assert!(pallet_membership::Members::<Runtime>::get().contains(&new_member_for_swap), "New member should have been added");

            let extension = resonance_runtime::transaction_extensions::TechCollectiveExtension::<Runtime>::new();

            // Prepare to create another referendum (for testing voting/submit)
            let remark_call_swap = RuntimeCall::System(frame_system::Call::remark { remark: vec![2]});
            let encoded_remark_call_swap = remark_call_swap.encode();
            let remark_preimage_hash_swap = <Runtime as frame_system::Config>::Hashing::hash(&encoded_remark_call_swap);
            assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(admin_proposer.clone()), encoded_remark_call_swap.clone()));
            let remark_bounded_call_swap = frame_support::traits::Bounded::Lookup { hash: remark_preimage_hash_swap, len: encoded_remark_call_swap.len() as u32 };
            assert_ok!(Referenda::submit(
                RuntimeOrigin::signed(admin_proposer.clone()), // or new_member_for_swap, if they can already
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                remark_bounded_call_swap.clone(),
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));
            let some_other_referendum_idx_swap = pallet_referenda::ReferendumCount::<Runtime>::get() - 1;
            assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(admin_proposer.clone()), some_other_referendum_idx_swap));


            // Check old_member permissions (should lose them)
            let vote_call_old = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote { poll_index: some_other_referendum_idx_swap, vote: pallet_conviction_voting::AccountVote::Standard { vote: Vote { aye: true, conviction: pallet_conviction_voting::Conviction::None }, balance: 1 * UNIT } });
            let submit_call_old = RuntimeCall::Referenda(pallet_referenda::Call::submit { proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Root)), proposal: remark_bounded_call_swap.clone(), enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()) });

            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(old_member.clone()), &vote_call_old).is_err(), "Old_member should not be able to vote");
            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(old_member.clone()), &submit_call_old).is_err(), "Old_member should not be able to create referendum");

            // Check new_member_for_swap permissions (should gain them)
            let vote_call_new = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote { poll_index: some_other_referendum_idx_swap, vote: pallet_conviction_voting::AccountVote::Standard { vote: Vote { aye: true, conviction: pallet_conviction_voting::Conviction::None }, balance: 1 * UNIT } });
            let submit_call_new = RuntimeCall::Referenda(pallet_referenda::Call::submit { proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Root)), proposal: remark_bounded_call_swap.clone(), enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()) });

            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(new_member_for_swap.clone()), &vote_call_new).is_ok(), "New_member_for_swap should be able to vote");
            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(new_member_for_swap.clone()), &submit_call_new).is_ok(), "New_member_for_swap should be able to create referendum");
        });
    }


    // Test Case 4: Non-TechCollective Call on Track 0 (Permission Check by Extension)
    #[test]
    fn test_non_tech_collective_call_on_track_0_permissions() {
        new_test_ext().execute_with(|| {
            let charlie_member = account_id(1);
            let bob_non_member = account_id(2);
            let root_tx_origin = RuntimeOrigin::root(); // Transaction origin as Root

            assert_ok!(resonance_runtime::TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::Id(charlie_member.clone())));
            Balances::make_free_balance_be(&charlie_member, 1000 * UNIT);
            Balances::make_free_balance_be(&bob_non_member, 1000 * UNIT);


            let extension = resonance_runtime::transaction_extensions::TechCollectiveExtension::<Runtime>::new();

            // Prepare a call not related to TechCollective (e.g., System::remark)
            // but we want to submit it on track 0 (proposal_origin = Root)
            let remark_data = b"remark_data_for_track0_test".to_vec();
            let remark_preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&remark_data);
            let remark_proposal_bounded = frame_support::traits::Bounded::Lookup {
                hash: remark_preimage_hash,
                len: remark_data.len() as u32
            };
            // Note the preimage for remark_proposal_bounded
            assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(charlie_member.clone()), remark_data.clone()));


            let submit_remark_on_track_0 = RuntimeCall::Referenda(pallet_referenda::Call::submit {
                proposal_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                proposal: remark_proposal_bounded.clone(),
                enactment_moment: frame_support::traits::schedule::DispatchTime::After(10u32.into()),
            });

            // TechCollective member should be able to create such a referendum
            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(charlie_member.clone()), &submit_remark_on_track_0).is_ok(),
                "Member should be able to create System::remark referendum on track 0");

            // Non-member should not be able to create such a referendum
            let validation_bob_submit = validate_with_extension(&extension, RuntimeOrigin::signed(bob_non_member.clone()), &submit_remark_on_track_0);
            assert!(validation_bob_submit.is_err(), "Non-member should not be able to create System::remark referendum on track 0");
            if let Err(frame_support::pallet_prelude::TransactionValidityError::Invalid(frame_support::pallet_prelude::InvalidTransaction::Custom(code))) = validation_bob_submit {
                assert_eq!(code, 43, "Expected error code 43 for creation by non-member");
            } else {
                 panic!("Expected InvalidTransaction::Custom(43) for creation by non-member, got: {:?}", validation_bob_submit);
            }

            // Root (as transaction origin) should be able to create such a referendum
            assert!(validate_with_extension(&extension, root_tx_origin.clone(), &submit_remark_on_track_0).is_ok(),
                "Root should be able to create System::remark referendum on track 0");

            // Let's create this referendum now (e.g., by a member) to test voting
             assert_ok!(Referenda::submit(
                RuntimeOrigin::signed(charlie_member.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                remark_proposal_bounded.clone(),
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));
            let referendum_idx = pallet_referenda::ReferendumCount::<Runtime>::get() - 1;
            assert_ok!(Referenda::place_decision_deposit(RuntimeOrigin::signed(charlie_member.clone()), referendum_idx));


            let vote_on_remark_referendum = RuntimeCall::ConvictionVoting(pallet_conviction_voting::Call::vote {
                poll_index: referendum_idx,
                vote: pallet_conviction_voting::AccountVote::Standard {
                    vote: Vote { aye: true, conviction: pallet_conviction_voting::Conviction::None },
                    balance: 1 * UNIT,
                },
            });

            // TechCollective member should be able to vote
            assert!(validate_with_extension(&extension, RuntimeOrigin::signed(charlie_member.clone()), &vote_on_remark_referendum).is_ok(),
                "Member should be able to vote on System::remark referendum on track 0");

            // Non-member should not be able to vote
            let validation_bob_vote = validate_with_extension(&extension, RuntimeOrigin::signed(bob_non_member.clone()), &vote_on_remark_referendum);
            assert!(validation_bob_vote.is_err(), "Non-member should not be able to vote on System::remark referendum on track 0");
            if let Err(frame_support::pallet_prelude::TransactionValidityError::Invalid(frame_support::pallet_prelude::InvalidTransaction::Custom(code))) = validation_bob_vote {
                assert_eq!(code, 42, "Expected error code 42 for voting by non-member");
            } else {
                panic!("Expected InvalidTransaction::Custom(42) for voting by non-member, got: {:?}", validation_bob_vote);
            }
        });
    }
}
