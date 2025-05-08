#[path = "common.rs"]
mod common;

#[cfg(test)]
mod fellowship_tests {
    use crate::common::{account_id, new_test_ext};
    use frame_support::assert_ok;
    use frame_support::traits::{Currency, RankedMembers};
    use sp_runtime::MultiAddress;
    use resonance_runtime::{Balances, Runtime, RuntimeOrigin, System, TechFellowship};
    use pallet_core_fellowship::{self};

    // // Helper function to create evidence
    // fn create_evidence(text: &[u8]) -> SpBoundedVec<u8, FellowshipEvidenceSize> {
    //     text.to_vec().try_into().unwrap()
    // }

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

    #[test]
    fn open_application_process_works() {
        new_test_ext().execute_with(|| {
            let applicant = account_id(2);

            // Give the account some funds
            Balances::make_free_balance_be(&applicant, 100_000_000_000_000);

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

}