use crate::{
	mock::{account_id, new_test_ext, Test, Treasury},
	Error,
};
use frame_support::{assert_err, assert_ok};

#[test]
fn genesis_sets_treasury_config() {
	new_test_ext().execute_with(|| {
		assert_eq!(Treasury::account_id(), account_id(1));
		assert_eq!(Treasury::portion(), 50);
	});
}

#[test]
fn set_treasury_account_works() {
	new_test_ext().execute_with(|| {
		assert_ok!(Treasury::set_treasury_account(
			frame_system::RawOrigin::Root.into(),
			account_id(99)
		));
		assert_eq!(Treasury::account_id(), account_id(99));
	});
}

#[test]
fn set_treasury_account_requires_root() {
	new_test_ext().execute_with(|| {
		assert_err!(
			Treasury::set_treasury_account(
				frame_system::RawOrigin::Signed(account_id(1)).into(),
				account_id(99)
			),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn set_treasury_portion_works() {
	new_test_ext().execute_with(|| {
		assert_ok!(Treasury::set_treasury_portion(frame_system::RawOrigin::Root.into(), 30));
		assert_eq!(Treasury::portion(), 30);
	});
}

#[test]
fn set_treasury_portion_rejects_invalid() {
	new_test_ext().execute_with(|| {
		assert_err!(
			Treasury::set_treasury_portion(frame_system::RawOrigin::Root.into(), 101),
			Error::<Test>::InvalidPortion
		);
	});
}
