//! Integration tests for the vesting pallet's runtime couplings that pallet unit tests
//! cannot see: the `EnsureTreasury` admin origin exercised through the *real* treasury
//! multisig, and the wormhole proof recorder consuming claim payout events.

#[cfg(test)]
mod tests {
	use crate::common::TestCommons;
	use codec::Encode;
	use frame_support::{assert_noop, assert_ok, traits::Currency};
	use pallet_multisig::BoundedCallOf;
	use quantus_runtime::{
		configs::{
			VestingMinClaimInterval, VestingMinimumPayout, VestingPayoutQuantum, VolumeFeeRateBps,
		},
		AccountId, Balance, Balances, Multisig, Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin,
		System, Vesting, Wormhole, EXISTENTIAL_DEPOSIT, UNIT,
	};
	use sp_core::crypto::AccountId32;
	use sp_runtime::{BuildStorage, DispatchError};

	const END_MS: u64 = 1_000_000;
	const GRANT: Balance = 100 * UNIT;

	fn account(id: u8) -> AccountId32 {
		TestCommons::account_id(id)
	}

	fn signers() -> Vec<AccountId> {
		vec![account(1), account(2), account(3)]
	}

	fn treasury_multisig() -> AccountId {
		pallet_multisig::Pallet::<Runtime>::derive_multisig_address(&signers(), 2, 0)
	}

	fn new_test_ext(treasury: Option<AccountId>) -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
		pallet_treasury::GenesisConfig::<Runtime> { treasury_account: treasury.clone() }
			.assimilate_storage(&mut t)
			.unwrap();
		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| {
			System::set_block_number(1);
			for signer in signers() {
				Balances::make_free_balance_be(&signer, 1000 * UNIT);
			}
			Balances::make_free_balance_be(&Vesting::pot_account_id(), EXISTENTIAL_DEPOSIT);
		});
		ext
	}

	fn set_time(now_ms: u64) {
		pallet_timestamp::Now::<Runtime>::put(now_ms);
	}

	fn max_exitable_from_recorded_leaves(beneficiary: &AccountId) -> (usize, Balance) {
		let quantum = VestingPayoutQuantum::get();
		let fee_bps = VolumeFeeRateBps::get() as u128;
		let leaves: Vec<_> = pallet_zk_tree::Leaves::<Runtime>::iter_values()
			.filter(|leaf| &leaf.to == beneficiary)
			.collect();
		let max_exit = leaves
			.iter()
			.map(|leaf| {
				let input_quanta = leaf.amount / quantum;
				input_quanta * (10_000 - fee_bps) / 10_000 * quantum
			})
			.sum();
		(leaves.len(), max_exit)
	}

	fn propose_approve_execute(call: RuntimeCall, proposal_id: u32) {
		let treasury = treasury_multisig();
		let encoded: BoundedCallOf<Runtime> = call.encode().try_into().unwrap();
		let expiry = System::block_number() + 100;
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(account(1)),
			treasury.clone(),
			encoded.clone(),
			expiry,
		));
		assert_ok!(Multisig::approve(
			RuntimeOrigin::signed(account(2)),
			treasury.clone(),
			proposal_id,
			encoded,
		));
		assert_ok!(Multisig::execute(RuntimeOrigin::signed(account(3)), treasury, proposal_id));
	}

	#[test]
	fn payout_policy_covers_account_and_wormhole_minimums() {
		let quantum = VestingPayoutQuantum::get();
		let minimum = VestingMinimumPayout::get();
		let quantized_minimum = minimum / quantum;
		let max_quantized_output =
			quantized_minimum * (10_000 - VolumeFeeRateBps::get() as u128) / 10_000;

		assert!(minimum > EXISTENTIAL_DEPOSIT);
		assert!(minimum >= 2 * quantum);
		assert_eq!(minimum % quantum, 0);
		assert!(max_quantized_output > 0);
		assert!(max_quantized_output < quantized_minimum);
		assert_eq!(VestingMinClaimInterval::get(), 24 * 60 * 60 * 1000);
		assert_eq!(quantum * pallet_vesting::NON_FINAL_PAYOUT_QUANTA, 25 * UNIT);
		assert_eq!(
			pallet_vesting::NON_FINAL_PAYOUT_QUANTA * VolumeFeeRateBps::get() as u128 % 10_000,
			0
		);
	}

	#[test]
	fn end_schedule_does_not_record_a_one_quantum_leaf() {
		new_test_ext(Some(account(4))).execute_with(|| {
			let treasury = account(4);
			let beneficiary = account(9);
			Balances::make_free_balance_be(&treasury, 1000 * UNIT);
			let total = 100 * UNIT;
			let quantum = VestingPayoutQuantum::get();
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				beneficiary.clone(),
				0,
				0,
				END_MS,
				total,
			));
			// One quantum vested — nearest is one quantum, below MinimumPayout.
			set_time((END_MS as u128 * quantum / total) as u64);
			let leaves_before = Wormhole::transfer_count(&beneficiary);
			assert_ok!(Vesting::end_schedule(RuntimeOrigin::root(), 0));
			assert_eq!(Balances::total_balance(&beneficiary), 0);
			assert_eq!(Wormhole::transfer_count(&beneficiary), leaves_before);
			assert_eq!(Balances::total_balance(&treasury), 1000 * UNIT);
			assert_eq!(max_exitable_from_recorded_leaves(&beneficiary), (0, 0));
		});
	}

	#[test]
	fn treasury_multisig_creates_and_ends_schedules() {
		new_test_ext(Some(treasury_multisig())).execute_with(|| {
			let treasury = treasury_multisig();
			let beneficiary = account(7);
			let pot = Vesting::pot_account_id();
			assert_ok!(Multisig::create_multisig(
				RuntimeOrigin::signed(account(1)),
				signers(),
				2,
				0,
			));
			Balances::make_free_balance_be(&treasury, 1000 * UNIT);

			propose_approve_execute(
				RuntimeCall::Vesting(pallet_vesting::Call::create_schedule {
					beneficiary: beneficiary.clone(),
					start: 0,
					cliff: 0,
					end: END_MS,
					total: GRANT,
				}),
				0,
			);
			let schedule =
				pallet_vesting::Schedules::<Runtime>::get(0).expect("schedule must be created");
			assert_eq!(schedule.beneficiary, beneficiary);
			assert_eq!(Balances::total_balance(&pot), GRANT + EXISTENTIAL_DEPOSIT);
			assert_eq!(Balances::total_balance(&treasury), 900 * UNIT);

			// Halfway through the schedule: ending it splits the grant exactly.
			set_time(END_MS / 2);
			propose_approve_execute(
				RuntimeCall::Vesting(pallet_vesting::Call::end_schedule { schedule_id: 0 }),
				1,
			);
			assert!(pallet_vesting::Schedules::<Runtime>::get(0).is_none());
			assert_eq!(Balances::total_balance(&beneficiary), GRANT / 2);
			assert_eq!(Balances::total_balance(&treasury), 950 * UNIT);
			assert_eq!(Balances::total_balance(&pot), EXISTENTIAL_DEPOSIT);
		});
	}

	#[test]
	fn non_treasury_origins_are_rejected() {
		new_test_ext(Some(treasury_multisig())).execute_with(|| {
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(account(1)),
					account(7),
					0,
					0,
					END_MS,
					GRANT,
				),
				DispatchError::BadOrigin
			);
			assert_noop!(
				Vesting::end_schedule(RuntimeOrigin::signed(account(1)), 0),
				DispatchError::BadOrigin
			);
			assert_noop!(
				Vesting::retarget_schedule(RuntimeOrigin::signed(account(1)), 0, account(8)),
				DispatchError::BadOrigin
			);

			// Root is the break-glass admin and works without the multisig.
			Balances::make_free_balance_be(&treasury_multisig(), 1000 * UNIT);
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				account(7),
				0,
				0,
				END_MS,
				GRANT,
			));
		});
	}

	#[test]
	fn unconfigured_treasury_fails_loudly() {
		new_test_ext(None).execute_with(|| {
			// Root passes the origin check but the pallet still refuses: there is no
			// treasury to fund from or refund to.
			assert_noop!(
				Vesting::create_schedule(RuntimeOrigin::root(), account(7), 0, 0, END_MS, GRANT),
				pallet_vesting::Error::<Runtime>::TreasuryNotConfigured
			);
			// A signed origin cannot match an unconfigured treasury either.
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::signed(account(1)),
					account(7),
					0,
					0,
					END_MS,
					GRANT,
				),
				DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn one_quan_schedule_claims_exactly_once_and_records_one_wormhole_leaf() {
		new_test_ext(Some(account(4))).execute_with(|| {
			Balances::make_free_balance_be(&account(4), 1000 * UNIT);
			let minimum = VestingMinimumPayout::get();
			assert_eq!(minimum, UNIT);
			// The beneficiary never signs anything — exactly like a wormhole address.
			let beneficiary = account(9);
			let pot = Vesting::pot_account_id();
			assert_noop!(
				Vesting::create_schedule(
					RuntimeOrigin::root(),
					beneficiary.clone(),
					0,
					0,
					END_MS,
					minimum - VestingPayoutQuantum::get(),
				),
				pallet_vesting::Error::<Runtime>::InvalidSchedule
			);
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				beneficiary.clone(),
				0,
				0,
				END_MS,
				minimum,
			));
			set_time(END_MS - 1);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(account(1)), 0),
				pallet_vesting::Error::<Runtime>::NothingToClaim
			);
			set_time(END_MS);
			System::reset_events();
			let count_before = Wormhole::transfer_count(&beneficiary);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(account(1)), 0));

			// The pallet records the payout itself, exactly once — this ZK-tree leaf is
			// what lets a wormhole owner later exit the funds via ZK proof. (The
			// event-scanning extension skips pot-sourced transfers, so signed
			// submissions don't add a second leaf — covered by the extension's own
			// unit test.)
			assert_eq!(Wormhole::transfer_count(&beneficiary), count_before + 1);

			// The payout itself is an ordinary keep-alive `Transfer` from the pot.
			let payout = System::events()
				.into_iter()
				.find_map(|record| match record.event {
					RuntimeEvent::Balances(pallet_balances::Event::Transfer {
						from,
						to,
						amount,
					}) if from == pot && to == beneficiary => Some(amount),
					_ => None,
				})
				.expect("claim must emit a plain Transfer event from the pot");
			assert_eq!(payout, UNIT);
			assert_eq!(Balances::total_balance(&beneficiary), UNIT);
			assert_eq!(Balances::total_balance(&pot), EXISTENTIAL_DEPOSIT);
			let schedule = pallet_vesting::Schedules::<Runtime>::get(0).unwrap();
			assert_eq!(schedule.total, UNIT);
			assert_eq!(schedule.claimed, UNIT);
			assert_noop!(
				Vesting::claim(RuntimeOrigin::signed(account(1)), 0),
				pallet_vesting::Error::<Runtime>::NothingToClaim
			);
		});
	}

	#[test]
	fn non_final_alignment_stops_daily_fragmentation_from_reducing_exit_value() {
		const DAY: u64 = 24 * 60 * 60 * 1000;
		const DAYS: u64 = 365;
		const TOTAL: Balance = DAYS as Balance * UNIT;
		let beneficiary = account(9);

		let (periodic_leaves, periodic_max_exit) =
			new_test_ext(Some(account(4))).execute_with(|| {
				Balances::make_free_balance_be(&account(4), 1000 * UNIT);
				assert_ok!(Vesting::create_schedule(
					RuntimeOrigin::root(),
					beneficiary.clone(),
					0,
					0,
					DAYS * DAY,
					TOTAL,
				));
				set_time(DAY);
				assert_noop!(
					Vesting::claim(RuntimeOrigin::signed(account(8)), 0),
					pallet_vesting::Error::<Runtime>::NothingToClaim
				);
				for day in 1..=DAYS {
					set_time(day * DAY);
					let _ = Vesting::claim(RuntimeOrigin::signed(account(8)), 0);
				}
				assert_eq!(pallet_vesting::Schedules::<Runtime>::get(0).unwrap().claimed, TOTAL);
				max_exitable_from_recorded_leaves(&beneficiary)
			});

		let (single_leaves, single_max_exit) = new_test_ext(Some(account(4))).execute_with(|| {
			Balances::make_free_balance_be(&account(4), 1000 * UNIT);
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				beneficiary.clone(),
				0,
				0,
				DAYS * DAY,
				TOTAL,
			));
			set_time(DAYS * DAY);
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(account(8)), 0));
			max_exitable_from_recorded_leaves(&beneficiary)
		});

		assert_eq!(single_leaves, 1);
		assert_eq!(periodic_leaves, 15);
		assert_eq!(periodic_max_exit, single_max_exit);
		assert_eq!(single_max_exit, 36_485 * VestingPayoutQuantum::get());
	}

	#[test]
	fn scheduler_enacted_root_end_schedule_records_the_payout_leaf() {
		use frame_support::traits::{
			schedule::{v3::Anon, DispatchTime},
			Hooks, StorePreimage,
		};
		use quantus_runtime::{OriginCaller, Scheduler};

		new_test_ext(Some(account(4))).execute_with(|| {
			Balances::make_free_balance_be(&account(4), 1000 * UNIT);
			let beneficiary = account(9);
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				beneficiary.clone(),
				0,
				0,
				END_MS,
				GRANT,
			));
			// Halfway vested at enactment time.
			set_time(END_MS / 2);

			// Schedule `end_schedule` as Root — the exact shape of a governance
			// enactment. The scheduler dispatches it from `on_initialize`, entirely
			// outside the signed-extrinsic pipeline: no transaction extension runs.
			let call = RuntimeCall::Vesting(pallet_vesting::Call::end_schedule { schedule_id: 0 });
			let bounded = <Runtime as pallet_scheduler::Config>::Preimages::bound(call)
				.expect("small call bounds inline");
			assert_ok!(<Scheduler as Anon<_, _, _>>::schedule(
				DispatchTime::At(3),
				None,
				0,
				OriginCaller::system(frame_system::RawOrigin::Root),
				bounded,
			));

			let count_before = Wormhole::transfer_count(&beneficiary);
			while System::block_number() < 3 {
				let block = System::block_number();
				Scheduler::on_finalize(block);
				System::set_block_number(block + 1);
				Scheduler::on_initialize(block + 1);
			}

			// The schedule was ended by the hook-dispatched Root call: the beneficiary
			// got the vested half, the treasury the rest — and the payout leaf exists
			// even though no extension ever saw this dispatch.
			assert!(pallet_vesting::Schedules::<Runtime>::get(0).is_none());
			assert_eq!(Balances::total_balance(&beneficiary), GRANT / 2);
			assert_eq!(Wormhole::transfer_count(&beneficiary), count_before + 1);
		});
	}
}
