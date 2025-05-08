use crate::Runtime;
use frame_support::{
    parameter_types,
};
use pallet_ranked_collective::EnsureRanked;

parameter_types! {
    pub const MaxFellowshipRank: u16 = 4;
    pub const FellowshipEvidenceSize: u32 = 32 * 1024; // 32 KB
}

// Define origins for fellowship ranks using the raw EnsureRanked type
// to avoid the EitherOfDiverse return type issues
pub type ApproveOrigin = EnsureRanked<Runtime, (), 1>;
pub type PromoteOrigin = EnsureRanked<Runtime, (), 3>;
pub type FastPromoteOrigin = EnsureRanked<Runtime, (), 4>;

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