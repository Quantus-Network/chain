use crate::{mock::*, Error, Event, NextScheduleId, Schedules, VestingSchedule};
use frame_support::{assert_noop, assert_ok, traits::fungible::Inspect};
use sp_runtime::{DispatchError, TokenError};

const START: u64 = 100_000;
const CLIFF: u64 = 200_000;
const END: u64 = 500_000;
const TOTAL: u128 = 4_000_000;

fn default_schedule(beneficiary: sp_core::crypto::AccountId32) -> ScheduleTuple {
	(beneficiary, START, CLIFF, END, TOTAL)
}

fn free(who: &sp_core::crypto::AccountId32) -> u128 {
	Balances::free_balance(who)
}

fn stored(id: u64) -> VestingSchedule<sp_core::crypto::AccountId32, u128> {
	Schedules::<Test>::get(id).expect("schedule must exist")
}

mod vested_amount {
	use super::*;

	fn schedule(
		start: u64,
		cliff: u64,
		end: u64,
		total: u128,
	) -> VestingSchedule<sp_core::crypto::AccountId32, u128> {
		VestingSchedule { beneficiary: BOB, start, cliff, end, total, claimed: 0 }
	}

	#[test]
	fn zero_before_cliff_even_after_start() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, CLIFF, END, TOTAL);
			assert_eq!(Vesting::vested_amount(&s, 0), 0);
			assert_eq!(Vesting::vested_amount(&s, START), 0);
			assert_eq!(Vesting::vested_amount(&s, CLIFF - 1), 0);
		});
	}

	#[test]
	fn jumps_to_accrued_amount_at_cliff() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, CLIFF, END, TOTAL);
			// Accrual runs from `start`, so the cliff unlocks 100_000ms worth at once.
			assert_eq!(Vesting::vested_amount(&s, CLIFF), TOTAL / 4);
		});
	}

	#[test]
	fn linear_between_cliff_and_end() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, CLIFF, END, TOTAL);
			assert_eq!(Vesting::vested_amount(&s, 300_000), TOTAL / 2);
			assert_eq!(Vesting::vested_amount(&s, 400_000), TOTAL * 3 / 4);
			assert_eq!(Vesting::vested_amount(&s, END - 1), 3_999_990);
		});
	}

	#[test]
	fn exact_total_at_and_after_end() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, CLIFF, END, TOTAL);
			assert_eq!(Vesting::vested_amount(&s, END), TOTAL);
			assert_eq!(Vesting::vested_amount(&s, u64::MAX), TOTAL);
		});
	}

	#[test]
	fn floor_rounding() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(0, 0, 3, 100);
			assert_eq!(Vesting::vested_amount(&s, 1), 33);
			assert_eq!(Vesting::vested_amount(&s, 2), 66);
			assert_eq!(Vesting::vested_amount(&s, 3), 100);
		});
	}

	#[test]
	fn start_equals_cliff_is_pure_linear() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, START, END, TOTAL);
			assert_eq!(Vesting::vested_amount(&s, START), 0);
			assert_eq!(Vesting::vested_amount(&s, START + 1), 10);
		});
	}

	#[test]
	fn cliff_equals_end_is_all_at_once() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(START, END, END, TOTAL);
			assert_eq!(Vesting::vested_amount(&s, END - 1), 0);
			assert_eq!(Vesting::vested_amount(&s, END), TOTAL);
		});
	}

	#[test]
	fn no_overflow_at_extreme_values() {
		new_test_ext(vec![]).execute_with(|| {
			let s = schedule(0, 0, u64::MAX, u128::from(u64::MAX) * 1_000_000_000_000);
			assert_eq!(Vesting::vested_amount(&s, u64::MAX), s.total);
			assert_eq!(Vesting::vested_amount(&s, u64::MAX - 1) > s.total / 2, true);
		});
	}
}

mod claim {
	use super::*;

	#[test]
	fn pays_out_and_updates_claimed() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			assert_eq!(free(&BOB), TOTAL / 2);
			assert_eq!(stored(0).claimed, TOTAL / 2);
			assert_eq!(free(&pot()), TOTAL / 2 + ExistentialDeposit::get());
			System::assert_last_event(
				Event::Claimed { schedule_id: 0, beneficiary: BOB, amount: TOTAL / 2 }.into(),
			);
		});
	}

	#[test]
	fn is_permissionless_and_pays_only_the_beneficiary() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			let pinger_before = free(&PINGER);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&BOB), TOTAL / 2);
			assert_eq!(free(&PINGER), pinger_before);
		});
	}

	#[test]
	fn nothing_before_cliff() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(CLIFF - 1);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(BOB), 0),
				Error::<Test>::NothingToClaim
			);
		});
	}

	#[test]
	fn repeat_claim_at_same_time_errors() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(BOB), 0),
				Error::<Test>::NothingToClaim
			);
		});
	}

	#[test]
	fn unknown_id_errors() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_noop!(Vesting::claim(RuntimeOrigin::signed(BOB), 1), Error::<Test>::NoSchedule);
		});
	}

	#[test]
	fn after_end_pays_remainder_exactly_and_schedule_stays() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			set_time(END + 1);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			assert_eq!(free(&BOB), TOTAL);
			assert_eq!(stored(0).claimed, TOTAL);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(BOB), 0),
				Error::<Test>::NothingToClaim
			);
		});
	}

	#[test]
	fn below_ed_payout_to_nonexistent_account_fails_cleanly_then_succeeds_later() {
		new_test_ext(vec![(CHARLIE, START, START, END, TOTAL)]).execute_with(|| {
			// 10 per ms; ED is 1_000, so 50ms in only 500 is owed.
			set_time(START + 50);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				TokenError::BelowMinimum
			);
			assert_eq!(stored(0).claimed, 0);
			set_time(START + 200);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), 2_000);
			assert_eq!(stored(0).claimed, 2_000);
		});
	}

	#[test]
	fn full_drain_leaves_pot_with_exactly_the_ed_buffer() {
		new_test_ext(vec![default_schedule(BOB), (CHARLIE, START, START, END, TOTAL)])
			.execute_with(|| {
				set_time(250_000);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 1));
				set_time(END);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 1));
				assert_eq!(free(&BOB), TOTAL);
				assert_eq!(free(&CHARLIE), TOTAL);
				assert_eq!(free(&pot()), ExistentialDeposit::get());
				assert_ok!(Vesting::do_try_state());
			});
	}

	#[test]
	fn multiple_schedules_for_same_beneficiary_claim_independently() {
		new_test_ext(vec![default_schedule(BOB), (BOB, START, START, 900_000, 8_000_000)])
			.execute_with(|| {
				set_time(500_000);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
				assert_eq!(free(&BOB), TOTAL);
				assert_eq!(stored(1).claimed, 0);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 1));
				assert_eq!(free(&BOB), TOTAL + 4_000_000);
				assert_eq!(stored(1).claimed, 4_000_000);
			});
	}
}

mod create_schedule {
	use super::*;

	#[test]
	fn treasury_signed_and_root_can_create_others_cannot() {
		new_test_ext(vec![]).execute_with(|| {
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::signed(TREASURY),
				BOB,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				CHARLIE,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(ALICE),
					BOB,
					START,
					CLIFF,
					END,
					TOTAL
				),
				DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn assigns_sequential_ids_and_moves_funds() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			let treasury_before = free(&TREASURY);
			let pot_before = free(&pot());
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::signed(TREASURY),
				CHARLIE,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert_eq!(NextScheduleId::<Test>::get(), 2);
			assert_eq!(stored(1).beneficiary, CHARLIE);
			assert_eq!(free(&TREASURY), treasury_before - TOTAL);
			assert_eq!(free(&pot()), pot_before + TOTAL);
			System::assert_last_event(
				Event::ScheduleCreated {
					schedule_id: 1,
					beneficiary: CHARLIE,
					start: START,
					cliff: CLIFF,
					end: END,
					total: TOTAL,
				}
				.into(),
			);
		});
	}

	#[test]
	fn rejects_invalid_parameters() {
		new_test_ext(vec![]).execute_with(|| {
			let cases = [
				(CLIFF, START, END, TOTAL),   // start > cliff
				(START, END + 1, END, TOTAL), // cliff > end
				(START, START, START, TOTAL), // start == end
				(START, CLIFF, END, 999),     // total < ED
				(START, CLIFF, END, 0),       // total == 0
			];
			for (start, cliff, end, total) in cases {
				assert_noop!(
					Vesting::create_schedule(
						RuntimeOrigin::signed(TREASURY),
						BOB,
						start,
						cliff,
						end,
						total
					),
					Error::<Test>::InvalidSchedule
				);
			}
		});
	}

	#[test]
	fn rejects_pot_as_beneficiary() {
		new_test_ext(vec![]).execute_with(|| {
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(TREASURY),
					pot(),
					START,
					CLIFF,
					END,
					TOTAL
				),
				Error::<Test>::InvalidBeneficiary
			);
		});
	}

	#[test]
	fn fails_when_treasury_not_configured() {
		new_test_ext(vec![]).execute_with(|| {
			TreasuryAccount::set(None);
			assert_noop!(
				Vesting::create_schedule(RuntimeOrigin::root(), BOB, START, CLIFF, END, TOTAL),
				Error::<Test>::TreasuryNotConfigured
			);
		});
	}

	#[test]
	fn fails_when_treasury_underfunded() {
		new_test_ext(vec![]).execute_with(|| {
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(TREASURY),
					BOB,
					START,
					CLIFF,
					END,
					TREASURY_FUNDS + 1
				),
				TokenError::FundsUnavailable
			);
			assert_eq!(NextScheduleId::<Test>::get(), 0);
			assert!(Schedules::<Test>::get(0).is_none());
		});
	}

	#[test]
	fn fails_when_pot_has_no_ed_buffer() {
		new_test_ext_with_pot_balance(vec![], 0).execute_with(|| {
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(TREASURY),
					BOB,
					START,
					CLIFF,
					END,
					TOTAL
				),
				Error::<Test>::PotUnderfunded
			);
		});
	}
}

mod end_schedule {
	use super::*;

	#[test]
	fn splits_mid_vesting_exactly() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			let treasury_before = free(&TREASURY);
			set_time(300_000);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_eq!(free(&BOB), TOTAL / 2);
			assert_eq!(free(&TREASURY), treasury_before + TOTAL / 2);
			assert_eq!(free(&pot()), ExistentialDeposit::get());
			assert!(Schedules::<Test>::get(0).is_none());
			System::assert_last_event(
				Event::ScheduleEnded {
					schedule_id: 0,
					beneficiary: BOB,
					vested_paid: TOTAL / 2,
					unvested_returned: TOTAL / 2,
				}
				.into(),
			);
		});
	}

	#[test]
	fn before_cliff_returns_everything_to_treasury() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			let treasury_before = free(&TREASURY);
			set_time(CLIFF - 1);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::root(), 0));
			assert_eq!(free(&BOB), 0);
			assert_eq!(free(&TREASURY), treasury_before + TOTAL);
		});
	}

	#[test]
	fn fully_vested_pays_beneficiary_everything() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			let treasury_before = free(&TREASURY);
			set_time(END);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_eq!(free(&BOB), TOTAL);
			assert_eq!(free(&TREASURY), treasury_before);
		});
	}

	#[test]
	fn after_partial_claims_pays_only_the_unpaid_vested_part() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(CLIFF);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			assert_eq!(free(&BOB), TOTAL / 4);
			let treasury_before = free(&TREASURY);
			set_time(300_000);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_eq!(free(&BOB), TOTAL / 2);
			assert_eq!(free(&TREASURY), treasury_before + TOTAL / 2);
			assert_eq!(free(&pot()), ExistentialDeposit::get());
		});
	}

	#[test]
	fn origin_and_error_paths() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::signed(ALICE), 0),
				DispatchError::BadOrigin
			);
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 7),
				Error::<Test>::NoSchedule
			);
			TreasuryAccount::set(None);
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::root(), 0),
				Error::<Test>::TreasuryNotConfigured
			);
		});
	}

	#[test]
	fn freed_ids_are_never_reused() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::signed(TREASURY),
				CHARLIE,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert!(Schedules::<Test>::get(0).is_none());
			assert_eq!(stored(1).beneficiary, CHARLIE);
			assert_eq!(NextScheduleId::<Test>::get(), 2);
		});
	}
}

mod retarget_schedule {
	use super::*;

	#[test]
	fn updates_beneficiary_and_preserves_everything_else() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(CLIFF);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
			assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::signed(TREASURY), 0, CHARLIE));
			let s = stored(0);
			assert_eq!(s.beneficiary, CHARLIE);
			assert_eq!(s.claimed, TOTAL / 4);
			System::assert_last_event(
				Event::ScheduleRetargeted {
					schedule_id: 0,
					old_beneficiary: BOB,
					new_beneficiary: CHARLIE,
				}
				.into(),
			);
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), TOTAL * 3 / 4);
			assert_eq!(free(&BOB), TOTAL / 4);
		});
	}

	#[test]
	fn rejects_pot_same_target_unknown_id_and_bad_origin() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			assert_noop!(
				Vesting::retarget_schedule(RuntimeOrigin::signed(TREASURY), 0, pot()),
				Error::<Test>::InvalidBeneficiary
			);
			assert_noop!(
				Vesting::retarget_schedule(RuntimeOrigin::root(), 0, BOB),
				Error::<Test>::InvalidBeneficiary
			);
			assert_noop!(
				Vesting::retarget_schedule(RuntimeOrigin::root(), 5, CHARLIE),
				Error::<Test>::NoSchedule
			);
			assert_noop!(
				Vesting::retarget_schedule(RuntimeOrigin::signed(ALICE), 0, CHARLIE),
				DispatchError::BadOrigin
			);
		});
	}
}

mod genesis {
	use super::*;

	#[test]
	fn builds_schedules_including_repeated_beneficiary() {
		new_test_ext(vec![
			default_schedule(BOB),
			default_schedule(BOB),
			(CHARLIE, START, START, END, TOTAL),
		])
		.execute_with(|| {
			assert_eq!(NextScheduleId::<Test>::get(), 3);
			assert_eq!(stored(0).beneficiary, BOB);
			assert_eq!(stored(1).beneficiary, BOB);
			assert_eq!(stored(2).beneficiary, CHARLIE);
			assert_eq!(free(&pot()), 3 * TOTAL + ExistentialDeposit::get());
			assert_ok!(Vesting::do_try_state());
		});
	}

	#[test]
	fn empty_is_a_noop() {
		new_test_ext(vec![]).execute_with(|| {
			assert_eq!(NextScheduleId::<Test>::get(), 0);
			assert_eq!(Schedules::<Test>::iter().count(), 0);
			assert_ok!(Vesting::do_try_state());
		});
	}

	#[test]
	#[should_panic(expected = "pot balance must equal sum of schedule totals")]
	fn pot_balance_mismatch_panics() {
		new_test_ext_with_pot_balance(vec![default_schedule(BOB)], TOTAL);
	}

	#[test]
	#[should_panic(expected = "invalid schedule at index 0")]
	fn invalid_schedule_panics() {
		new_test_ext(vec![(BOB, CLIFF, START, END, TOTAL)]);
	}

	#[test]
	#[should_panic(expected = "the pot cannot be a beneficiary")]
	fn pot_as_beneficiary_panics() {
		let pot = crate::Pallet::<Test>::pot_account_id();
		new_test_ext(vec![(pot, START, CLIFF, END, TOTAL)]);
	}
}

mod try_state {
	use super::*;

	#[test]
	fn holds_through_a_full_lifecycle() {
		new_test_ext(vec![default_schedule(BOB), (BOB, START, START, 900_000, 8_000_000)])
			.execute_with(|| {
				assert_ok!(Vesting::do_try_state());
				set_time(300_000);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
				assert_ok!(Vesting::do_try_state());
				assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::root(), 1, CHARLIE));
				assert_ok!(Vesting::do_try_state());
				assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
				assert_ok!(Vesting::do_try_state());
				assert_ok!(Vesting::create_schedule(
					RuntimeOrigin::signed(TREASURY),
					ALICE,
					START,
					CLIFF,
					END,
					TOTAL
				));
				assert_ok!(Vesting::do_try_state());
				set_time(END);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 2));
				assert_ok!(Vesting::do_try_state());
			});
	}
}
