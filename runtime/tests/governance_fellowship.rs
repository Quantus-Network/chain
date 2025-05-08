#[path = "common.rs"]
mod common;

#[cfg(test)]
mod fellowship_tests {
    use crate::common::{account_id, new_test_ext};
    use frame_support::{assert_ok, BoundedVec};
    use frame_support::traits::{RankedMembers};
    use sp_runtime::MultiAddress;
    use resonance_runtime::{Runtime, RuntimeOrigin, System};
    use resonance_runtime::fellowship::FellowshipEvidenceSize;

    // Test adding a new member at rank 0
    #[test]
    fn add_member_works() {
        new_test_ext().execute_with(|| {
            //let admin = account_id(1);
            let new_member = account_id(2);

            // Setup admin with root privileges
            System::set_block_number(1);

            // Add a new member at rank 0
            assert_ok!(
                pallet_ranked_collective::Pallet::<Runtime>::add_member(
                    RuntimeOrigin::root(),
                    MultiAddress::from(new_member.clone())
                )
            );

            // Verify the member was added with rank 0
            assert_eq!(
                pallet_ranked_collective::Pallet::<Runtime>::rank_of(&new_member),
                Some(0)
            );

            // Verify the member count was updated by checking MemberCount storage
            assert_eq!(
                pallet_ranked_collective::MemberCount::<Runtime>::get(0),
                1
            );
        });
    }

/*    #[test]
    fn evidence_based_fellowship_works() {
        new_test_ext().execute_with(|| {
            let candidate = account_id(2);
            let approver = account_id(1);
            let evidence = b"Evidence of contributions: GitHub commits, documentation, community support".to_vec();

            // Set up an approver with high rank
            assert_ok!(
            pallet_ranked_collective::Pallet::<Runtime>::add_member(
                RuntimeOrigin::root(),
                MultiAddress::from(approver.clone())
            )
        );
            let member_record = pallet_ranked_collective::MemberRecord::new(3);

            // Insert the MemberRecord for the approver
            pallet_ranked_collective::Members::<Runtime>::insert(&approver, member_record);


            // Step 1: Submit evidence with a wish for induction
            // First, we need to create a bounded vector for the evidence
            let bounded_evidence = BoundedVec::<u8, FellowshipEvidenceSize>::try_from(evidence.clone())
                .expect("Evidence should fit within size limit");


            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::induct(
                    RuntimeOrigin::root(),//RuntimeOrigin::signed(approver.clone()),
                    candidate.clone()
                )
            );

            // Verify induction worked
            assert_eq!(
                pallet_ranked_collective::Pallet::<Runtime>::rank_of(&candidate),
                Some(0)
            );

            // Step 3: Member submits evidence with a wish for promotion
            let bounded_evidence = BoundedVec::<u8, FellowshipEvidenceSize>::try_from(evidence.clone())
                .expect("Evidence should fit within size limit");

            assert_ok!(
            pallet_core_fellowship::Pallet::<Runtime>::submit_evidence(
                RuntimeOrigin::signed(candidate.clone()),
                pallet_core_fellowship::Wish::Promotion, // Request promotion to next rank
                bounded_evidence
            )
        );

            // Step 4: Approver promotes the member
            assert_ok!(
            pallet_core_fellowship::Pallet::<Runtime>::promote(
                RuntimeOrigin::root(),//RuntimeOrigin::signed(approver.clone()),
                candidate.clone(),
                 1// No specific rank - advance by 1
            )
        );

            // Verify promotion worked
            assert_eq!(
                pallet_ranked_collective::Pallet::<Runtime>::rank_of(&candidate),
                Some(1)
            );
        });
    }*/
}