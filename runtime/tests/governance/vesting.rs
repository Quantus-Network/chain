//! Integration tests for the vesting pallet's runtime couplings that pallet unit tests
//! cannot see: the `EnsureTreasury` admin origin exercised through the *real* treasury
//! multisig, and the wormhole proof recorder consuming claim payout events.

#[cfg(test)]
mod tests {
	use codec::Encode;
	use frame_support::{assert_noop, assert_ok, traits::Currency};
	use pallet_multisig::BoundedCallOf;
	use qp_wormhole::TransferProofRecorder;
	use quantus_runtime::{
		AccountId, AssetId, Balance, Balances, Multisig, Runtime, RuntimeCall, RuntimeEvent,
		RuntimeOrigin, System, Vesting, Wormhole, EXISTENTIAL_DEPOSIT, UNIT,
	};
	use sp_core::crypto::AccountId32;
	use sp_runtime::{BuildStorage, DispatchError, Permill};

	const END_MS: u64 = 1_000_000;
	const GRANT: Balance = 100 * UNIT;

	fn account(id: u8) -> AccountId32 {
		let mut bytes = [0u8; 32];
		bytes[0] = id;
		AccountId32::new(bytes)
	}

	fn signers() -> Vec<AccountId> {
		vec![account(1), account(2), account(3)]
	}

	fn treasury_multisig() -> AccountId {
		pallet_multisig::Pallet::<Runtime>::derive_multisig_address(&signers(), 2, 0)
	}

	fn new_test_ext(treasury: Option<AccountId>) -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: treasury.clone(),
			treasury_portion: treasury.map(|_| Permill::from_percent(50)),
		}
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
	fn claim_payout_event_feeds_the_wormhole_proof_recorder() {
		new_test_ext(Some(account(4))).execute_with(|| {
			Balances::make_free_balance_be(&account(4), 1000 * UNIT);
			// The beneficiary never signs anything — exactly like a wormhole address.
			let beneficiary = account(9);
			let pot = Vesting::pot_account_id();
			assert_ok!(Vesting::create_schedule(
				RuntimeOrigin::root(),
				beneficiary.clone(),
				0,
				0,
				END_MS,
				GRANT,
			));
			set_time(END_MS);
			System::reset_events();
			assert_ok!(Vesting::claim(RuntimeOrigin::signed(account(1)), 0));

			// The payout is an ordinary `Balances::Transfer` — the exact event shape the
			// `WormholeProofRecorderExtension` scans in `post_dispatch`.
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
			assert_eq!(payout, GRANT);

			// Feeding that event into the recorder (as post_dispatch does) records a
			// ZK-tree leaf for the beneficiary — this is what lets a wormhole owner
			// later exit the funds via ZK proof.
			let count_before = Wormhole::transfer_count(&beneficiary);
			assert!(<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::
				record_transfer_proof(None, pot, beneficiary.clone(), payout));
			assert_eq!(Wormhole::transfer_count(&beneficiary), count_before + 1);
		});
	}
}
