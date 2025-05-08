use sp_std::marker::PhantomData;
use crate::Runtime;
use frame_support::{
    parameter_types,
};
use frame_support::traits::{EnsureOrigin, RankedMembers};
use pallet_ranked_collective::Pallet as RankedCollective;

parameter_types! {
    pub const MaxFellowshipRank: u16 = 4;
    pub const FellowshipEvidenceSize: u32 = 32 * 1024; // 32 KB
}

pub struct ApproveOriginImpl<T, I>(PhantomData<(T, I)>);
impl<T: pallet_ranked_collective::Config<I>, I: 'static> EnsureOrigin<T::RuntimeOrigin>
for ApproveOriginImpl<T, I>
{
    type Success = u16;

    fn try_origin(o: T::RuntimeOrigin) -> Result<Self::Success, T::RuntimeOrigin> {
        // First check if it's root
        let root_check = frame_system::EnsureRoot::<T::AccountId>::try_origin(o.clone());
        if root_check.is_ok() {
            return Ok(1u16); // Return rank 1 for root
        }

        // Otherwise check if it's a signed origin with rank 1+
        o.clone().into().and_then(|o| match o.clone() {
            frame_system::RawOrigin::Signed(who) => {
                if let Some(rank) = RankedCollective::<T, I>::rank_of(&who) {
                    if rank >= 1 {
                        return Ok(rank);
                    }
                }
                Err(o.into())
            }
            _ => Err(o.into()),
        }).map_err(|_| o)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<T::RuntimeOrigin, ()> {
        Ok(frame_system::RawOrigin::Root::<T::AccountId>::into())
    }
}

// Custom origin for promotion that requires higher rank
// Returns rank 2 for root or the actual rank for members with rank > target
pub struct PromoteOriginImpl<T, I>(PhantomData<(T, I)>);
impl<T: pallet_ranked_collective::Config<I>, I: 'static> EnsureOrigin<T::RuntimeOrigin>
for PromoteOriginImpl<T, I>
{
    type Success = u16;

    fn try_origin(o: T::RuntimeOrigin) -> Result<Self::Success, T::RuntimeOrigin> {
        // First check if it's root
        let root_check = frame_system::EnsureRoot::<T::AccountId>::try_origin(o.clone());
        if root_check.is_ok() {
            return Ok(2u16); // Return rank 2 for root
        }

        // Otherwise check if it's a signed origin with higher rank than target
        o.clone().into().and_then(|o| match o.clone() {
            frame_system::RawOrigin::Signed(who) => {
                if let Some(rank) = RankedCollective::<T, I>::rank_of(&who) {
                    if rank > 0 { // Higher than target (which is 0)
                        return Ok(rank);
                    }
                }
                Err(o.into())
            }
            _ => Err(o.into()),
        }).map_err(|_| o)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<T::RuntimeOrigin, ()> {
        Ok(frame_system::RawOrigin::Root::<T::AccountId>::into())
    }
}

// Custom origin for fast promotion
// Returns rank 3 for root or the actual rank for members with rank 3+
pub struct FastPromoteOriginImpl<T, I>(PhantomData<(T, I)>);
impl<T: pallet_ranked_collective::Config<I>, I: 'static> EnsureOrigin<T::RuntimeOrigin>
for FastPromoteOriginImpl<T, I>
{
    type Success = u16;

    fn try_origin(o: T::RuntimeOrigin) -> Result<Self::Success, T::RuntimeOrigin> {
        // First check if it's root
        let root_check = frame_system::EnsureRoot::<T::AccountId>::try_origin(o.clone());
        if root_check.is_ok() {
            return Ok(3u16); // Return rank 3 for root
        }

        // Otherwise check if it's a signed origin with rank 3+
        o.clone().into().and_then(|o| match o.clone() {
            frame_system::RawOrigin::Signed(who) => {
                if let Some(rank) = RankedCollective::<T, I>::rank_of(&who) {
                    if rank >= 3 {
                        return Ok(rank);
                    }
                }
                Err(o.into())
            }
            _ => Err(o.into()),
        }).map_err(|_| o)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<T::RuntimeOrigin, ()> {
        Ok(frame_system::RawOrigin::Root::<T::AccountId>::into())
    }
}

// Define origin types to be used in the pallet configuration
pub type ApproveOrigin = ApproveOriginImpl<Runtime, ()>;
pub type PromoteOrigin = PromoteOriginImpl<Runtime, ()>;
pub type FastPromoteOrigin = FastPromoteOriginImpl<Runtime, ()>;





// Utility functions for tracks
pub fn track_for_rank(rank: u16) -> u16 {
    match rank {
        rank if rank >= 3 => 0, // Track 0 (Critical)
        rank if rank >= 1 => 1, // Track 1 (Major)
        _ => 2,                 // Track 2 (Minor)
    }
}

pub fn min_rank_for_track(track: u16) -> u16 {
    match track {
        0 => 3,
        1 => 1,
        _ => 0,
    }
}