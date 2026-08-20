use crate::{mock::*, Error, Event, NextScheduleId, Schedules, VestingSchedule};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::{DispatchError, TokenError};

const START: u64 = 100_000;
const CLIFF: u64 = 200_000;
const END: u64 = 500_000;
/// Large enough that a half-vested claim is a multiple of
/// `NON_FINAL_PAYOUT_QUANTA * PayoutQuantum` (2_500_000 at the default quantum).
const TOTAL: u128 = 10_000_000;

fn non_final() -> u128 {
	PayoutQuantum::get() * crate::NON_FINAL_PAYOUT_QUANTA
}

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
		VestingSchedule {
			beneficiary: BOB,
			start,
			cliff,
			end,
			total,
			claimed: 0,
			last_claim_at: None,
		}
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
			assert_eq!(Vesting::vested_amount(&s, END - 1), 9_999_975);
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
			assert_eq!(Vesting::vested_amount(&s, START + 1), TOTAL / (END - START) as u128);
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
			assert!(Vesting::vested_amount(&s, u64::MAX - 1) > s.total / 2);
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
	fn minimum_payout_prevents_below_ed_transfers() {
		new_test_ext(vec![(CHARLIE, START, START, END, TOTAL)]).execute_with(|| {
			PayoutQuantum::set(100);
			set_time(START + 50);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::NothingToClaim
			);
			assert_eq!(stored(0).claimed, 0);
			// A minimum-sized accrual is still below the non-final fee alignment.
			set_time(START + 400);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::NothingToClaim
			);
			set_time(START + 10_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), non_final());
			assert_eq!(stored(0).claimed, non_final());
		});
	}

	#[test]
	fn claims_are_rate_limited_per_schedule() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(350_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).last_claim_at, Some(350_000));
			set_time(400_000);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::ClaimTooSoon
			);
			set_time(450_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).last_claim_at, Some(450_000));
		});
	}

	#[test]
	fn daily_cadence_bounds_a_year_schedule_to_366_payouts() {
		const DAY: u64 = 24 * 60 * 60 * 1000;
		const YEAR: u64 = 365 * DAY;
		const YEAR_TOTAL: u128 = YEAR as u128 * 2_500_000;
		new_test_ext(vec![(BOB, 0, 0, YEAR, YEAR_TOTAL)]).execute_with(|| {
			MinClaimInterval::set(DAY);
			for day in 0..365 {
				set_time(1 + day * DAY);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			}
			set_time(YEAR);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::ClaimTooSoon
			);
			set_time(YEAR + 1);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(MockProofRecorder::recorded().len(), 366);
			assert_eq!(stored(0).claimed, YEAR_TOTAL);
		});
	}

	#[test]
	fn reserves_a_minimum_sized_final_payout() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			MinimumPayout::set(1_000_000);
			MinClaimInterval::set(40_000);
			set_time(450_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).claimed, 7_500_000);
			assert!(TOTAL - stored(0).claimed >= MinimumPayout::get());
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).claimed, TOTAL);
		});
	}

	#[test]
	fn waits_for_full_vesting_when_no_valid_non_final_payout_exists() {
		const SMALL_TOTAL: u128 = 1_500_000;
		new_test_ext(vec![(BOB, START, START, END, SMALL_TOTAL)]).execute_with(|| {
			MinimumPayout::set(1_000_000);
			// Nothing claimable has accrued yet: NothingToClaim, not WouldLeaveDust,
			// even though the remainder can never support a non-final payout.
			set_time(START + 100_000);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::NothingToClaim
			);
			set_time(START + 266_667);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::ClaimWouldLeaveDust
			);
			assert_eq!(stored(0).claimed, 0);
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).claimed, SMALL_TOTAL);
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
		new_test_ext(vec![default_schedule(BOB), (BOB, START, START, 900_000, 10_000_000)])
			.execute_with(|| {
				set_time(500_000);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 0));
				assert_eq!(free(&BOB), TOTAL);
				assert_eq!(stored(1).claimed, 0);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(BOB), 1));
				assert_eq!(free(&BOB), TOTAL + 5_000_000);
				assert_eq!(stored(1).claimed, 5_000_000);
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
				(CLIFF, START, END, TOTAL),       // start > cliff
				(START, END + 1, END, TOTAL),     // cliff > end
				(START, START, START, TOTAL),     // start == end
				(START, CLIFF, END, 999),         // total < ED
				(START, CLIFF, END, 0),           // total == 0
				(START, CLIFF, END, TOTAL + 500), // total not quantum-aligned
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
					TREASURY_FUNDS + 1_000
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
	fn rejects_a_non_zero_payout_below_the_minimum() {
		new_test_ext(vec![(BOB, START, START, END, TOTAL)]).execute_with(|| {
			set_time(START + 300);
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0),
				Error::<Test>::PayoutBelowMinimum
			);
			assert_eq!(stored(0).claimed, 0);
			set_time(START + 400);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_eq!(free(&BOB), MinimumPayout::get());
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
	fn rejects_the_pot_as_treasury_without_removing_the_schedule() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			let pot_before = free(&pot());
			TreasuryAccount::set(Some(pot()));
			set_time(300_000);
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::root(), 0),
				Error::<Test>::TreasuryNotConfigured
			);
			assert!(Schedules::<Test>::contains_key(0));
			assert_eq!(free(&BOB), 0);
			assert_eq!(free(&pot()), pot_before);
			TreasuryAccount::set(Some(TREASURY));
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

	/// A retarget replaces the same grantee's lost or stolen wallet, so it must pay
	/// the old address nothing — even when a permissionless claim could currently
	/// force a payout to it. The accrual follows the schedule to the new wallet.
	#[test]
	fn pays_the_outgoing_wallet_nothing_even_when_a_claim_could() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			// Half the grant is vested and freely claimable (`last_claim_at` is None).
			set_time(300_000);
			assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::signed(TREASURY), 0, CHARLIE));
			assert_eq!(free(&BOB), 0, "the lost/stolen wallet must not be paid");
			assert_eq!(stored(0).beneficiary, CHARLIE);
			assert_eq!(stored(0).claimed, 0);
			assert_eq!(stored(0).last_claim_at, None);
			System::assert_last_event(
				Event::ScheduleRetargeted {
					schedule_id: 0,
					old_beneficiary: BOB,
					new_beneficiary: CHARLIE,
				}
				.into(),
			);
			// The entire grant reaches the grantee's new wallet.
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&BOB), 0);
			assert_eq!(free(&CHARLIE), TOTAL);
		});
	}

	/// The report-89524 shape: retargeting inside the claim rate-limit window behaves
	/// exactly like retargeting outside it — nothing is paid to the old wallet and
	/// nothing is silently lost; the rotation merely redirects future payouts.
	#[test]
	fn behaves_identically_inside_the_claim_rate_limit_window() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			// Pays the fee-aligned 2_500_000 of the 3_750_000 vested; stamps
			// `last_claim_at` (any account can do this, e.g. before the wallet was
			// reported stolen).
			set_time(250_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&BOB), 2_500_000);

			// 50_000 into the 100_000 rate-limit window: rotate the wallet.
			set_time(300_000);
			assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::signed(TREASURY), 0, CHARLIE));
			assert_eq!(free(&BOB), 2_500_000, "no payout on retarget");
			assert_eq!(stored(0).claimed, 2_500_000);
			assert_eq!(stored(0).last_claim_at, Some(250_000), "no claim happened");

			// Everything unclaimed — including the amount accrued before the swap —
			// reaches the new wallet.
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), TOTAL - 2_500_000);
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
		new_test_ext_with_pot_balance(vec![], 0).execute_with(|| {
			assert_eq!(NextScheduleId::<Test>::get(), 0);
			assert_eq!(Schedules::<Test>::iter().count(), 0);
			assert_eq!(free(&pot()), 0);
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

mod quantization {
	use super::*;

	#[test]
	fn sub_quantum_claims_are_rejected_not_paid() {
		// The griefing scenario from review: per-block accrual above the ED but below
		// one wormhole quantum. A third party claiming every block must get an error —
		// never a sub-quantum payout that would land as a zero-value leaf.
		new_test_ext(vec![(CHARLIE, START, START, END, TOTAL)]).execute_with(|| {
			PayoutQuantum::set(3_000);
			// 10 per ms: 250ms in, 2_500 accrued — above the 1_000 ED, below one quantum.
			set_time(START + 250);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::NothingToClaim
			);
			assert_eq!(stored(0).claimed, 0);
			assert_eq!(free(&CHARLIE), 0);
			assert!(MockProofRecorder::recorded().is_empty());
		});
	}

	#[test]
	fn payouts_are_always_quantum_multiples_and_claimed_stays_aligned() {
		// A total aligned to the coarser 3_000 quantum this test switches to.
		const ALIGNED_TOTAL: u128 = 15_000_000;
		const ALIGNED_END: u64 = START + 300_000;
		new_test_ext(vec![(CHARLIE, START, START, ALIGNED_END, ALIGNED_TOTAL)]).execute_with(
			|| {
				PayoutQuantum::set(3_000);
				MinimumPayout::set(12_000);
				set_time(START + 1_250);
				assert_noop!(
					Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
					Error::<Test>::NothingToClaim
				);
				set_time(START + 150_000);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
				assert_eq!(free(&CHARLIE), non_final());
				assert_eq!(stored(0).claimed, non_final());
				assert_noop!(
					Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
					Error::<Test>::NothingToClaim
				);
				// Repeated eager third-party claims can never strand value: at `end`
				// the aligned total drains exactly.
				set_time(ALIGNED_END);
				assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
				assert_eq!(free(&CHARLIE), ALIGNED_TOTAL);
				assert_eq!(stored(0).claimed, ALIGNED_TOTAL);
				assert_eq!(free(&pot()), ExistentialDeposit::get());
				for (_, _, amount) in MockProofRecorder::recorded() {
					assert_eq!(amount % 3_000, 0, "every payout must be quantum-aligned");
				}
			},
		);
	}

	#[test]
	fn non_final_claims_hold_below_alignment_until_enough_accrues() {
		const SMALL_TOTAL: u128 = 6_000_000;
		new_test_ext(vec![(CHARLIE, START, START, END, SMALL_TOTAL)]).execute_with(|| {
			// A minimum-sized accrual is already leaf-aligned, but must not be paid —
			// that is the permissionless fragmentation case.
			set_time(START + 400);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::NothingToClaim
			);
			assert_eq!(stored(0).claimed, 0);
			assert!(MockProofRecorder::recorded().is_empty());
			set_time(START + 166_667);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), non_final());
			assert_eq!(stored(0).claimed, non_final());
			set_time(END);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(free(&CHARLIE), SMALL_TOTAL);
			assert_eq!(stored(0).claimed, SMALL_TOTAL);
			assert_eq!(
				MockProofRecorder::recorded(),
				vec![(pot(), CHARLIE, non_final()), (pot(), CHARLIE, SMALL_TOTAL - non_final())]
			);
		});
	}

	#[test]
	fn end_schedule_sends_sub_quantum_dust_to_treasury_not_the_beneficiary() {
		new_test_ext(vec![(BOB, START, START, END, TOTAL)]).execute_with(|| {
			PayoutQuantum::set(3_000);
			MinimumPayout::set(12_000);
			let treasury_before = free(&TREASURY);
			set_time(START + 1_250);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::signed(TREASURY), 0));
			assert_eq!(free(&BOB), 30_000);
			assert_eq!(free(&TREASURY), treasury_before + TOTAL - 30_000);
			assert_eq!(free(&pot()), ExistentialDeposit::get());
			System::assert_last_event(
				Event::ScheduleEnded {
					schedule_id: 0,
					beneficiary: BOB,
					vested_paid: 30_000,
					unvested_returned: TOTAL - 30_000,
				}
				.into(),
			);
		});
	}
}

mod proof_recording {
	use super::*;

	#[test]
	fn claim_records_the_payout() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(MockProofRecorder::recorded(), vec![(pot(), BOB, TOTAL / 2)]);
		});
	}

	#[test]
	fn end_schedule_records_only_the_beneficiary_payout() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			// Root origin — the scheduler-enacted governance path the event-scanning
			// extension never sees; the pallet must record the payout itself. The
			// treasury refund is signature-controlled and gets no leaf.
			set_time(300_000);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::root(), 0));
			assert_eq!(MockProofRecorder::recorded(), vec![(pot(), BOB, TOTAL / 2)]);
		});
	}

	#[test]
	fn failed_and_zero_payouts_record_nothing() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(CLIFF - 1);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(BOB), 0),
				Error::<Test>::NothingToClaim
			);
			// Ending before the cliff pays the beneficiary nothing: no payout, no leaf.
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::root(), 0));
			assert!(MockProofRecorder::recorded().is_empty());
		});
	}

	#[test]
	fn create_and_retarget_without_a_payout_record_nothing() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::signed(TREASURY),
				CHARLIE,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::root(), 0, ALICE));
			assert!(MockProofRecorder::recorded().is_empty());
		});
	}

	#[test]
	fn retarget_records_nothing_even_with_a_claimable_accrual() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			set_time(300_000);
			assert_ok!(Vesting::retarget_schedule(RuntimeOrigin::root(), 0, CHARLIE));
			assert!(MockProofRecorder::recorded().is_empty(), "retarget never pays out");
		});
	}

	/// The `TransferProofRecorder` contract permits `false` for a deliberately dropped
	/// credit. A payout without its proof material must not be finalized: the funds
	/// move but no wormhole leaf exists, and once `claimed` advances (or the schedule
	/// is removed) the payout can never be retried to create the missing proof. Every
	/// payout path must roll back completely and stay retryable.
	#[test]
	fn payout_paths_roll_back_when_the_recorder_drops_the_credit() {
		new_test_ext(vec![default_schedule(BOB)]).execute_with(|| {
			MockProofRecorder::set_drop_credits(true);
			set_time(300_000);

			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(PINGER), 0),
				Error::<Test>::PayoutProofNotRecorded
			);
			assert_eq!(stored(0).claimed, 0, "claim must not advance without a proof");
			assert_eq!(free(&BOB), 0, "the payout transfer must be rolled back");

			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::root(), 0),
				Error::<Test>::PayoutProofNotRecorded
			);
			assert!(
				Schedules::<Test>::contains_key(0),
				"the schedule must not be removed without a proof for its final payout"
			);

			// Once the recorder records again, the untouched schedule pays out normally.
			MockProofRecorder::set_drop_credits(false);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), 0));
			assert_eq!(stored(0).claimed, TOTAL / 2);
			assert_eq!(free(&BOB), TOTAL / 2);
			assert_eq!(MockProofRecorder::recorded(), vec![(pot(), BOB, TOTAL / 2)]);
		});
	}
}

mod unfunded_pot_bootstrap {
	use super::*;

	#[test]
	fn create_is_blocked_loudly_until_the_pot_gets_its_ed_buffer() {
		// The state of any chain where genesis did not endow the pot (this pallet is
		// deployed on fresh chains, so normally genesis does): schedule creation fails
		// with an explicit error until the treasury sends the pot one ED — the
		// documented manual bootstrap, no migration involved.
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
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(TREASURY),
				pot(),
				ExistentialDeposit::get()
			));
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::signed(TREASURY),
				BOB,
				START,
				CLIFF,
				END,
				TOTAL
			));
			assert_ok!(Vesting::do_try_state());
		});
	}
}

mod random_walk {
	use super::*;

	/// Deterministic xorshift64 so failures are reproducible from the seed.
	fn next(state: &mut u64) -> u64 {
		let mut x = *state;
		x ^= x << 13;
		x ^= x >> 7;
		x ^= x << 17;
		*state = x;
		x
	}

	/// Several schedules of different shapes under a pseudo-random walk of time and
	/// permissionless claims: claims may fail only for the documented reasons, the
	/// storage invariants hold at every step, every recorded payment is
	/// quantum-aligned and at least the minimum, and once drained every planck of
	/// every grant has reached exactly its beneficiary.
	#[test]
	fn random_claim_walks_pay_exact_quantized_totals() {
		let schedules: Vec<ScheduleTuple> = vec![
			// Cliffed mid-size grant.
			(BOB, 100_000, 200_000, 500_000, 10_000_000),
			// Second grant on the same account, no cliff, longest end.
			(BOB, 100_000, 100_000, 900_000, 8_000_000),
			// Cliff at half of vesting.
			(CHARLIE, 0, 300_000, 600_000, 5_000_000),
			// Small grant below the non-final alignment: payable only as the exact
			// final claim once fully vested.
			(ALICE, 50_000, 50_000, 250_000, 40_000),
		];
		let count = schedules.len() as u64;
		let last_end = 900_000u64;
		for seed in [1u64, 0xDEAD_BEEF, 424_242, 987_654_321] {
			new_test_ext(schedules.clone()).execute_with(|| {
				let mut rng = seed;
				let mut now = 0u64;
				for _ in 0..300 {
					// Mostly small steps inside the rate-limit window, some
					// window-sized ones, an occasional jump past several ends.
					let r = next(&mut rng);
					now += match r % 10 {
						0 => 0,
						1..=6 => r % 40_000,
						7 | 8 => r % 200_000,
						_ => r % 700_000,
					};
					set_time(now);
					let id = next(&mut rng) % count;
					if let Err(e) = Vesting::claim(RuntimeOrigin::signed(PINGER), id) {
						let benign = [
							Error::<Test>::NothingToClaim.into(),
							Error::<Test>::ClaimTooSoon.into(),
							Error::<Test>::ClaimWouldLeaveDust.into(),
						];
						assert!(
							benign.contains(&e),
							"seed {seed}: unexpected claim error {e:?} at t={now}"
						);
					}
					assert_ok!(Vesting::do_try_state());
				}

				// Drain: past every end the final claim pays the exact remainder,
				// so each round only has to wait out the rate limit.
				now = now.max(last_end);
				let mut rounds = 0;
				while (0..count).any(|id| stored(id).claimed < stored(id).total) {
					rounds += 1;
					assert!(rounds <= 3, "seed {seed}: drain did not converge");
					now += MinClaimInterval::get();
					set_time(now);
					for id in 0..count {
						if stored(id).claimed < stored(id).total {
							assert_ok!(Vesting::claim(RuntimeOrigin::signed(PINGER), id));
						}
					}
					assert_ok!(Vesting::do_try_state());
				}

				// Every grant landed with its beneficiary in full; the pot keeps
				// exactly its ED buffer.
				assert_eq!(free(&BOB), 18_000_000);
				assert_eq!(free(&CHARLIE), 5_000_000);
				assert_eq!(free(&ALICE), 40_000);
				assert_eq!(free(&pot()), ExistentialDeposit::get());

				// Every individual payment was quantized and claim-worthy, and the
				// payment stream reconstructs each grant exactly.
				let mut per_beneficiary = std::collections::BTreeMap::new();
				for (from, to, amount) in MockProofRecorder::recorded() {
					assert_eq!(from, pot());
					assert_eq!(
						amount % PayoutQuantum::get(),
						0,
						"seed {seed}: unaligned payout {amount}"
					);
					assert!(amount >= MinimumPayout::get(), "seed {seed}: dust payout {amount}");
					*per_beneficiary.entry(to).or_insert(0u128) += amount;
				}
				assert_eq!(per_beneficiary.get(&BOB), Some(&18_000_000));
				assert_eq!(per_beneficiary.get(&CHARLIE), Some(&5_000_000));
				assert_eq!(per_beneficiary.get(&ALICE), Some(&40_000));
			});
		}
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
