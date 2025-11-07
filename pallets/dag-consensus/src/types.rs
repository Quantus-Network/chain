//! Type definitions for DAG consensus pallet

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{BoundedBTreeMap, BoundedVec};
use scale_info::TypeInfo;

/// DAG block data containing GHOSTDAG information
/// Uses fixed bounds for storage efficiency
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct DagBlockData<Hash> {
	/// Blue score - number of blue blocks in the past
	pub blue_score: u64,
	/// Blue work - cumulative work of blue blocks
	pub blue_work: u128,
	/// Selected parent - parent with highest blue work
	pub selected_parent: Hash,
	/// Blue blocks in mergeset (max 100)
	pub mergeset_blues: BoundedVec<Hash, frame_support::traits::ConstU32<100>>,
	/// Red blocks in mergeset (max 100)
	pub mergeset_reds: BoundedVec<Hash, frame_support::traits::ConstU32<100>>,
	/// Anticone sizes for each blue block (max 200 entries)
	pub blues_anticone_sizes: BoundedBTreeMap<Hash, u32, frame_support::traits::ConstU32<200>>,
}

impl<Hash: Default + Ord> Default for DagBlockData<Hash> {
	fn default() -> Self {
		Self {
			blue_score: 0,
			blue_work: 0,
			selected_parent: Hash::default(),
			mergeset_blues: BoundedVec::new(),
			mergeset_reds: BoundedVec::new(),
			blues_anticone_sizes: BoundedBTreeMap::new(),
		}
	}
}

/// Virtual DAG state representing the current consensus view
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct VirtualDagState<Hash> {
	/// Current DAG tips (parents of virtual block) - max 50 parents
	pub parents: BoundedVec<Hash, frame_support::traits::ConstU32<50>>,
	/// Selected parent from the virtual perspective
	pub selected_parent: Hash,
	/// Blue work of the selected parent
	pub blue_work: u128,
}

impl<Hash: Default> Default for VirtualDagState<Hash> {
	fn default() -> Self {
		Self { parents: BoundedVec::new(), selected_parent: Hash::default(), blue_work: 0 }
	}
}

/// Block reward information for GHOSTDAG
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct DagBlockReward<AccountId, Balance> {
	/// Miner account
	pub miner: AccountId,
	/// Base block reward
	pub base_reward: Balance,
	/// Transaction fees
	pub fees: Balance,
	/// Whether this block is blue (affects reward distribution)
	pub is_blue: bool,
}

/// Reachability interval for efficient DAG queries
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct ReachabilityInterval {
	/// Start of the interval
	pub start: u64,
	/// End of the interval
	pub end: u64,
}

impl Default for ReachabilityInterval {
	fn default() -> Self {
		Self { start: 0, end: 0 }
	}
}

/// DAG validation error types
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub enum DagValidationError {
	/// K-cluster violation
	KClusterViolation,
	/// Invalid parent reference
	InvalidParent,
	/// Circular dependency detected
	CircularDependency,
	/// Too many parents
	TooManyParents,
	/// Invalid mergeset
	InvalidMergeset,
}

/// Statistics for DAG analysis
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug, Default)]
pub struct DagStatistics {
	/// Total number of blocks in DAG
	pub total_blocks: u64,
	/// Total blue blocks
	pub total_blue_blocks: u64,
	/// Total red blocks
	pub total_red_blocks: u64,
	/// Average mergeset size
	pub avg_mergeset_size: u32,
	/// Current DAG depth (longest chain)
	pub dag_depth: u64,
	/// Number of current tips
	pub tips_count: u32,
}

/// Block position in DAG (for ordering and navigation)
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct DagPosition<Hash> {
	/// Block hash
	pub hash: Hash,
	/// Blue score (position in blue ordering)
	pub blue_score: u64,
	/// Blue work (cumulative work)
	pub blue_work: u128,
	/// DAG level (distance from genesis)
	pub level: u32,
}

/// Mergeset information for a block
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct MergesetInfo<Hash> {
	/// Selected parent
	pub selected_parent: Hash,
	/// All mergeset blocks (blues + reds) - max 200
	pub mergeset: BoundedVec<Hash, frame_support::traits::ConstU32<200>>,
	/// Blue blocks only - max 200
	pub blues: BoundedVec<Hash, frame_support::traits::ConstU32<200>>,
	/// Red blocks only - max 200
	pub reds: BoundedVec<Hash, frame_support::traits::ConstU32<200>>,
	/// Size of the mergeset
	pub size: u32,
}

impl<Hash: Default> Default for MergesetInfo<Hash> {
	fn default() -> Self {
		Self {
			selected_parent: Hash::default(),
			mergeset: BoundedVec::new(),
			blues: BoundedVec::new(),
			reds: BoundedVec::new(),
			size: 0,
		}
	}
}
