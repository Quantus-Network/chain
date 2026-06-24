use crate::{
	mock::{account_id, new_test_ext, AccountId, RuntimeOrigin, System, Test, UpgradeGov},
	pallet::{EnactmentDelay, Members, Proposals, Threshold},
	Error, Event,
};
use frame_support::{assert_noop, assert_ok, traits::OnInitialize};
use sp_runtime::testing::H256;

type GovAction = crate::pallet::ActionOf<Test>;

fn signed(id: u8) -> RuntimeOrigin {
	RuntimeOrigin::signed(account_id(id))
}

fn members() -> Vec<AccountId> {
	vec![account_id(1), account_id(2), account_id(3)]
}

fn run_to_block(n: u64) {
	while System::block_number() < n {
		let b = System::block_number() + 1;
		System::set_block_number(b);
		UpgradeGov::on_initialize(b);
	}
}

#[test]
fn genesis_seeds_members_threshold_delay() {
	new_test_ext(members(), 2, 5).execute_with(|| {
		assert_eq!(Members::<Test>::get().into_inner(), members());
		assert_eq!(Threshold::<Test>::get(), 2);
		assert_eq!(EnactmentDelay::<Test>::get(), 5);
	});
}

#[test]
fn non_member_cannot_propose() {
	new_test_ext(members(), 2, 5).execute_with(|| {
		assert_noop!(
			UpgradeGov::propose(signed(9), GovAction::AuthorizeUpgrade(H256::repeat_byte(1))),
			Error::<Test>::NotMember
		);
	});
}

#[test]
fn authorize_upgrade_flows_through_threshold_and_timelock() {
	new_test_ext(members(), 2, 5).execute_with(|| {
		let code_hash = H256::repeat_byte(0xab);
		assert_ok!(UpgradeGov::propose(signed(1), GovAction::AuthorizeUpgrade(code_hash)));
		// One approval, threshold 2 => not armed.
		assert_eq!(Proposals::<Test>::get(0).unwrap().enact_at, None);

		assert_ok!(UpgradeGov::approve(signed(2), 0));
		// Armed at now (1) + delay (5).
		assert_eq!(Proposals::<Test>::get(0).unwrap().enact_at, Some(6));

		// Nothing happens before the timelock elapses.
		run_to_block(5);
		assert!(Proposals::<Test>::get(0).is_some());

		run_to_block(6);
		assert!(Proposals::<Test>::get(0).is_none());
		System::assert_has_event(Event::Enacted { id: 0 }.into());
		System::assert_has_event(
			frame_system::Event::UpgradeAuthorized { code_hash, check_version: true }.into(),
		);
	});
}

#[test]
fn threshold_one_arms_on_propose() {
	new_test_ext(members(), 1, 0).execute_with(|| {
		assert_ok!(UpgradeGov::propose(
			signed(1),
			GovAction::AuthorizeUpgrade(H256::repeat_byte(2))
		));
		// delay 0 => enact_at == now.
		assert_eq!(Proposals::<Test>::get(0).unwrap().enact_at, Some(1));
		run_to_block(2);
		System::assert_has_event(Event::Enacted { id: 0 }.into());
	});
}

#[test]
fn double_approve_and_armed_guards() {
	new_test_ext(members(), 2, 5).execute_with(|| {
		assert_ok!(UpgradeGov::propose(
			signed(1),
			GovAction::AuthorizeUpgrade(H256::repeat_byte(3))
		));
		assert_noop!(UpgradeGov::approve(signed(1), 0), Error::<Test>::AlreadyApproved);
		assert_ok!(UpgradeGov::approve(signed(2), 0));
		// Already armed; further approvals rejected.
		assert_noop!(UpgradeGov::approve(signed(3), 0), Error::<Test>::AlreadyArmed);
	});
}

#[test]
fn add_and_remove_member_via_governance() {
	new_test_ext(members(), 2, 0).execute_with(|| {
		// Add member 4.
		assert_ok!(UpgradeGov::propose(signed(1), GovAction::AddMember(account_id(4))));
		assert_ok!(UpgradeGov::approve(signed(2), 0));
		run_to_block(2);
		assert!(Members::<Test>::get().contains(&account_id(4)));

		// Remove member 4.
		assert_ok!(UpgradeGov::propose(signed(1), GovAction::RemoveMember(account_id(4))));
		assert_ok!(UpgradeGov::approve(signed(2), 1));
		run_to_block(3);
		assert!(!Members::<Test>::get().contains(&account_id(4)));
	});
}

#[test]
fn remove_member_blocked_when_threshold_would_exceed_members() {
	// 2 members, threshold 2: removing one would make threshold > members => enactment fails.
	new_test_ext(vec![account_id(1), account_id(2)], 2, 0).execute_with(|| {
		assert_ok!(UpgradeGov::propose(signed(1), GovAction::RemoveMember(account_id(2))));
		assert_ok!(UpgradeGov::approve(signed(2), 0));
		run_to_block(2);
		// Still two members; the removal was rejected at enactment.
		assert_eq!(Members::<Test>::get().len(), 2);
		System::assert_has_event(Event::EnactmentFailed { id: 0 }.into());
	});
}

#[test]
fn cancel_removes_proposal() {
	new_test_ext(members(), 2, 5).execute_with(|| {
		assert_ok!(UpgradeGov::propose(
			signed(1),
			GovAction::AuthorizeUpgrade(H256::repeat_byte(7))
		));
		assert_ok!(UpgradeGov::cancel(signed(3), 0));
		assert!(Proposals::<Test>::get(0).is_none());
		System::assert_has_event(Event::Cancelled { id: 0 }.into());
	});
}

#[test]
fn enactment_revalidates_membership() {
	// A proposal armed by {m1, m2} must fail at enactment if m2 is removed first, dropping its
	// valid approvals below the threshold.
	new_test_ext(members(), 2, 5).execute_with(|| {
		// Proposal 0: remove m2 (approved by m1 + m3), arms at block 6.
		assert_ok!(UpgradeGov::propose(signed(1), GovAction::RemoveMember(account_id(2))));
		assert_ok!(UpgradeGov::approve(signed(3), 0));
		assert_eq!(Proposals::<Test>::get(0).unwrap().enact_at, Some(6));

		// Proposal 1 is created later (block 3) and armed by m1 + m2 for block 8.
		run_to_block(3);
		assert_ok!(UpgradeGov::propose(
			signed(1),
			GovAction::AuthorizeUpgrade(H256::repeat_byte(9))
		));
		assert_ok!(UpgradeGov::approve(signed(2), 1));
		assert_eq!(Proposals::<Test>::get(1).unwrap().enact_at, Some(8));

		// Block 6: m2 removed.
		run_to_block(6);
		assert!(!Members::<Test>::get().contains(&account_id(2)));

		// Block 8: proposal 1 re-validates; only m1 of its approvers remains => below threshold.
		run_to_block(8);
		System::assert_has_event(Event::EnactmentFailed { id: 1 }.into());
		assert!(Proposals::<Test>::get(1).is_none());
	});
}
