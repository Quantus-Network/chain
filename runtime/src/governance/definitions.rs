use crate::configs::TreasuryPalletId;
use crate::governance::pallet_custom_origins;
use crate::{
    AccountId, Balance, Balances, BlockNumber, Runtime, RuntimeOrigin, DAYS, HOURS, MICRO_UNIT,
    UNIT,
};
use alloc::vec::Vec;
use codec::{Decode, Encode, EncodeLike, MaxEncodedLen};
use frame_support::pallet_prelude::TypeInfo;
use frame_support::traits::tokens::{ConversionFromAssetBalance, Pay, PaymentStatus};
#[cfg(feature = "runtime-benchmarks")]
use frame_support::traits::Currency;
use frame_support::traits::{
    CallerTrait, Consideration, Currency as CurrencyTrait, EnsureOrigin, EnsureOriginWithArg,
    Footprint, Get, OriginTrait, ReservableCurrency,
};
use lazy_static::lazy_static;
use pallet_ranked_collective::Rank;
use sp_core::crypto::AccountId32;
use sp_runtime::traits::{AccountIdConversion, Convert, MaybeConvert};
use sp_runtime::{DispatchError, Perbill};
use sp_std::marker::PhantomData;
///Preimage pallet fee model

#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
pub struct PreimageDeposit {
    amount: Balance,
}

impl Consideration<AccountId, Footprint> for PreimageDeposit {
    fn new(who: &AccountId, footprint: Footprint) -> Result<Self, DispatchError> {
        // Simple fee model: 0.1 UNIT + 0.0001 UNIT for one byte
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        Balances::reserve(who, amount)?;
        Ok(Self { amount })
    }

    fn update(self, who: &AccountId, new_footprint: Footprint) -> Result<Self, DispatchError> {
        // Calculate new amount
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
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
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        // Check if user has enough coins
        if Balances::free_balance(who) < amount {
            Balances::make_free_balance_be(who, amount.saturating_mul(2));
        }
    }
}

/// Global dynamic configuration for ALL governance tracks
/// This allows tests to override the production timing values at runtime
pub struct GlobalTrackConfig;

static mut GLOBAL_TRACK_OVERRIDE: Option<(BlockNumber, BlockNumber, BlockNumber, BlockNumber)> =
    None;

impl GlobalTrackConfig {
    /// Set global track timing overrides for ALL governance tracks
    /// This affects CommunityTracksInfo, TechCollectiveTracksInfo, and Treasury tracks
    pub fn set_track_override(
        prepare_period: BlockNumber,
        decision_period: BlockNumber,
        confirm_period: BlockNumber,
        min_enactment_period: BlockNumber,
    ) {
        unsafe {
            GLOBAL_TRACK_OVERRIDE = Some((
                prepare_period,
                decision_period,
                confirm_period,
                min_enactment_period,
            ));
        }
    }

    /// Get current global timing override, or None if using production values
    pub fn get_track_override() -> Option<(BlockNumber, BlockNumber, BlockNumber, BlockNumber)> {
        unsafe { GLOBAL_TRACK_OVERRIDE }
    }

    /// Clear global overrides - return to production values
    pub fn clear_track_override() {
        unsafe {
            GLOBAL_TRACK_OVERRIDE = None;
        }
    }

    /// Set fast test timing (2 blocks for all periods) for ALL tracks
    pub fn set_fast_test_timing() {
        Self::set_track_override(2, 2, 2, 2);
    }

    /// Apply timing overrides to a track if global overrides are set
    pub fn apply_timing_override(
        mut track: pallet_referenda::TrackInfo<Balance, BlockNumber>,
    ) -> pallet_referenda::TrackInfo<Balance, BlockNumber> {
        if let Some((prepare, decision, confirm, enactment)) = Self::get_track_override() {
            track.prepare_period = prepare;
            track.decision_period = decision;
            track.confirm_period = confirm;
            track.min_enactment_period = enactment;
        }
        track
    }
}

// Define tracks for referenda
pub struct CommunityTracksInfo;

impl CommunityTracksInfo {
    /// Creates the base track configurations with production values
    fn create_community_tracks() -> [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 6] {
        [
            // Track 0: Signed Track (authenticated proposals)
            (
                0,
                pallet_referenda::TrackInfo {
                    name: "signed",
                    max_deciding: 5, // Allow several concurrent proposals
                    decision_deposit: 500 * UNIT, // Moderate deposit
                    prepare_period: 12 * HOURS, // Shorter preparation time
                    decision_period: 7 * DAYS, // 1 week voting period
                    confirm_period: 12 * HOURS, // 12 hours confirmation
                    min_enactment_period: DAYS, // 1 day until execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(55), // Majority approval required
                        ceil: Perbill::from_percent(70),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(5),
                        ceil: Perbill::from_percent(25),
                    },
                },
            ),
            // Track 1: Signaling Track (non-binding community opinions)
            (
                1,
                pallet_referenda::TrackInfo {
                    name: "signaling",
                    max_deciding: 20, // High throughput for community proposals
                    decision_deposit: 100 * UNIT, // Low deposit requirement
                    prepare_period: 6 * HOURS, // Short preparation time
                    decision_period: 5 * DAYS, // Standard voting period
                    confirm_period: 3 * HOURS, // Minimal confirmation period
                    min_enactment_period: 1, // 1 Block - immediate "execution" (just for record-keeping)
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(50),
                        ceil: Perbill::from_percent(60),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(1),
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
            // Track 2: Treasury tracks
            (
                2,
                pallet_referenda::TrackInfo {
                    name: "treasury_small_spender",
                    max_deciding: 5,
                    decision_deposit: 100 * UNIT,
                    prepare_period: DAYS,
                    decision_period: 3 * DAYS,
                    confirm_period: DAYS,
                    min_enactment_period: 12 * HOURS,
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(25),
                        ceil: Perbill::from_percent(50),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(1),
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
            (
                3,
                pallet_referenda::TrackInfo {
                    name: "treasury_medium_spender",
                    max_deciding: 2,
                    decision_deposit: 250 * UNIT,
                    prepare_period: 6 * HOURS,
                    decision_period: 5 * DAYS,
                    confirm_period: DAYS,
                    min_enactment_period: 12 * HOURS,
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(50),
                        ceil: Perbill::from_percent(75),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(2),
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
            (
                4,
                pallet_referenda::TrackInfo {
                    name: "treasury_big_spender",
                    max_deciding: 2,
                    decision_deposit: 500 * UNIT,
                    prepare_period: DAYS,
                    decision_period: 7 * DAYS,
                    confirm_period: 2 * DAYS,
                    min_enactment_period: 12 * HOURS,
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(65),
                        ceil: Perbill::from_percent(85),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(5),
                        ceil: Perbill::from_percent(15),
                    },
                },
            ),
            (
                5,
                pallet_referenda::TrackInfo {
                    name: "treasury_treasurer",
                    max_deciding: 1,
                    decision_deposit: 1000 * UNIT,
                    prepare_period: 2 * DAYS,
                    decision_period: 14 * DAYS,
                    confirm_period: 4 * DAYS,
                    min_enactment_period: 24 * HOURS,
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(75),
                        ceil: Perbill::from_percent(100),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(10),
                        ceil: Perbill::from_percent(25),
                    },
                },
            ),
        ]
    }
}

impl pallet_referenda::TracksInfo<Balance, BlockNumber> for CommunityTracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        // Static tracks with production values
        lazy_static! {
            static ref STATIC_TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 6] =
                CommunityTracksInfo::create_community_tracks();
        }

        // Test tracks with fast governance timing
        lazy_static! {
            static ref TEST_TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 6] = {
                let base_tracks = CommunityTracksInfo::create_community_tracks();
                let mut test_tracks = base_tracks.clone();

                // Apply global timing overrides to all tracks
                for (_, track) in &mut test_tracks {
                    *track = GlobalTrackConfig::apply_timing_override(track.clone());
                }

                test_tracks
            };
        }

        // Return the appropriate tracks based on whether global overrides are set
        if GlobalTrackConfig::get_track_override().is_some() {
            &*TEST_TRACKS
        } else {
            &*STATIC_TRACKS
        }
    }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check for specific custom origins first (Spender/Treasurer types)
        if let crate::OriginCaller::Origins(custom_origin) = id {
            match custom_origin {
                pallet_custom_origins::Origin::SmallSpender => return Ok(2),
                pallet_custom_origins::Origin::MediumSpender => return Ok(3),
                pallet_custom_origins::Origin::BigSpender => return Ok(4),
                pallet_custom_origins::Origin::Treasurer => return Ok(2),
            }
        }

        // Check for system origins (like None for track 1, Root for track 0)
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::None => return Ok(1),
                frame_system::RawOrigin::Root => return Ok(0),
                _ => {}
            }
        }

        // Fallback for general signed users (catches frame_system::RawOrigin::Signed too, if not Root)
        if let Some(_signer) = id.as_signed() {
            return Ok(0);
        }

        Err(())
    }

    fn info(id: Self::Id) -> Option<&'static pallet_referenda::TrackInfo<Balance, BlockNumber>> {
        Self::tracks()
            .iter()
            .find(|(track_id, _)| *track_id == id)
            .map(|(_, info)| info)
    }

    fn check_integrity() -> Result<(), &'static str> {
        // Basic check that all track IDs are unique
        let mut track_ids = Self::tracks().iter().map(|(id, _)| *id).collect::<Vec<_>>();
        track_ids.sort();
        track_ids.dedup();
        if track_ids.len() != Self::tracks().len() {
            return Err("Duplicate track IDs found");
        }
        Ok(())
    }
}

pub struct TechCollectiveTracksInfo;

impl TechCollectiveTracksInfo {
    fn create_tech_collective_tracks(
    ) -> [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 1] {
        [(
            0,
            pallet_referenda::TrackInfo {
                name: "tech_collective_members",
                max_deciding: 1,
                decision_deposit: 1000 * UNIT,
                prepare_period: 2 * DAYS,
                decision_period: 14 * DAYS,
                confirm_period: 2 * DAYS,
                min_enactment_period: 24 * HOURS,
                min_approval: pallet_referenda::Curve::LinearDecreasing {
                    length: Perbill::from_percent(100),
                    floor: Perbill::from_percent(50),
                    ceil: Perbill::from_percent(100),
                },
                min_support: pallet_referenda::Curve::LinearDecreasing {
                    length: Perbill::from_percent(100),
                    floor: Perbill::from_percent(0),
                    ceil: Perbill::from_percent(0),
                },
            },
        )]
    }
}

impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TechCollectiveTracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        // Static tracks with production values
        lazy_static! {
            static ref STATIC_TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 1] =
                TechCollectiveTracksInfo::create_tech_collective_tracks();
        }

        // Test tracks with fast governance timing
        lazy_static! {
            static ref TEST_TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 1] = {
                let base_tracks = TechCollectiveTracksInfo::create_tech_collective_tracks();
                let mut test_tracks = [(0, base_tracks[0].1.clone())];

                // Apply global timing override to the track
                test_tracks[0].1 = GlobalTrackConfig::apply_timing_override(test_tracks[0].1.clone());

                test_tracks
            };
        }

        // Return the appropriate tracks based on whether global overrides are set
        if GlobalTrackConfig::get_track_override().is_some() {
            &*TEST_TRACKS
        } else {
            &*STATIC_TRACKS
        }
    }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check for system origins first
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::Root => return Ok(0), // Root can use track 0
                frame_system::RawOrigin::None => return Ok(2), // None origin uses track 2
                _ => {}
            }
        }

        // Check for signed origins - simplified version
        if let Some(_signer) = id.as_signed() {
            return Ok(1);
        }
        Err(())
    }

    fn info(id: Self::Id) -> Option<&'static pallet_referenda::TrackInfo<Balance, BlockNumber>> {
        Self::tracks()
            .iter()
            .find(|(track_id, _)| *track_id == id)
            .map(|(_, info)| info)
    }

    fn check_integrity() -> Result<(), &'static str> {
        // Basic check that all track IDs are unique
        let mut track_ids = Self::tracks().iter().map(|(id, _)| *id).collect::<Vec<_>>();
        track_ids.sort();
        track_ids.dedup();
        if track_ids.len() != Self::tracks().len() {
            return Err("Duplicate track IDs found");
        }
        Ok(())
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

pub struct RootOrMemberForCollectiveOriginImpl<Runtime, I>(PhantomData<(Runtime, I)>);

impl<Runtime, I> EnsureOrigin<Runtime::RuntimeOrigin>
    for RootOrMemberForCollectiveOriginImpl<Runtime, I>
where
    Runtime: pallet_ranked_collective::Config<I> + frame_system::Config,
    <Runtime as frame_system::Config>::RuntimeOrigin:
        OriginTrait<PalletsOrigin = crate::OriginCaller>,
    for<'a> &'a AccountId32: EncodeLike<<Runtime as frame_system::Config>::AccountId>,
    I: 'static,
{
    type Success = Rank;

    fn try_origin(o: Runtime::RuntimeOrigin) -> Result<Self::Success, Runtime::RuntimeOrigin> {
        if <frame_system::EnsureRoot<Runtime::AccountId> as EnsureOrigin<
            Runtime::RuntimeOrigin,
        >>::try_origin(o.clone())
        .is_ok()
        {
            return Ok(0);
        }

        let original_o_for_error = o.clone();
        let pallets_origin = o.into_caller();

        match pallets_origin {
            crate::OriginCaller::system(frame_system::RawOrigin::Signed(who)) => {
                if pallet_ranked_collective::Members::<Runtime, I>::contains_key(&who) {
                    Ok(0)
                } else {
                    Err(original_o_for_error)
                }
            }
            _ => Err(original_o_for_error),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<Runtime::RuntimeOrigin, ()> {
        Ok(frame_system::RawOrigin::<Runtime::AccountId>::Root.into())
    }
}

pub type RootOrMemberForCollectiveOrigin = RootOrMemberForCollectiveOriginImpl<Runtime, ()>;

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
        let pallets_origin = o.clone().into_caller();

        if let crate::OriginCaller::system(frame_system::RawOrigin::Root) = pallets_origin {
            if let Ok(signer) = <frame_system::EnsureSigned<Runtime::AccountId> as EnsureOrigin<
                Runtime::RuntimeOrigin,
            >>::try_origin(o.clone())
            {
                return Ok(signer);
            }
        }

        let original_o_for_error = o.clone();
        let pallets_origin = o.into_caller();

        match pallets_origin {
            crate::OriginCaller::system(frame_system::RawOrigin::Signed(who)) => {
                if pallet_ranked_collective::Members::<Runtime, I>::contains_key(&who) {
                    Ok(who)
                } else {
                    Err(original_o_for_error)
                }
            }
            _ => Err(original_o_for_error),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin(_arg: &crate::OriginCaller) -> Result<Runtime::RuntimeOrigin, ()> {
        Ok(
            frame_system::RawOrigin::<Runtime::AccountId>::Signed(AccountId32::new([0u8; 32]))
                .into(),
        )
    }
}

pub type RootOrMemberForTechReferendaOrigin = RootOrMemberForTechReferendaOriginImpl<Runtime, ()>;

// Helper structs for pallet_treasury::Config
pub struct RuntimeNativeBalanceConverter;
impl ConversionFromAssetBalance<Balance, (), Balance> for RuntimeNativeBalanceConverter {
    type Error = sp_runtime::DispatchError;
    fn from_asset_balance(
        balance: Balance,
        _asset_kind: (),
    ) -> Result<Balance, sp_runtime::DispatchError> {
        Ok(balance)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn ensure_successful(_asset_kind: ()) -> () {
        // For an identity conversion with AssetKind = (), there are no
        // external conditions to set up for the conversion itself to succeed.
        // The from_asset_balance call is trivial.
    }
}

pub struct RuntimeNativePaymaster;
impl Pay for RuntimeNativePaymaster {
    type AssetKind = ();
    type Balance = crate::Balance;
    type Beneficiary = crate::AccountId;
    type Id = u32; // Simple payment ID
    type Error = sp_runtime::DispatchError;

    fn pay(
        who: &Self::Beneficiary,
        _asset_kind: Self::AssetKind,
        amount: Self::Balance,
    ) -> Result<Self::Id, sp_runtime::DispatchError> {
        let treasury_account = TreasuryPalletId::get().into_account_truncating();
        <crate::Balances as CurrencyTrait<crate::AccountId>>::transfer(
            &treasury_account,
            who,
            amount,
            frame_support::traits::ExistenceRequirement::AllowDeath,
        )?;
        Ok(0_u32) // Dummy ID
    }

    fn check_payment(id: Self::Id) -> PaymentStatus {
        if id == 0_u32 {
            PaymentStatus::Success
        } else {
            PaymentStatus::Unknown
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn ensure_successful(
        _who: &Self::Beneficiary,
        _asset_kind: Self::AssetKind,
        amount: Self::Balance,
    ) {
        let treasury_account = TreasuryPalletId::get().into_account_truncating();
        let current_balance = crate::Balances::free_balance(&treasury_account);
        if current_balance < amount {
            let missing = amount - current_balance;
            // Assuming deposit_creating is infallible or panics on error internally, returning PositiveImbalance directly.
            let _ = crate::Balances::deposit_creating(&treasury_account, missing);
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn ensure_concluded(_id: Self::Id) {
        // For this synchronous paymaster, payment is concluded once pay returns.
        // No further action needed for ensure_concluded.
    }
}
