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
    use resonance_runtime::{
        Balances, ConvictionVoting, OriginCaller, Preimage, Referenda, Runtime, RuntimeCall,
        RuntimeOrigin, UNIT,
    };
    use sp_runtime::traits::Hash;
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
            let call = RuntimeCall::TechCommunity(pallet_membership::Call::add_member {
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
}
