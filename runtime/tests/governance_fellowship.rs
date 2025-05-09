#[path = "common.rs"]
mod common;

#[cfg(test)]
mod fellowship_tests {
    use crate::common::{account_id, new_test_ext};
    use frame_support::assert_ok;
    use frame_support::traits::RankedMembers;
    use sp_runtime::MultiAddress;
    use resonance_runtime::{Runtime, RuntimeOrigin, System, TechFellowship};
    use pallet_core_fellowship::{self};

    // Test adding a new member at rank 0
    #[test]
    fn add_member_as_root_works() {
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

    #[test]
    fn open_application_process_works() {
        new_test_ext().execute_with(|| {
            let applicant = account_id(2);

            // Set block number
            System::set_block_number(1);

            // Self-induct directly
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::induct(
                    RuntimeOrigin::signed(applicant.clone()),
                    applicant.clone()
                )
            );

            // Verify the member was added with rank 0
            assert_eq!(
                TechFellowship::rank_of(&applicant),
                Some(0)
            );
        });
    }

    #[test]
    fn fellowship_fast_promotion_works() {
        new_test_ext().execute_with(|| {
            // Create accounts for testing
            let high_rank_member = account_id(1);
            let regular_member = account_id(2);
            let candidate = account_id(3);

            // Idea here is that
            // root will promote high_rank_member,
            // next he will promote regular_member, and regular member will promote candidate.


            System::set_block_number(1);

            // First add members to the collective
            assert_ok!(
                pallet_ranked_collective::Pallet::<Runtime>::add_member(
                    RuntimeOrigin::root(),
                    MultiAddress::from(high_rank_member.clone())
                )
            );
            assert_ok!(
                pallet_ranked_collective::Pallet::<Runtime>::add_member(
                    RuntimeOrigin::root(),
                    MultiAddress::from(regular_member.clone())
                )
            );

            // Verify they are in the collective by checking their rank
            assert_eq!(TechFellowship::rank_of(&high_rank_member), Some(0));
            assert_eq!(TechFellowship::rank_of(&regular_member), Some(0));

            // Now import them into the fellowship
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::import(
                    RuntimeOrigin::signed(high_rank_member.clone())
                )
            );

            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::import(
                    RuntimeOrigin::signed(regular_member.clone())
                )
            );

            // Promote high_rank_member to rank 3
            for i in 1..=3 {
                assert_ok!(
                    pallet_core_fellowship::Pallet::<Runtime>::promote_fast(
                        RuntimeOrigin::root(),
                        high_rank_member.clone(),
                        i
                    )
                );
            }

            // Promote regular_member to rank 2
            for i in 1..=2 {
                assert_ok!(
                    pallet_core_fellowship::Pallet::<Runtime>::promote_fast(
                        RuntimeOrigin::signed(high_rank_member.clone()),
                        regular_member.clone(),
                        i
                    )
                );
            }

            // Verify initial ranks
            assert_eq!(TechFellowship::rank_of(&high_rank_member), Some(3));
            assert_eq!(TechFellowship::rank_of(&regular_member), Some(2));

            // Create a new candidate via self-induction
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::induct(
                    RuntimeOrigin::signed(candidate.clone()),
                    candidate.clone()
                )
            );

            // Verify candidate's initial rank
            assert_eq!(TechFellowship::rank_of(&candidate), Some(0));

            // Test promotion using high_rank_member (rank 3) to promote to rank 1
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::promote_fast(
                    RuntimeOrigin::signed(regular_member.clone()), // Using regular_member
                    candidate.clone(),
                    1
                )
            );

            // Verify the promotion was successful
            assert_eq!(TechFellowship::rank_of(&candidate), Some(1));
        });
    }

}