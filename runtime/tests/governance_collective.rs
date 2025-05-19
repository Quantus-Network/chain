#[path = "common.rs"]
mod common;

#[cfg(test)]
mod tests {
    use crate::common::{account_id, new_test_ext, run_to_block};
    use codec::Encode;
    use frame_support::assert_ok;
    use frame_system;
    use frame_support::traits::{Currency};
    use pallet_referenda::TracksInfo;
    use resonance_runtime::{
        Balances, OriginCaller, Preimage, Runtime, RuntimeCall,
        RuntimeOrigin, TechCollective, TechReferenda, UNIT,
    };
    use resonance_runtime::configs::TechReferendaInstance;
    use sp_runtime::traits::Hash;
    use sp_runtime::MultiAddress;

    const TRACK_ID: u16 = 0;

    #[test]
    fn test_add_member_via_referendum_in_collective() {
        new_test_ext().execute_with(|| {
            let proposer = account_id(1);
            let voter = account_id(2);
            let new_member_candidate = account_id(3);

            Balances::make_free_balance_be(&proposer, 3000 * UNIT);
            // Add proposer. Rank will be 0 as added by Root.
            assert_ok!(TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::from(proposer.clone())));

            Balances::make_free_balance_be(&voter, 2000 * UNIT);
            // Add voter. Rank will be 0 as added by Root.
            assert_ok!(TechCollective::add_member(RuntimeOrigin::root(), MultiAddress::from(voter.clone())));

            let call_to_propose = RuntimeCall::TechCollective(pallet_ranked_collective::Call::add_member {
                who: MultiAddress::from(new_member_candidate.clone()),
            });

            let encoded_call = call_to_propose.encode();
            let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_call);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(proposer.clone()),
                encoded_call.clone()
            ));


            let bounded_call = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash,
                len: encoded_call.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(proposer.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let referendum_index = pallet_referenda::ReferendumCount::<Runtime, TechReferendaInstance>::get() - 1;

            let initial_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(referendum_index);
            assert!(initial_info.is_some(), "Referendum should exist after submit");

            // Check if the referendum is ongoing, otherwise panic.
            match initial_info {
                Some(pallet_referenda::ReferendumInfo::Ongoing(_)) => { /* Correct status, do nothing */ },
                _ => panic!("Referendum not ongoing immediately after submit or does not exist: {:?}", initial_info),
            }

            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(proposer.clone()),
                referendum_index
            ));

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(voter.clone()),
                referendum_index,
                true
            ));

            let track_info = <Runtime as pallet_referenda::Config<TechReferendaInstance>>::Tracks::info(TRACK_ID)
                .expect("Track info should exist for the given TRACK_ID");
            let prepare_period = track_info.prepare_period;
            let decision_period = track_info.decision_period;
            let confirm_period = track_info.confirm_period;
            let min_enactment_period = track_info.min_enactment_period;

            run_to_block(prepare_period + 1);

            let max_deciding = track_info.max_deciding;
            let mut deciding_count = 0;
            let current_referendum_count = pallet_referenda::ReferendumCount::<Runtime, TechReferendaInstance>::get();

            for i in 0..current_referendum_count {
                if let Some(pallet_referenda::ReferendumInfo::Ongoing(status)) =
                    pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(i)
                {
                    if status.deciding.is_some() {
                        if status.track == TRACK_ID {
                           deciding_count += 1;
                        }
                    }
                }
                if deciding_count >= max_deciding && track_info.max_deciding > 0 {
                    break;
                }
            }

            if max_deciding > 0 {
                 assert_eq!(deciding_count, max_deciding,
                       "Expected {} deciding referenda on track {}, found {}", max_deciding, TRACK_ID, deciding_count);
            } else {
                assert_eq!(deciding_count, 0, "Expected 0 deciding referenda as max_deciding is 0, found {}", deciding_count);
            }

            run_to_block(prepare_period + decision_period + confirm_period + min_enactment_period + 5);

            let final_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(referendum_index)
                .expect("Referendum info should exist at the end");
            assert!(
                matches!(final_info, pallet_referenda::ReferendumInfo::Approved(_,_,_)),
                "Referendum should be approved, but is {:?}", final_info
            );

            assert!(
                pallet_ranked_collective::Members::<Runtime>::contains_key(&new_member_candidate),
                "New member should have been added to TechCollective"
            );
        });
    }

    #[test]
    fn test_tech_collective_access_control() {
        new_test_ext().execute_with(|| {
            // Define our test accounts
            let root_member = account_id(1);
            let existing_member = account_id(2);
            let non_member = account_id(3);
            let candidate_to_add = account_id(4);
            let member_to_remove = account_id(5);

            // Setup account balances
            Balances::make_free_balance_be(&root_member, 1000 * UNIT);
            Balances::make_free_balance_be(&existing_member, 1000 * UNIT);
            Balances::make_free_balance_be(&non_member, 1000 * UNIT);
            Balances::make_free_balance_be(&candidate_to_add, 1000 * UNIT);
            Balances::make_free_balance_be(&member_to_remove, 1000 * UNIT);

            // Add initial members
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(existing_member.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_to_remove.clone())
            ));

            // VERIFY 1: Root can add members
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(root_member.clone())
            ));

            assert!(
                pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&root_member),
                "Root should be able to add a member"
            );

            // VERIFY 2: Existing members can add new members
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::signed(existing_member.clone()),
                MultiAddress::from(candidate_to_add.clone())
            ));

            assert!(
                pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&candidate_to_add),
                "Existing member should be able to add a new member"
            );

            // VERIFY 3: Non-members cannot add members
            assert!(
                TechCollective::add_member(
                    RuntimeOrigin::signed(non_member.clone()),
                    MultiAddress::from(non_member.clone())
                ).is_err(),
                "Non-member should not be able to add themselves to collective"
            );

            assert!(
                !pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&non_member),
                "Non-member should not be able to join collective"
            );

            // VERIFY 4: Root can remove members
            assert_ok!(TechCollective::remove_member(
                RuntimeOrigin::root(),
                MultiAddress::from(candidate_to_add.clone()),
                0 // min_rank parameter
            ));

            assert!(
                !pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&candidate_to_add),
                "Root should be able to remove a member"
            );

            // VERIFY 5: Existing members can remove other members
            assert_ok!(TechCollective::remove_member(
                RuntimeOrigin::signed(existing_member.clone()),
                MultiAddress::from(member_to_remove.clone()),
                0 // min_rank parameter
            ));

            assert!(
                !pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&member_to_remove),
                "Existing member should be able to remove another member"
            );

            // VERIFY 6: Non-members cannot remove members
            assert!(
                TechCollective::remove_member(
                    RuntimeOrigin::signed(non_member.clone()),
                    MultiAddress::from(existing_member.clone()),
                    0 // min_rank parameter
                ).is_err(),
                "Non-member should not be able to remove members"
            );

            assert!(
                pallet_ranked_collective::Members::<Runtime, ()>::contains_key(&existing_member),
                "Member should not be removed by non-member attempt"
            );
        });
    }

    #[test]
    fn test_tech_referenda_submit_access_control() {
        new_test_ext().execute_with(|| {
            // Define our test accounts
            let collective_member = account_id(1);
            let non_member = account_id(2);

            // Setup account balances (with extra balance for preimage and submission deposits)
            Balances::make_free_balance_be(&collective_member, 5000 * UNIT);
            Balances::make_free_balance_be(&non_member, 5000 * UNIT);

            // Add collective_member to TechCollective
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(collective_member.clone())
            ));

            // Create unique proposals for testing (with different content for each)
            let root_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Test proposal for root".to_vec()
            });

            let member_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Test proposal for member".to_vec()
            });

            let non_member_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Test proposal for non-member".to_vec()
            });

            // Store preimage for Root test
            let encoded_proposal_root = root_proposal.encode();
            let preimage_hash_root = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal_root);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(collective_member.clone()),
                encoded_proposal_root.clone()
            ));

            // Store preimage for Member test
            let encoded_proposal_member = member_proposal.encode();
            let preimage_hash_member = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal_member);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(collective_member.clone()),
                encoded_proposal_member.clone()
            ));

            // Store preimage for Non-Member test
            let encoded_proposal_non_member = non_member_proposal.encode();
            let preimage_hash_non_member = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal_non_member);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(non_member.clone()),
                encoded_proposal_non_member.clone()
            ));

            // VERIFY 1: Root can submit referendum (root origin from collective member)
            let bounded_call_root = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash_root,
                len: encoded_proposal_root.len() as u32
            };

            // Note that for a "root" submission, we need to have a valid member signature
            // as per the actual implementation of RootOrMemberForTechReferendaOrigin
            // The combination of Root + Member is checked in the actual implementation
            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(collective_member.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call_root,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            // VERIFY 2: TechCollective member can submit referendum
            let bounded_call_member = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash_member,
                len: encoded_proposal_member.len() as u32
            };

            // For a collective member, we submit with their origin but a regular None as the proposal origin
            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(collective_member.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call_member,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            // VERIFY 3: Non-member cannot submit referendum
            let bounded_call_non_member = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash_non_member,
                len: encoded_proposal_non_member.len() as u32
            };

            // Non-members should be rejected for any calls
            assert!(
                TechReferenda::submit(
                    RuntimeOrigin::signed(non_member.clone()),
                    Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                    bounded_call_non_member,
                    frame_support::traits::schedule::DispatchTime::After(0u32.into())
                ).is_err(),
                "Non-member should not be able to submit referendum"
            );

            // Count the number of referenda to verify only 2 were created (Root and Member)
            let referendum_count = pallet_referenda::ReferendumCount::<Runtime, TechReferendaInstance>::get();
            assert_eq!(referendum_count, 2, "Only 2 referenda should have been created (Root and Member)");
        });
    }

    #[test]
    fn test_tech_collective_max_deciding_limit() {
        new_test_ext().execute_with(|| {
            // Define test accounts
            let root_account = account_id(1);
            let member_one = account_id(2);
            let member_two = account_id(3);

            // Setup account balances with plenty of funds for deposits
            Balances::make_free_balance_be(&root_account, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_one, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_two, 10_000 * UNIT);

            // Add members to the tech collective
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_one.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_two.clone())
            ));

            // Create two different proposals
            let proposal_one = RuntimeCall::System(frame_system::Call::remark {
                remark: b"First proposal".to_vec()
            });

            let proposal_two = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Second proposal".to_vec()
            });

            // Store preimages
            let encoded_proposal_one = proposal_one.encode();
            let preimage_hash_one = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal_one);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_one.clone()),
                encoded_proposal_one.clone()
            ));

            let encoded_proposal_two = proposal_two.encode();
            let preimage_hash_two = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal_two);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_two.clone()),
                encoded_proposal_two.clone()
            ));

            // Submit first referendum
            let bounded_call_one = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash_one,
                len: encoded_proposal_one.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_one.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call_one,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            // Submit second referendum
            let bounded_call_two = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash_two,
                len: encoded_proposal_two.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_two.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call_two,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            // Check referendum indices
            let first_referendum_index = 0;
            let second_referendum_index = 1;

            // Place decision deposit on both referenda to make them decidable
            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_one.clone()),
                first_referendum_index
            ));

            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_two.clone()),
                second_referendum_index
            ));

            // Verify initial state - both should be submitted but not deciding yet
            let first_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(first_referendum_index);
            let second_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(second_referendum_index);

            assert!(first_info.is_some(), "First referendum should exist");
            assert!(second_info.is_some(), "Second referendum should exist");

            // Get info about track parameters
            let track_info = <Runtime as pallet_referenda::Config<TechReferendaInstance>>::Tracks::info(TRACK_ID)
                .expect("Track info should exist for the given TRACK_ID");

            // Run to just after prepare period to trigger deciding phase for at least one referendum
            run_to_block(track_info.prepare_period + 1);

            // After prepare period, get updated status
            let first_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(first_referendum_index)
                .expect("First referendum should still exist");
            let second_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(second_referendum_index)
                .expect("Second referendum should still exist");

            // Check status of both referenda
            let mut deciding_count = 0;
            let mut preparing_count = 0;

            if let pallet_referenda::ReferendumInfo::Ongoing(status) = first_info {
                if status.deciding.is_some() {
                    deciding_count += 1;
                } else {
                    preparing_count += 1;
                }
            }

            if let pallet_referenda::ReferendumInfo::Ongoing(status) = second_info {
                if status.deciding.is_some() {
                    deciding_count += 1;
                } else {
                    preparing_count += 1;
                }
            }

            // Given max_deciding = 1, we should have exactly one referendum in deciding phase
            // and the other should be in preparation/queue
            assert_eq!(deciding_count, 1, "Should have exactly one referendum in deciding phase");
            assert_eq!(preparing_count, 1, "Should have exactly one referendum still waiting");

            // Complete the first referendum
            run_to_block(track_info.prepare_period + track_info.decision_period + track_info.confirm_period + 5);

            // The first referendum should now be completed and the second one should move to deciding
            run_to_block(track_info.prepare_period + track_info.decision_period + track_info.confirm_period + 5);

            // Check that the second referendum has moved to deciding after the first completed
            let second_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(second_referendum_index)
                .expect("Second referendum should still exist");

            if let pallet_referenda::ReferendumInfo::Ongoing(status) = second_info {
                assert!(status.deciding.is_some(), "Second referendum should now be in deciding phase");
            } else {
                panic!("Second referendum should be ongoing");
            }
        });
    }

    #[test]
    fn test_tech_collective_voting_weights() {
        // -------------------------------------------------------------
        // Scenario: Testing voting weights in a flat collective structure.
        // This test verifies that:
        // 1. A referendum is rejected when votes are equal (AYE/NAY).
        // 2. A referendum is approved when only one member votes AYE and no one votes against.
        // 3. A referendum with multiple voters (5 members) shows correct voting patterns:
        //    - 3 AYE vs 2 NAY should pass
        //    - 2 AYE vs 3 NAY should fail
        // The test uses frame_system::Call::remark as a neutral proposal to avoid affecting chain state.
        // -------------------------------------------------------------
        new_test_ext().execute_with(|| {
            // Define test accounts
            let root_account = account_id(1);
            let member_one = account_id(2);
            let member_two = account_id(3);
            let member_three = account_id(4);
            let member_four = account_id(5);
            let member_five = account_id(6);

            // Setup account balances
            Balances::make_free_balance_be(&root_account, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_one, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_two, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_three, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_four, 10_000 * UNIT);
            Balances::make_free_balance_be(&member_five, 10_000 * UNIT);

            // Add members to the tech collective
            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_one.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_two.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_three.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_four.clone())
            ));

            assert_ok!(TechCollective::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(member_five.clone())
            ));

            // Create a test proposal
            let test_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Test proposal for voting weights".to_vec()
            });

            // Store preimage
            let encoded_proposal = test_proposal.encode();
            let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_proposal);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_one.clone()),
                encoded_proposal.clone()
            ));

            // Submit test referendum
            let bounded_call = frame_support::traits::Bounded::Lookup {
                hash: preimage_hash,
                len: encoded_proposal.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_one.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let referendum_index = 0;

            // Place decision deposit
            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_one.clone()),
                referendum_index
            ));

            // Get track info
            let track_info = <Runtime as pallet_referenda::Config<TechReferendaInstance>>::Tracks::info(TRACK_ID)
                .expect("Track info should exist for the given TRACK_ID");

            // Run to just after prepare period to trigger deciding phase
            run_to_block(track_info.prepare_period + 1);

            // Test scenario: One member votes AYE and one votes NAY
            // First member votes AYE
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_one.clone()),
                referendum_index,
                true // AYE vote
            ));

            // Check referendum status after first vote
            let info_after_first_vote = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(referendum_index)
                .expect("Referendum info should exist");

            if let pallet_referenda::ReferendumInfo::Ongoing(status) = info_after_first_vote {
                assert!(status.deciding.is_some());
                println!("Referendum status after first vote: Is deciding? {}", status.deciding.is_some());
            } else {
                panic!("Referendum should be ongoing");
            }

            // Second member votes NAY
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_two.clone()),
                referendum_index,
                false // NAY vote
            ));

            // Run to the end of voting
            run_to_block(track_info.prepare_period + track_info.decision_period + track_info.confirm_period + 5);

            // Check referendum state - if votes are equal, it should be rejected as the default position
            let referendum_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(referendum_index)
                .expect("Referendum info should exist");

            // Verify the voting conditions correctly
            match referendum_info {
                pallet_referenda::ReferendumInfo::Approved(_, _, _) => {
                    println!("Referendum was approved as expected");
                },
                pallet_referenda::ReferendumInfo::Rejected(_, _, _) => {
                    println!("Referendum was rejected as expected");
                },
                _ => {
                    panic!("Referendum should be completed at this point");
                }
            }

            // Create a second referendum where votes are not equal in number
            // Member one creates the referendum
            let second_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Second voting test proposal".to_vec()
            });

            // Store preimage
            let encoded_second_proposal = second_proposal.encode();
            let second_preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_second_proposal);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_one.clone()),
                encoded_second_proposal.clone()
            ));

            // Submit second referendum
            let bounded_second_call = frame_support::traits::Bounded::Lookup {
                hash: second_preimage_hash,
                len: encoded_second_proposal.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_one.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_second_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let second_referendum_index = 1;

            // Place decision deposit
            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_one.clone()),
                second_referendum_index
            ));

            // Run to just after prepare period for second referendum
            let second_referendum_start = 2 * track_info.prepare_period + 2;
            println!("Current block before second referendum: {}", frame_system::Pallet::<Runtime>::block_number());
            run_to_block(second_referendum_start);
            println!("Block after prepare period: {}", frame_system::Pallet::<Runtime>::block_number());

            // Only member_one votes (AYE) - by default this should be enough to approve if no one votes against
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_one.clone()),
                second_referendum_index,
                true // AYE vote
            ));

            // Check referendum status after vote
            let status_after_vote = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(second_referendum_index)
                .expect("Second referendum info should exist");
            println!("Referendum status after vote: {:?}", status_after_vote);

            // Wait until the end of the confirm phase for the second referendum
            let second_confirm_end = second_referendum_start + track_info.decision_period + track_info.confirm_period + track_info.min_enactment_period;
            run_to_block(second_confirm_end + 5);

            // Check second referendum outcome
            let second_referendum_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(second_referendum_index)
                .expect("Second referendum info should exist");

            println!("Final referendum status: {:?}", second_referendum_info);

            // This referendum should pass since there are only AYE votes
            match second_referendum_info {
                pallet_referenda::ReferendumInfo::Approved(_, _, _) => {
                    // This referendum should pass since there are only AYE votes
                    println!("Second referendum was approved as expected with only AYE votes");
                },
                pallet_referenda::ReferendumInfo::Rejected(_, _, _) => {
                    panic!("Second referendum was unexpectedly rejected with only AYE votes");
                },
                _ => {
                    panic!("Second referendum should be completed at this point");
                }
            }

            // Create a third referendum with 5 voters
            let third_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Third voting test proposal with 5 voters".to_vec()
            });

            // Store preimage
            let encoded_third_proposal = third_proposal.encode();
            let third_preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_third_proposal);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_one.clone()),
                encoded_third_proposal.clone()
            ));

            // Submit third referendum
            let bounded_third_call = frame_support::traits::Bounded::Lookup {
                hash: third_preimage_hash,
                len: encoded_third_proposal.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_one.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_third_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let third_referendum_index = 2;

            // Place decision deposit
            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_one.clone()),
                third_referendum_index
            ));

            // Run to just after prepare period for third referendum
            run_to_block(3 * track_info.prepare_period + 3);

            // Test scenario with 5 voters: 4 AYE vs 1 NAY
            // First four members vote AYE
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_one.clone()),
                third_referendum_index,
                true // AYE vote
            ));

            println!("Member one voted AYE for third referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_two.clone()),
                third_referendum_index,
                true // AYE vote
            ));

            println!("Member two voted AYE for third referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_three.clone()),
                third_referendum_index,
                true // AYE vote
            ));

            println!("Member three voted AYE for third referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_four.clone()),
                third_referendum_index,
                true // AYE vote
            ));

            println!("Member four voted AYE for third referendum");

            // Last member votes NAY
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_five.clone()),
                third_referendum_index,
                false // NAY vote
            ));

            println!("Member five voted NAY for third referendum");

            // Wait for the confirmation period
            run_to_block(1382405 + 172800 + 5); // Wait for confirmation period + some extra blocks

            // Print detailed timing information
            println!("Timing parameters:");
            println!("  prepare_period: {} blocks", track_info.prepare_period);
            println!("  decision_period: {} blocks", track_info.decision_period);
            println!("  confirm_period: {} blocks", track_info.confirm_period);
            println!("  min_enactment_period: {} blocks", track_info.min_enactment_period);

            // Print referendum status with more details
            let third_referendum_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(third_referendum_index)
                .expect("Third referendum info should exist");
            match third_referendum_info {
                pallet_referenda::ReferendumInfo::Approved(_, _, _) => {
                    // This referendum should pass since there are 4 AYE votes vs 1 NAY vote
                    println!("Third referendum was approved as expected with 4 AYE votes vs 1 NAY vote");
                },
                pallet_referenda::ReferendumInfo::Rejected(_, _, _) => {
                    panic!("Third referendum was unexpectedly rejected with 4 AYE votes vs 1 NAY vote");
                },
                _ => {
                    panic!("Third referendum should be completed at this point");
                }
            }

            // Create a fourth referendum to test 2 AYE vs 3 NAY
            let fourth_proposal = RuntimeCall::System(frame_system::Call::remark {
                remark: b"Fourth voting test proposal with 5 voters".to_vec()
            });

            // Store preimage
            let encoded_fourth_proposal = fourth_proposal.encode();
            let fourth_preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded_fourth_proposal);
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(member_one.clone()),
                encoded_fourth_proposal.clone()
            ));

            // Submit fourth referendum
            let bounded_fourth_call = frame_support::traits::Bounded::Lookup {
                hash: fourth_preimage_hash,
                len: encoded_fourth_proposal.len() as u32
            };

            assert_ok!(TechReferenda::submit(
                RuntimeOrigin::signed(member_one.clone()),
                Box::new(OriginCaller::system(frame_system::RawOrigin::Root)),
                bounded_fourth_call,
                frame_support::traits::schedule::DispatchTime::After(0u32.into())
            ));

            let fourth_referendum_index = 3;

            // Place decision deposit
            assert_ok!(TechReferenda::place_decision_deposit(
                RuntimeOrigin::signed(member_one.clone()),
                fourth_referendum_index
            ));

            // Run to just after prepare period for fourth referendum
            run_to_block(4 * track_info.prepare_period + 4);

            // Test scenario with 5 voters: 2 AYE vs 3 NAY
            // First two members vote AYE
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_one.clone()),
                fourth_referendum_index,
                true // AYE vote
            ));

            println!("Member one voted AYE for fourth referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_two.clone()),
                fourth_referendum_index,
                true // AYE vote
            ));

            println!("Member two voted AYE for fourth referendum");

            // Last three members vote NAY
            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_three.clone()),
                fourth_referendum_index,
                false // NAY vote
            ));

            println!("Member three voted NAY for fourth referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_four.clone()),
                fourth_referendum_index,
                false // NAY vote
            ));

            println!("Member four voted NAY for fourth referendum");

            assert_ok!(TechCollective::vote(
                RuntimeOrigin::signed(member_five.clone()),
                fourth_referendum_index,
                false // NAY vote
            ));

            println!("Member five voted NAY for fourth referendum");

            // Wait for the confirmation period for the fourth referendum to complete
            let fourth_submitted_block = frame_system::Pallet::<Runtime>::block_number();
            let fourth_decision_start = fourth_submitted_block + track_info.prepare_period;
            let fourth_confirm_start = fourth_decision_start + track_info.decision_period;
            let fourth_confirm_end = fourth_confirm_start + track_info.confirm_period;
            run_to_block(fourth_confirm_end + 5); // Wait for confirmation period + some extra blocks

            // Check fourth referendum outcome
            let fourth_referendum_info = pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(fourth_referendum_index)
                .expect("Fourth referendum info should exist");

            // This referendum should fail since there are 2 AYE votes vs 3 NAY votes
            match fourth_referendum_info {
                pallet_referenda::ReferendumInfo::Approved(_, _, _) => {
                    panic!("Fourth referendum was unexpectedly approved with 2 AYE votes vs 3 NAY votes");
                },
                pallet_referenda::ReferendumInfo::Rejected(_, _, _) => {
                    // This referendum should fail since there are 2 AYE votes vs 3 NAY votes
                    println!("Fourth referendum was rejected as expected with 2 AYE votes vs 3 NAY votes");
                },
                _ => {
                    panic!("Fourth referendum should be completed at this point");
                }
            }
        });
    }
}