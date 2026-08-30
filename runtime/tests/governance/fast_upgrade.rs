//! Fast-upgrade track (track 1): an emergency runtime-upgrade lane modeled on
//! Polkadot's Whitelisted Caller track. Proposals dispatch with the custom
//! `FastUpgrade` origin, which only `system.authorize_upgrade` honors, and the
//! 80%/80% constant curves require 8-of-10 ayes from the genesis collective.

#[cfg(test)]
mod tests {
	use crate::common::TestCommons;
	use codec::Encode;
	use frame_support::{assert_ok, traits::Currency};
	use pallet_referenda::TracksInfo;
	use quantus_runtime::{
		configs::TechReferendaInstance,
		genesis_config_presets::governance_treasury_signer_seed,
		governance::definitions::{TechCollectiveTracksInfo, FAST_UPGRADE_TRACK_ID},
		pallet_custom_origins, Balances, OriginCaller, Preimage, Runtime, RuntimeCall,
		RuntimeOrigin, System, TechCollective, TechReferenda,
	};
	use sp_runtime::{traits::Hash, MultiAddress, Perbill};

	fn fast_upgrade_origin() -> OriginCaller {
		OriginCaller::Origins(pallet_custom_origins::Origin::FastUpgrade)
	}

	fn upgrade_hash(seed: &[u8]) -> <Runtime as frame_system::Config>::Hash {
		<Runtime as frame_system::Config>::Hashing::hash(seed)
	}

	/// Seed a 10-member collective (accounts 1..=10, mirroring the mainnet genesis
	/// collective size) and fund the proposer for the preimage + submission +
	/// decision deposits. Returns the proposer.
	fn seed_ten_member_collective() -> sp_core::crypto::AccountId32 {
		for i in 1..=10u8 {
			assert_ok!(TechCollective::add_member(
				RuntimeOrigin::root(),
				MultiAddress::from(TestCommons::account_id(i))
			));
		}
		let proposer = TestCommons::account_id(1);
		Balances::make_free_balance_be(&proposer, governance_treasury_signer_seed(10));
		proposer
	}

	/// Submit a referendum with the `FastUpgrade` proposal origin, place the
	/// decision deposit, and return its index.
	fn submit_fast_track_referendum(
		proposer: &sp_core::crypto::AccountId32,
		call: RuntimeCall,
	) -> u32 {
		let encoded = call.encode();
		let preimage_hash = <Runtime as frame_system::Config>::Hashing::hash(&encoded);
		assert_ok!(Preimage::note_preimage(
			RuntimeOrigin::signed(proposer.clone()),
			encoded.clone()
		));
		assert_ok!(TechReferenda::submit(
			RuntimeOrigin::signed(proposer.clone()),
			Box::new(fast_upgrade_origin()),
			frame_support::traits::Bounded::Lookup {
				hash: preimage_hash,
				len: encoded.len() as u32,
			},
			frame_support::traits::schedule::DispatchTime::After(0u32)
		));
		let index = pallet_referenda::ReferendumCount::<Runtime, TechReferendaInstance>::get() - 1;
		assert_ok!(TechReferenda::place_decision_deposit(
			RuntimeOrigin::signed(proposer.clone()),
			index
		));
		index
	}

	fn vote(member: u8, index: u32, aye: bool) {
		assert_ok!(TechCollective::vote(
			RuntimeOrigin::signed(TestCommons::account_id(member)),
			index,
			aye
		));
	}

	fn run_out_the_track(track_id: u16) {
		let info = <TechCollectiveTracksInfo as TracksInfo<_, _>>::info(track_id)
			.expect("track must exist");
		let target = System::block_number() +
			TestCommons::calculate_governance_blocks(
				info.prepare_period,
				info.decision_period,
				info.confirm_period,
				info.min_enactment_period,
			);
		TestCommons::run_to_block(target);
	}

	fn approved(index: u32) -> bool {
		matches!(
			pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(index),
			Some(pallet_referenda::ReferendumInfo::Approved(..))
		)
	}

	fn rejected(index: u32) -> bool {
		matches!(
			pallet_referenda::ReferendumInfoFor::<Runtime, TechReferendaInstance>::get(index),
			Some(pallet_referenda::ReferendumInfo::Rejected(..))
		)
	}

	fn authorized_upgrade() -> Option<frame_system::CodeUpgradeAuthorization<Runtime>> {
		frame_system::Pallet::<Runtime>::authorized_upgrade()
	}

	/// Only the `FastUpgrade` custom origin routes to the fast track; Root keeps
	/// the normal track and signed origins map to no track at all.
	#[test]
	fn fast_track_is_reachable_only_via_the_custom_origin() {
		assert_eq!(
			TechCollectiveTracksInfo::track_for(&OriginCaller::system(
				frame_system::RawOrigin::Root
			)),
			Ok(0)
		);
		assert_eq!(
			TechCollectiveTracksInfo::track_for(&fast_upgrade_origin()),
			Ok(FAST_UPGRADE_TRACK_ID)
		);
		assert!(TechCollectiveTracksInfo::track_for(&OriginCaller::system(
			frame_system::RawOrigin::Signed(TestCommons::account_id(1))
		))
		.is_err());
	}

	/// The curves are constant 80%/80%: with the 10-member genesis collective that
	/// is exactly 8-of-10 ayes, at any point of the decision period.
	#[test]
	fn fast_track_curves_pin_eight_of_ten() {
		let info = <TechCollectiveTracksInfo as TracksInfo<_, _>>::info(FAST_UPGRADE_TRACK_ID)
			.expect("fast track must exist");
		for x in [Perbill::zero(), Perbill::from_percent(50), Perbill::one()] {
			assert_eq!(info.min_approval.threshold(x), Perbill::from_percent(80));
			assert_eq!(info.min_support.threshold(x), Perbill::from_percent(80));
		}
	}

	/// Happy path: 8 ayes / 2 nays authorizes the upgrade hash (support 8/10 = 80%
	/// and approval 8/(8+2) = 80% both pass on the constant curves).
	#[test]
	fn eight_of_ten_authorizes_the_upgrade() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = seed_ten_member_collective();
			let code_hash = upgrade_hash(b"fake runtime wasm");
			let index = submit_fast_track_referendum(
				&proposer,
				RuntimeCall::System(frame_system::Call::authorize_upgrade { code_hash }),
			);

			for member in 1..=8u8 {
				vote(member, index, true);
			}
			vote(9, index, false);
			vote(10, index, false);

			run_out_the_track(FAST_UPGRADE_TRACK_ID);

			assert!(approved(index), "8-of-10 with 2 nays must approve on the fast track");
			let authorized = authorized_upgrade()
				.expect("approved fast-track referendum must authorize the upgrade");
			assert_eq!(*authorized.code_hash(), code_hash);
		});
	}

	/// 7 ayes / 3 nays fails both 80% thresholds: the referendum is rejected and
	/// nothing is authorized.
	#[test]
	fn seven_of_ten_is_not_enough() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = seed_ten_member_collective();
			let index = submit_fast_track_referendum(
				&proposer,
				RuntimeCall::System(frame_system::Call::authorize_upgrade {
					code_hash: upgrade_hash(b"insufficient support"),
				}),
			);

			for member in 1..=7u8 {
				vote(member, index, true);
			}
			for member in 8..=10u8 {
				vote(member, index, false);
			}

			run_out_the_track(FAST_UPGRADE_TRACK_ID);

			assert!(rejected(index), "7-of-10 must not pass the fast track");
			assert!(
				authorized_upgrade().is_none(),
				"a rejected referendum must not authorize anything"
			);
		});
	}

	/// The scoping property: the fast lane cannot do arbitrary Root business. A
	/// Root-gated call (here `TechCollective::add_member`) proposed on the fast
	/// track passes the vote but fails at dispatch with `BadOrigin`, because it is
	/// dispatched with the `FastUpgrade` origin — not Root.
	#[test]
	fn fast_track_cannot_dispatch_arbitrary_root_calls() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let proposer = seed_ten_member_collective();
			let candidate = TestCommons::account_id(42);
			let index = submit_fast_track_referendum(
				&proposer,
				RuntimeCall::TechCollective(pallet_ranked_collective::Call::add_member {
					who: MultiAddress::from(candidate.clone()),
				}),
			);

			for member in 1..=10u8 {
				vote(member, index, true);
			}

			run_out_the_track(FAST_UPGRADE_TRACK_ID);

			assert!(approved(index), "the vote itself passes");
			assert!(
				!pallet_ranked_collective::Members::<Runtime>::contains_key(&candidate),
				"a Root-gated call must not execute under the FastUpgrade origin"
			);
		});
	}

	/// Direct calls: Root still authorizes upgrades, plain signed accounts never do.
	#[test]
	fn direct_authorize_upgrade_origin_checks() {
		TestCommons::new_fast_governance_test_ext().execute_with(|| {
			let hash = upgrade_hash(b"root direct");
			assert!(frame_system::Pallet::<Runtime>::authorize_upgrade(
				RuntimeOrigin::signed(TestCommons::account_id(1)),
				hash
			)
			.is_err());
			assert!(authorized_upgrade().is_none());
			assert_ok!(frame_system::Pallet::<Runtime>::authorize_upgrade(
				RuntimeOrigin::root(),
				hash
			));
			assert_eq!(
				*authorized_upgrade().expect("root authorization must persist").code_hash(),
				hash
			);
		});
	}
}
