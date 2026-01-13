use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};

#[test]
fn genesis_config_works() {
	new_test_ext().execute_with(|| {
		// Check that genesis config was applied
		let signatories = TreasuryConfig::signatories();
		assert_eq!(signatories.len(), 5);
		assert_eq!(signatories.to_vec(), vec![1, 2, 3, 4, 5]);

		let threshold = TreasuryConfig::threshold();
		assert_eq!(threshold, 3);

		// Check that treasury account is computed correctly
		let treasury_account = TreasuryConfig::get_treasury_account();
		assert_ne!(treasury_account, 0); // Should be non-zero
	});
}

#[test]
fn set_treasury_signatories_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1); // Events are registered from block 1
		let old_account = TreasuryConfig::get_treasury_account();

		// Update signatories
		let new_signatories = vec![10, 20, 30, 40, 50];
		assert_ok!(TreasuryConfig::set_treasury_signatories(
			RuntimeOrigin::root(),
			new_signatories.clone(),
			3
		));

		// Check storage was updated
		assert_eq!(TreasuryConfig::signatories().to_vec(), new_signatories);
		assert_eq!(TreasuryConfig::threshold(), 3);

		// Check new account is different
		let new_account = TreasuryConfig::get_treasury_account();
		assert_ne!(old_account, new_account);

		// Check event was emitted
		System::assert_last_event(
			Event::TreasurySignatoriesUpdated { old_account, new_account }.into(),
		);
	});
}

#[test]
fn set_treasury_signatories_requires_root() {
	new_test_ext().execute_with(|| {
		// Try to update without root - should fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::signed(1), vec![10, 20, 30], 2),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn set_treasury_signatories_validates_empty() {
	new_test_ext().execute_with(|| {
		// Empty signatories should fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), vec![], 1),
			Error::<Test>::EmptySignatories
		);
	});
}

#[test]
fn set_treasury_signatories_validates_threshold() {
	new_test_ext().execute_with(|| {
		// Threshold = 0 should fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), vec![1, 2, 3], 0),
			Error::<Test>::InvalidThreshold
		);

		// Threshold > signatories should fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), vec![1, 2, 3], 4),
			Error::<Test>::InvalidThreshold
		);
	});
}

#[test]
fn set_treasury_signatories_rejects_duplicates() {
	new_test_ext().execute_with(|| {
		// Same signatory 5 times should fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), vec![1, 1, 1, 1, 1], 3),
			Error::<Test>::DuplicateSignatories
		);

		// Partial duplicates should also fail
		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), vec![1, 2, 3, 2, 4], 3),
			Error::<Test>::DuplicateSignatories
		);

		// No duplicates should succeed
		assert_ok!(TreasuryConfig::set_treasury_signatories(
			RuntimeOrigin::root(),
			vec![1, 2, 3, 4, 5],
			3
		));
	});
}

#[test]
fn set_treasury_signatories_validates_max() {
	new_test_ext().execute_with(|| {
		// Create more than MaxSignatories (100)
		let too_many: Vec<u64> = (0..101).collect();

		assert_noop!(
			TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), too_many, 50),
			Error::<Test>::TooManySignatories
		);
	});
}

#[test]
fn changing_threshold_changes_address() {
	new_test_ext().execute_with(|| {
		let signatories = vec![1, 2, 3, 4, 5];

		// Set threshold to 2
		assert_ok!(TreasuryConfig::set_treasury_signatories(
			RuntimeOrigin::root(),
			signatories.clone(),
			2
		));
		let account_threshold_2 = TreasuryConfig::get_treasury_account();

		// Set threshold to 4 (same signatories)
		assert_ok!(TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), signatories, 4));
		let account_threshold_4 = TreasuryConfig::get_treasury_account();

		// Addresses should be different
		assert_ne!(account_threshold_2, account_threshold_4);
	});
}

#[test]
fn deterministic_address_generation() {
	new_test_ext().execute_with(|| {
		let signatories = vec![1, 2, 3];

		// Set signatories
		assert_ok!(TreasuryConfig::set_treasury_signatories(
			RuntimeOrigin::root(),
			signatories.clone(),
			2
		));
		let account1 = TreasuryConfig::get_treasury_account();

		// Change to different signatories
		assert_ok!(TreasuryConfig::set_treasury_signatories(
			RuntimeOrigin::root(),
			vec![10, 20, 30],
			2
		));

		// Change back to original
		assert_ok!(TreasuryConfig::set_treasury_signatories(RuntimeOrigin::root(), signatories, 2));
		let account2 = TreasuryConfig::get_treasury_account();

		// Should get the same address
		assert_eq!(account1, account2);
	});
}
