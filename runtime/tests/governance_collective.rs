#[path = "common.rs"]
mod common;

#[cfg(test)]
mod tests {
    use crate::common::{account_id, new_test_ext, run_to_block};
    use codec::Encode;
    use frame_support::assert_ok;
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
    fn test_add_member_via_referendum_in_fellowship() {
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
            println!("Initial referendum info for index {}: {:?}", referendum_index, initial_info);
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
                        println!("Referendum {} is deciding. Track: {}, Submitted: {}", i, status.track, status.submitted);
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
}