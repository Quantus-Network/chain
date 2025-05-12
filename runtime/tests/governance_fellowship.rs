#[path = "common.rs"]
mod common;

#[cfg(test)]
mod fellowship_tests {
    use crate::common::{account_id, new_test_ext};
    use frame_support::{assert_noop, assert_ok};
    use frame_support::traits::RankedMembers;
    use sp_runtime::{bounded_vec, DispatchError, MultiAddress};
    use resonance_runtime::{CoreFellowship, Runtime, RuntimeOrigin, System, TechFellowship};
    use pallet_core_fellowship::{self, ParamsType};
    use sp_core::crypto::AccountId32;
    use resonance_runtime::fellowship::MaxFellowshipRank;

    fn signed(who: AccountId32) -> RuntimeOrigin {
        RuntimeOrigin::signed(who)
    }

    fn setup_fellowship_params() -> ParamsType<u128, u32, MaxFellowshipRank> {
        ParamsType {
            active_salary: bounded_vec![0, 0, 0, 0],  // no salary
            passive_salary: bounded_vec![0, 0, 0, 0], // no salary
            demotion_period: bounded_vec![1, 2, 3, 4], // short periods for tests
            min_promotion_period: bounded_vec![1, 1, 1, 1], // minimal values for tests
            offboard_timeout: 1, // short timeout
        }
    }

    #[test]
    fn set_params_works() {
        new_test_ext().execute_with(|| {
            let params = setup_fellowship_params();
            let new_member = account_id(1);

            assert_noop!(
                CoreFellowship::set_params(signed(new_member), Box::new(params.clone())),
                DispatchError::BadOrigin
            );
            assert_ok!(CoreFellowship::set_params(RuntimeOrigin::root(), Box::new(params)));
        });
    }

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

    #[test]
    fn evidence_submission_and_approval_works() {
        new_test_ext().execute_with(|| {

            let params = setup_fellowship_params();
            assert_ok!(CoreFellowship::set_params(RuntimeOrigin::root(), Box::new(params)));

            // Create accounts for testing
            let high_rank_member = account_id(1);
            let regular_member = account_id(2);
            let candidate = account_id(3);

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

            // Import them into the fellowship
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

            // Promote high_rank_member to rank 2 (needed for approval)
            for i in 1..=3 {
                assert_ok!(
                    pallet_core_fellowship::Pallet::<Runtime>::promote_fast(
                        RuntimeOrigin::root(),
                        high_rank_member.clone(),
                        i
                    )
                );
            }

            // Create a new candidate via self-induction
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::induct(
                    RuntimeOrigin::signed(candidate.clone()),
                    candidate.clone()
                )
            );

            // Verify initial ranks
            assert_eq!(TechFellowship::rank_of(&high_rank_member), Some(3));
            assert_eq!(TechFellowship::rank_of(&candidate), Some(0));

            // Create evidence for promotion
            /*let evidence = vec![1, 2, 3, 4, 5]; // Sample evidence data


            // Submit evidence for promotion to rank 1
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::submit_evidence(
                    RuntimeOrigin::signed(candidate.clone()),
                    pallet_core_fellowship::Wish::Promotion,
                    evidence.clone().try_into().unwrap()
                )
            );

            System::assert_last_event(
                pallet_core_fellowship::Event::<Runtime>::Requested {
                    who: candidate.clone(),
                    wish: pallet_core_fellowship::Wish::Promotion
                }.into()
            );*/

            System::set_block_number(2);

            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::promote(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    1  // Target rank for promotion
                )
            );

            // System::assert_has_event(
            //     pallet_core_fellowship::Event::<Runtime>::EvidenceJudged {
            //         who: candidate.clone(),
            //         wish: pallet_core_fellowship::Wish::Promotion,
            //         evidence: evidence.clone().try_into().unwrap(),
            //         old_rank: 0,
            //         new_rank: Some(1)
            //     }.into()
            // );

            System::set_block_number(5);

            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::promote(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    2  // Target rank for promotion
                )
            );

            let evidence_lv2 = vec![1, 2, 3, 4, 5]; // Sample evidence data
            // Submit evidence for promotion to rank 1
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::submit_evidence(
                    RuntimeOrigin::signed(candidate.clone()),
                    pallet_core_fellowship::Wish::Promotion,
                    evidence_lv2.clone().try_into().unwrap()
                )
            );

            /*

            // Approve the evidence for promotion to rank 1
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::approve(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    1  // Target rank for promotion
                )
            );


            // Then promote to rank 1
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::promote(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    1
                )
            );

            // Verify the promotion was successful
            assert_eq!(TechFellowship::rank_of(&candidate), Some(1));

            // Try to submit new evidence for next promotion
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::submit_evidence(
                    RuntimeOrigin::signed(candidate.clone()),
                    pallet_core_fellowship::Wish::Promotion,
                    evidence.clone().try_into().unwrap()
                )
            );

            // Approve new evidence for promotion to rank 2
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::approve(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    2  // Target rank for promotion
                )
            );

            // Try to promote to rank 2
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::promote(
                    RuntimeOrigin::signed(high_rank_member.clone()),
                    candidate.clone(),
                    2
                )
            );

            // Verify the second promotion was successful
            assert_eq!(TechFellowship::rank_of(&candidate), Some(2));
            */
        });
    }
    #[test]
    fn evidence_resets_demotion_period() {
        new_test_ext().execute_with(|| {
            // Set parameters with short demotion periods for tests
            let params = setup_fellowship_params();
            assert_ok!(CoreFellowship::set_params(RuntimeOrigin::root(), Box::new(params)));

            let member = account_id(1);
            let high_rank_member = account_id(2);  // Dodajemy członka wysokiej rangi

            // Add members and promote high_rank_member to rank 3
            assert_ok!(
                pallet_ranked_collective::Pallet::<Runtime>::add_member(
                    RuntimeOrigin::root(),
                    MultiAddress::from(member.clone())
                )
            );
            assert_ok!(
                pallet_ranked_collective::Pallet::<Runtime>::add_member(
                    RuntimeOrigin::root(),
                    MultiAddress::from(high_rank_member.clone())
                )
            );
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::import(
                    RuntimeOrigin::signed(member.clone())
                )
            );
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::import(
                    RuntimeOrigin::signed(high_rank_member.clone())
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

            // Promote member to rank 2
            for i in 1..=2 {
                assert_ok!(
                    pallet_core_fellowship::Pallet::<Runtime>::promote_fast(
                        RuntimeOrigin::root(),
                        member.clone(),
                        i
                    )
                );
            }

            // Check initial rank
            assert_eq!(TechFellowship::rank_of(&member), Some(2));

            // Wait almost until demotion period (2 blocks)
            System::set_block_number(3);

            // Submit evidence to reset demotion period
            let evidence = vec![1, 2, 3, 4, 5];
            assert_ok!(
                pallet_core_fellowship::Pallet::<Runtime>::submit_evidence(
                    RuntimeOrigin::signed(member.clone()),
                    pallet_core_fellowship::Wish::Promotion,
                    evidence.clone().try_into().unwrap()
                )
            );

            // Wait past original demotion period
            System::set_block_number(4);

            // Check that rank hasn't changed due to evidence submission
            assert_eq!(TechFellowship::rank_of(&member), Some(2));

            // Add bump to actually trigger the demotion using high_rank_member
            // Root can't do this, only user with higher rank
            // We could run it from outside or even from offchain worker, but it's not triggered automatically
            assert_ok!(CoreFellowship::bump(signed(high_rank_member.clone()), member.clone()));
            assert_eq!(TechFellowship::rank_of(&member), Some(1));
         });
    }

}