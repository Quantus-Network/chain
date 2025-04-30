use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::pallet_prelude::TypeInfo;
use frame_support::traits::{CallerTrait, Consideration, Footprint, RankedMembers, ReservableCurrency};
use sp_runtime::{DispatchError, Perbill};
use crate::{AccountId, Balance, Balances, BlockNumber, Runtime, RuntimeOrigin, DAYS, HOURS, MICRO_UNIT, UNIT};
use alloc::vec::Vec;
use sp_std::marker::PhantomData;
#[cfg(feature = "runtime-benchmarks")]
use frame_support::traits::Currency;
use pallet_referenda::Get;
use sp_runtime::traits::Convert;

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

// Define tracks for referenda
pub struct TracksInfo;
impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        static TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 3] = [
            // Track 0: Root Track (major system changes)
            // - Highest privileges for critical protocol upgrades and parameter changes
            (
                0,
                pallet_referenda::TrackInfo {
                    name: "root",
                    max_deciding: 1,                // Only 1 referendum can be in deciding phase at a time
                    decision_deposit: 10 * UNIT,    // Highest deposit requirement to prevent spam
                    prepare_period: 1 * DAYS,       // 1 day preparation before voting begins
                    decision_period: 14 * DAYS,     // 2 weeks for community to vote
                    confirm_period: 1 * DAYS,       // 1 day confirmation period once passing
                    min_enactment_period: 1 * DAYS, // At least 1 day between approval and execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(50),    // Minimum 50% approval at end
                        ceil: Perbill::from_percent(100),    // Requires 100% approval at start
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(10),    // At least 10% support at end
                        ceil: Perbill::from_percent(50),     // 50% support required at start
                    },
                },
            ),

            // Track 1: Signed Track (authenticated proposals)
            // - For proposals from authenticated users that require privileges
            // - Less stringent than root but still requires identity
            (
                1,
                pallet_referenda::TrackInfo {
                    name: "signed",
                    max_deciding: 5,                // Allow several concurrent proposals
                    decision_deposit: 5 * UNIT,     // Moderate deposit
                    prepare_period: 12 * HOURS,     // Shorter preparation time
                    decision_period: 7 * DAYS,      // 1 week voting period
                    confirm_period: 12 * HOURS,     // 12 hours confirmation
                    min_enactment_period: 12 * HOURS, // 12 hours until execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(55),    // Majority approval required
                        ceil: Perbill::from_percent(70),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(5),
                        ceil: Perbill::from_percent(25),
                    },
                },
            ),

            // Track 2: Signaling Track (non-binding community opinions)
            // - For community sentiment and direction gathering
            (
                2,
                pallet_referenda::TrackInfo {
                    name: "signaling",
                    max_deciding: 20,               // High throughput for community proposals
                    decision_deposit: 1 * UNIT,     // Low deposit requirement
                    prepare_period: 6 * HOURS,      // Short preparation time
                    decision_period: 5 * DAYS,      // Standard voting period
                    confirm_period: 3 * HOURS,      // Minimal confirmation period
                    min_enactment_period: 1,        // Immediate "execution" (just for record-keeping)
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(50),    // Simple majority approval
                        ceil: Perbill::from_percent(60),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(1),     // Very low support threshold
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
        ];
        &TRACKS
    }

    // fn track_for(origin: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
    //     if origin.eq(&frame_support::dispatch::RawOrigin::Root.into()) {
    //         Ok(0)
    //     } else {
    //         Err(())
    //     }
    // }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check for system origins first
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::Root => return Ok(0),
                frame_system::RawOrigin::None => return Ok(2),
                _ => {}
            }
        }

        // Check for other custom origins
        // This syntax depends on exactly how your custom origins are implemented
        if let Some(_) = id.as_signed() {
            return Ok(1);
        }

        // No match found
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

// Definition of tracks for technical referenda
pub struct TechTracksInfo;
impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TechTracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        static TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 3] = [
            // Track 0: Critical Track (only for highest ranked members)
            // - Highest privileges for critical protocol upgrades
            (
                0,
                pallet_referenda::TrackInfo {
                    name: "tech_critical",
                    max_deciding: 1,                // Only 1 referendum can be in deciding phase at a time
                    decision_deposit: 5 * UNIT,     // High deposit to prevent spam
                    prepare_period: 12 * HOURS,     // 12 hours preparation before voting
                    decision_period: 3 * DAYS,      // 3 days for voting
                    confirm_period: 6 * HOURS,      // 6 hours confirmation period after passing
                    min_enactment_period: 1 * DAYS, // At least 1 day between approval and execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(80),    // Minimum 80% approval at end
                        ceil: Perbill::from_percent(100),    // Requires 100% approval at start
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(10),    // At least 10% support at end
                        ceil: Perbill::from_percent(30),     // 30% support required at start
                    },
                },
            ),

            // Track 1: Major Track (for medium ranked members)
            // - For important technical updates that are not critical
            (
                1,
                pallet_referenda::TrackInfo {
                    name: "tech_major",
                    max_deciding: 5,                // Allow several concurrent proposals
                    decision_deposit: 2 * UNIT,     // Moderate deposit
                    prepare_period: 6 * HOURS,      // Shorter preparation time
                    decision_period: 2 * DAYS,      // 2 days voting period
                    confirm_period: 6 * HOURS,      // 6 hours confirmation
                    min_enactment_period: 12 * HOURS, // 12 hours to execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(70),    // Requires majority
                        ceil: Perbill::from_percent(90),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(5),
                        ceil: Perbill::from_percent(20),
                    },
                },
            ),

            // Track 2: Minor Track (for all members)
            // - For minor fixes and technical changes
            (
                2,
                pallet_referenda::TrackInfo {
                    name: "tech_minor",
                    max_deciding: 10,               // High throughput for smaller proposals
                    decision_deposit: 1 * UNIT,     // Low deposit requirement
                    prepare_period: 3 * HOURS,      // Short preparation time
                    decision_period: 1 * DAYS,      // Standard voting period
                    confirm_period: 3 * HOURS,      // Minimal confirmation period
                    min_enactment_period: 6 * HOURS, // Relatively quick execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(60),    // Simple majority
                        ceil: Perbill::from_percent(75),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(2),     // Very low support threshold
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
        ];
        &TRACKS
    }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check different origins and assign to appropriate tracks
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::Root => return Ok(0), // Critical track for Root
                _ => {}
            }
        }

        // Check member rank and assign appropriate track
        // NOTE: This requires additional work to make information flow correctly
        // Below is a sample implementation - in real code we need a way
        // to check the signer's rank

        // Get the signer if available
        if let Some(who) = id.as_signed() {
            // Check rank in RankedCollective
            if let Some(rank) = pallet_ranked_collective::Pallet::<Runtime>::rank_of(&who) {
                // Assign track based on rank
                return Ok(Self::track_for_rank(rank));
            }
        }

        // Default to minor track
        Ok(2)
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
            return Err("Duplicate track IDs found in TechTracksInfo");
        }
        Ok(())
    }
}

// Helper function to determine track based on rank
impl TechTracksInfo {
    pub fn track_for_rank(rank: u16) -> u16 {
        match rank {
            rank if rank >= 3 => 0, // Track 0 (Critical) for members with rank >= 3
            rank if rank >= 1 => 1, // Track 1 (Major) for members with rank >= 1
            _ => 2,                 // Track 2 (Minor) for remaining members
        }
    }
}
type Class = <<Runtime as pallet_referenda::Config>::Tracks as pallet_referenda::TracksInfo<Balance, BlockNumber>>::Id;
pub struct MinRankOfClassConverter<Delta>(PhantomData<Delta>);
impl<Delta: Get<u16>> Convert<u16, u16> for MinRankOfClassConverter<Delta> {
    fn convert(a: u16) -> u16 {
        a.saturating_sub(Delta::get())
    }
}
