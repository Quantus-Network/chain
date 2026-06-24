use crate::{AccountId, Balance, Balances, BlockNumber, Runtime, RuntimeOrigin, DAYS, HOURS, UNIT};
use alloc::borrow::Cow;
use codec::{Decode, Encode, MaxEncodedLen};
use core::marker::PhantomData;
#[cfg(feature = "runtime-benchmarks")]
use frame_support::traits::Currency;
use frame_support::{
	pallet_prelude::TypeInfo,
	traits::{
		CallerTrait, Consideration, EnsureOriginWithArg, Footprint, Get, OriginTrait,
		ReservableCurrency,
	},
};
use lazy_static::lazy_static;
use sp_core::crypto::AccountId32;
use sp_runtime::{
	str_array,
	traits::{Convert, MaybeConvert},
	DispatchError, Perbill,
};
///Preimage pallet fee model

#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
pub struct PreimageDeposit {
	amount: Balance,
}

impl Consideration<AccountId, Footprint> for PreimageDeposit {
	fn new(who: &AccountId, footprint: Footprint) -> Result<Self, DispatchError> {
		// Fee model: 0.1 UNIT base + 0.0001 UNIT/byte (#91165: per-byte was 1000x too low).
		let base = UNIT / 10;
		let per_byte = UNIT / 10_000;
		let size = (footprint.size as u128).saturating_add(footprint.count as u128);
		let amount = base.saturating_add(per_byte.saturating_mul(size));

		Balances::reserve(who, amount)?;
		Ok(Self { amount })
	}

	fn update(self, who: &AccountId, new_footprint: Footprint) -> Result<Self, DispatchError> {
		// Calculate new amount
		let base = UNIT / 10;
		let per_byte = UNIT / 10_000;
		let size = (new_footprint.size as u128).saturating_add(new_footprint.count as u128);
		let new_amount = base.saturating_add(per_byte.saturating_mul(size));

		// Release old deposite
		Balances::unreserve(who, self.amount);

		// Take new deposite
		Balances::reserve(who, new_amount)?;

		Ok(Self { amount: new_amount })
	}

	fn drop(self, who: &AccountId) -> Result<(), DispatchError> {
		Balances::unreserve(who, self.amount);
		Ok(())
	}

	///We will have to finally focus on fees, so weight and benchamrks will be important.
	/// For now, it's AI implementation

	#[cfg(feature = "runtime-benchmarks")]
	fn ensure_successful(who: &AccountId, footprint: Footprint) {
		let base = UNIT / 10;
		let per_byte = UNIT / 10_000;
		let size = (footprint.size as u128).saturating_add(footprint.count as u128);
		let amount = base.saturating_add(per_byte.saturating_mul(size));

		// Check if user has enough coins
		if Balances::free_balance(who) < amount {
			Balances::make_free_balance_be(who, amount.saturating_mul(2));
		}
	}
}

/// Collapses every referenda timing window to 2 blocks when the
/// `fast-governance` feature is enabled. Only compiled into test builds; production
/// builds never reference this function or its callers.
#[cfg(feature = "fast-governance")]
fn apply_test_timing(
	mut info: pallet_referenda::TrackInfo<Balance, BlockNumber>,
) -> pallet_referenda::TrackInfo<Balance, BlockNumber> {
	info.prepare_period = 2;
	info.decision_period = 2;
	info.confirm_period = 2;
	info.min_enactment_period = 2;
	info
}

// The community/public referenda lane (and its `CommunityTracksInfo`) was removed. The
// tech-collective lane below is the sole governance lane (runtime upgrades + other Root calls).
pub struct TechCollectiveTracksInfo;

impl TechCollectiveTracksInfo {
	fn create_tech_collective_tracks() -> [pallet_referenda::Track<u16, Balance, BlockNumber>; 1] {
		// With 5 members: >=3 ayes required (support 60%), and 2 nays always block
		// (3 ayes / 2 nays = 60% approval < 61%). Constant curves: thresholds don't
		// decay over the decision period. The 24h confirm period guarantees nays can
		// arrive for a full day before any approval; enactment is delayed another 24h.
		let info = pallet_referenda::TrackInfo {
			name: str_array("tech_collective_members"),
			max_deciding: 1,
			decision_deposit: 1000 * UNIT,
			// Advance-notice window before deciding starts. Raised from 4 min to give the collective
			// (and observers) visibility of a pending Root proposal before voting can conclude.
			prepare_period: 2 * HOURS,
			decision_period: DAYS,
			confirm_period: DAYS,
			min_enactment_period: DAYS,
			min_approval: pallet_referenda::Curve::LinearDecreasing {
				length: Perbill::from_percent(100),
				floor: Perbill::from_percent(61),
				ceil: Perbill::from_percent(61),
			},
			min_support: pallet_referenda::Curve::LinearDecreasing {
				length: Perbill::from_percent(100),
				floor: Perbill::from_percent(60),
				ceil: Perbill::from_percent(60),
			},
		};
		#[cfg(feature = "fast-governance")]
		let info = apply_test_timing(info);
		[pallet_referenda::Track { id: 0, info }]
	}
}

impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TechCollectiveTracksInfo {
	type Id = u16;
	type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

	fn tracks(
	) -> impl Iterator<Item = Cow<'static, pallet_referenda::Track<Self::Id, Balance, BlockNumber>>>
	{
		lazy_static! {
			static ref TRACKS: [pallet_referenda::Track<u16, Balance, BlockNumber>; 1] =
				TechCollectiveTracksInfo::create_tech_collective_tracks();
		}
		TRACKS.iter().map(Cow::Borrowed)
	}

	fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
		// #91247/#91270: only a `Root` proposal origin is accepted. A referendum's `proposal_origin`
		// is stored and dispatched verbatim on approval, so accepting `Signed(_)` here would let a
		// passed referendum execute calls as an arbitrary account (impersonation) and route Root-
		// level dispatch through this single low-threshold track. The tech lane exists solely to
		// authorize Root governance (e.g. runtime upgrades); members submit via `SubmitOrigin`.
		match id.as_system_ref() {
			Some(frame_system::RawOrigin::Root) => Ok(0),
			_ => Err(()),
		}
	}
}

/// Converts a track ID to a minimum required rank for voting.
/// Currently, all tracks require rank 0 as the minimum rank.
/// In the future, this could be extended to support multiple ranks
/// where different tracks might require different minimum ranks.
/// For example:
/// - Track 1 might require rank 0
/// - Track 2 might require rank 1
/// - Track 3 might require rank 2
///
/// This would allow for a hierarchical voting system where higher-ranked
/// members can vote on more important proposals.
pub struct MinRankOfClassConverter<Delta>(PhantomData<Delta>);
impl<Delta: Get<u16>> Convert<u16, u16> for MinRankOfClassConverter<Delta> {
	fn convert(_a: u16) -> u16 {
		0 // Currently, all tracks require rank 0 as the minimum rank
	}
}

pub struct GlobalMaxMembers<MaxVal: Get<u32>>(PhantomData<MaxVal>);

impl<MaxVal: Get<u32>> MaybeConvert<u16, u32> for GlobalMaxMembers<MaxVal> {
	fn maybe_convert(_a: u16) -> Option<u32> {
		Some(MaxVal::get())
	}
}

pub struct RootOrMemberForTechReferendaOriginImpl<Runtime, I>(PhantomData<(Runtime, I)>);

impl<Runtime, I> EnsureOriginWithArg<Runtime::RuntimeOrigin, crate::OriginCaller>
	for RootOrMemberForTechReferendaOriginImpl<Runtime, I>
where
	Runtime: frame_system::Config<AccountId = AccountId32> + pallet_ranked_collective::Config<I>,
	<Runtime as frame_system::Config>::RuntimeOrigin:
		OriginTrait<PalletsOrigin = crate::OriginCaller>,
	I: 'static,
{
	type Success = Runtime::AccountId;

	fn try_origin(
		o: Runtime::RuntimeOrigin,
		_: &crate::OriginCaller,
	) -> Result<Self::Success, Runtime::RuntimeOrigin> {
		// #91248: the previous `Root` branch re-authenticated with `EnsureSigned`, which can never
		// succeed for `Root`, so it was silently dead. Tech referenda are submitted by collective
		// members (a `Signed` origin); there is no meaningful submitter account for `Root`.
		let original_o_for_error = o.clone();
		let pallets_origin = o.into_caller();

		match pallets_origin {
			crate::OriginCaller::system(frame_system::RawOrigin::Signed(who)) =>
				if pallet_ranked_collective::Members::<Runtime, I>::contains_key(&who) {
					Ok(who)
				} else {
					Err(original_o_for_error)
				},
			_ => Err(original_o_for_error),
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin(_arg: &crate::OriginCaller) -> Result<Runtime::RuntimeOrigin, ()> {
		Ok(frame_system::RawOrigin::<Runtime::AccountId>::Signed(AccountId32::new([0u8; 32]))
			.into())
	}
}

pub type RootOrMemberForTechReferendaOrigin = RootOrMemberForTechReferendaOriginImpl<Runtime, ()>;
