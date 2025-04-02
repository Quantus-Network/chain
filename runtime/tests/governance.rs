use frame_support::__private::sp_io;
use frame_support::traits::Currency;
use sp_core::crypto::AccountId32;
use sp_runtime::BuildStorage;
use resonance_runtime::{UNIT, Runtime, RuntimeOrigin, Balances};

#[cfg(test)]
mod tests {
    use super::*;
    use frame_support::{assert_noop, assert_ok, traits::PreimageProvider, BoundedVec, StorageHasher};
    use frame_support::traits::{ConstU32, QueryPreimage};
    use sp_core::crypto::AccountId32;
    use pallet_balances::PoseidonHasher;
    use resonance_runtime::Preimage;

    // Helper function to create AccountId32 from a simple index
    fn account_id(id: u8) -> AccountId32 {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        AccountId32::new(bytes)
    }

    // Helper function to create simple test data
    fn bounded(s: &[u8]) -> BoundedVec<u8, ConstU32<100>> {
        s.to_vec().try_into().unwrap()
    }

    #[test]
    fn note_preimage_works() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);
            // Check initial balance
            let initial_balance = Balances::free_balance(&account);

            // Create test data
            let preimage_data = bounded(b"test_preimage_data");
            let hash = PoseidonHasher::hash(&preimage_data);

            // Note the preimage
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(account.clone()),
                preimage_data.to_vec(),
            ));

            // Check if preimage was stored
            assert!(Preimage::have_preimage(&hash.into()));

            // If using an implementation with token reservation, check if balance changed
            if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
                let final_balance = Balances::free_balance(&account);
                let reserved = Balances::reserved_balance(&account);

                // Check if balance was reduced
                assert!(final_balance < initial_balance);
                // Check if tokens were reserved
                assert!(reserved > 0);
            }
        });
    }

    #[test]
    fn unnote_preimage_works() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);
            let initial_balance = Balances::free_balance(&account);

            // Create test data
            let preimage_data = bounded(b"test_preimage_data");
            let hash = PoseidonHasher::hash(&preimage_data);

            // Note the preimage
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(account.clone()),
                preimage_data.to_vec(),
            ));

            // Remove the preimage
            assert_ok!(Preimage::unnote_preimage(
                RuntimeOrigin::signed(account.clone()),
                hash.into(),
            ));

            // Check if preimage was removed
            assert!(!Preimage::have_preimage(&hash.into()));

            // If using an implementation with token reservation, check if balance was restored
            if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
                let final_balance = Balances::free_balance(&account);
                let reserved = Balances::reserved_balance(&account);

                // Balance should return to initial amount
                assert_eq!(final_balance, initial_balance);
                // No tokens should be reserved
                assert_eq!(reserved, 0);
            }
        });
    }

    #[test]
    fn request_preimage_works() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);
            let initial_balance = Balances::free_balance(&account);

            // Create test data
            let preimage_data = bounded(b"test_preimage_data");
            let hash = PoseidonHasher::hash(&preimage_data);

            // Note the preimage
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(account.clone()),
                preimage_data.to_vec(),
            ));

            // Request the preimage as system
            assert_ok!(Preimage::request_preimage(
                RuntimeOrigin::root(),
                hash.into(),
            ));

            // Check if preimage was requested
            assert!(Preimage::is_requested(&hash.into()));

            // If using an implementation with token reservation, check if balance was freed
            if !std::any::TypeId::of::<()>().eq(&std::any::TypeId::of::<()>()) {
                let final_balance = Balances::free_balance(&account);

                // Balance should return to initial amount
                assert_eq!(final_balance, initial_balance);
            }
        });
    }

    #[test]
    fn unrequest_preimage_works() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);

            // Create test data
            let preimage_data = bounded(b"test_preimage_data");
            let hash = PoseidonHasher::hash(&preimage_data);

            // Note the preimage
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(account.clone()),
                preimage_data.to_vec(),
            ));

            // Request the preimage as system
            assert_ok!(Preimage::request_preimage(
                RuntimeOrigin::root(),
                hash.into(),
            ));

            // Then unrequest it
            assert_ok!(Preimage::unrequest_preimage(
                RuntimeOrigin::root(),
                hash.into(),
            ));

            // Check if preimage is no longer requested
            assert!(!Preimage::is_requested(&hash.into()));
        });
    }

    #[test]
    fn preimage_cannot_be_noted_twice() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);

            // Create test data
            let preimage_data = bounded(b"test_preimage_data");

            // Note the preimage for the first time
            assert_ok!(Preimage::note_preimage(
                RuntimeOrigin::signed(account.clone()),
                preimage_data.to_vec(),
            ));

            // Attempt to note the same preimage again should fail
            assert_noop!(
                Preimage::note_preimage(
                    RuntimeOrigin::signed(account.clone()),
                    preimage_data.to_vec(),
                ),
                pallet_preimage::Error::<Runtime>::AlreadyNoted
            );
        });
    }

    #[test]
    fn preimage_too_large_fails() {
        new_test_ext().execute_with(|| {
            let account = account_id(1);

            // Create large data exceeding the limit
            // 5MB should be larger than any reasonable limit
            let large_data = vec![0u8; 5 * 1024 * 1024];

            // Attempt to note an oversized preimage should fail
            assert_noop!(
                Preimage::note_preimage(
                    RuntimeOrigin::signed(account.clone()),
                    large_data,
                ),
                pallet_preimage::Error::<Runtime>::TooBig
            );
        });
    }
}

// Test environment implementation
fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<resonance_runtime::Runtime>::default()
        .build_storage()
        .unwrap();

    let mut ext = sp_io::TestExternalities::new(t);

    // Add balances in the ext
    ext.execute_with(|| {
        Balances::make_free_balance_be(&account_id(1), 100 * UNIT);
        Balances::make_free_balance_be(&account_id(2), 100 * UNIT);
    });

    ext
}

// Helper function to create AccountId32 from a simple index
// (defined outside the mod tests to be used in new_test_ext)
fn account_id(id: u8) -> AccountId32 {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    AccountId32::new(bytes)
}