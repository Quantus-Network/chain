//! # ZK Trie Pallet
//!
//! A 4-ary Poseidon Merkle tree for storing ZK transfer proofs.
//!
//! ## Overview
//!
//! This pallet provides a separate Merkle tree structure optimized for ZK circuits:
//! - 4-ary tree (4 children per node) for optimal ZK circuit efficiency
//! - Leaves hashed with injective Poseidon (4 bytes/felt) for collision resistance
//! - Internal nodes hashed with non-injective Poseidon (8 bytes/felt) for efficiency
//! - Tree root published in block digest for ZK verification
//!
//! ## Tree Structure
//!
//! ```text
//!                     [Root]                    Level 2
//!                    /  |  \  \
//!              [N0] [N1] [N2] [N3]              Level 1  
//!             /|||\  ...
//!          [L0-L3]  ...                         Level 0 (leaves)
//! ```
//!
//! Leaf data: (to_account, transfer_count, asset_id, amount)
//! Leaf hash: injective_poseidon(leaf_data)
//! Node hash: poseidon(child0 || child1 || child2 || child3)

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;

pub use pallet::*;

pub mod tree;

#[cfg(test)]
mod tests;

/// Maximum depth supported by ZK circuits.
/// A tree of depth 32 can hold 4^32 leaves (more than enough).
pub const MAX_TREE_DEPTH: u8 = 32;

/// Branching factor of the tree.
pub const ARITY: usize = 4;

/// A 32-byte hash output.
pub type Hash256 = [u8; 32];

/// Leaf data for the ZK tree.
#[derive(
	codec::Encode,
	codec::Decode,
	codec::MaxEncodedLen,
	Clone,
	PartialEq,
	Eq,
	scale_info::TypeInfo,
	Debug,
)]
pub struct ZkLeaf<AccountId, AssetId, Balance> {
	/// Recipient account
	pub to: AccountId,
	/// Transfer count (unique per recipient)
	pub transfer_count: u64,
	/// Asset ID (0 for native token)
	pub asset_id: AssetId,
	/// Transfer amount
	pub amount: Balance,
}

/// Merkle proof for a leaf in the 4-ary tree.
#[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, scale_info::TypeInfo, Debug)]
pub struct ZkMerkleProof {
	/// Index of the leaf
	pub leaf_index: u64,
	/// Sibling hashes at each level (3 siblings per level for 4-ary tree)
	pub siblings: alloc::vec::Vec<[Hash256; 3]>,
	/// Position within siblings at each level (0-3)
	pub path_indices: alloc::vec::Vec<u8>,
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config<RuntimeEvent: From<Event<Self>>> {
		/// Asset ID type.
		type AssetId: Parameter + Member + Copy + Default + MaxEncodedLen + Into<u128>;

		/// Balance type.
		type Balance: Parameter + Member + Copy + Default + MaxEncodedLen + Into<u128>;
	}

	/// Account ID type alias for convenience.
	pub type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

	/// Leaf data stored by index.
	#[pallet::storage]
	#[pallet::getter(fn leaf)]
	pub type Leaves<T: Config> =
		StorageMap<_, Identity, u64, ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>, OptionQuery>;

	/// Internal tree nodes: (level, index) -> hash.
	/// Level 0 is unused (leaves are hashed on-demand).
	/// Level 1+ contains internal node hashes.
	#[pallet::storage]
	#[pallet::getter(fn node)]
	pub type Nodes<T: Config> = StorageMap<_, Identity, (u8, u64), Hash256, OptionQuery>;

	/// Number of leaves in the tree.
	#[pallet::storage]
	#[pallet::getter(fn leaf_count)]
	pub type LeafCount<T: Config> = StorageValue<_, u64, ValueQuery>;

	/// Current depth of the tree (0 = empty, 1 = up to 4 leaves, etc.).
	#[pallet::storage]
	#[pallet::getter(fn depth)]
	pub type Depth<T: Config> = StorageValue<_, u8, ValueQuery>;

	/// Current root hash of the tree.
	#[pallet::storage]
	#[pallet::getter(fn root)]
	pub type Root<T: Config> = StorageValue<_, Hash256, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new leaf was inserted into the tree.
		LeafInserted { index: u64, leaf_hash: Hash256, new_root: Hash256 },
		/// Tree depth increased.
		TreeGrew { new_depth: u8 },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Tree has reached maximum depth.
		MaxDepthReached,
		/// Leaf index out of bounds.
		LeafIndexOutOfBounds,
		/// Leaf not found.
		LeafNotFound,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_finalize(_n: BlockNumberFor<T>) {
			// Add ZkRoot to digest
			let root = Root::<T>::get();
			let item = sp_runtime::generic::DigestItem::Other(root.to_vec());
			<frame_system::Pallet<T>>::deposit_log(item);
		}
	}

	impl<T: Config> Pallet<T>
	where
		AccountIdOf<T>: AsRef<[u8]>,
	{
		/// Insert a new leaf into the tree.
		///
		/// Returns the leaf index and new root hash.
		pub fn insert_leaf(
			to: AccountIdOf<T>,
			transfer_count: u64,
			asset_id: T::AssetId,
			amount: T::Balance,
		) -> Result<(u64, Hash256), Error<T>> {
			let leaf = ZkLeaf { to, transfer_count, asset_id, amount };
			let leaf_index = LeafCount::<T>::get();

			// Check if we need to grow the tree
			let current_depth = Depth::<T>::get();
			let capacity = tree::capacity_at_depth(current_depth);

			if leaf_index >= capacity {
				// Need to grow the tree
				let new_depth = current_depth.checked_add(1).ok_or(Error::<T>::MaxDepthReached)?;
				ensure!(new_depth <= MAX_TREE_DEPTH, Error::<T>::MaxDepthReached);

				tree::grow_tree::<T>(current_depth, new_depth)?;
				Depth::<T>::put(new_depth);

				Self::deposit_event(Event::TreeGrew { new_depth });
			}

			// Store the leaf
			Leaves::<T>::insert(leaf_index, leaf.clone());
			LeafCount::<T>::put(leaf_index + 1);

			// Compute leaf hash and update tree
			let leaf_hash = tree::hash_leaf::<T>(&leaf);
			let new_root = tree::update_path::<T>(leaf_index, leaf_hash)?;

			Root::<T>::put(new_root);

			Self::deposit_event(Event::LeafInserted { index: leaf_index, leaf_hash, new_root });

			Ok((leaf_index, new_root))
		}

		/// Get a Merkle proof for a leaf at the given index.
		pub fn get_merkle_proof(leaf_index: u64) -> Result<ZkMerkleProof, Error<T>> {
			let leaf_count = LeafCount::<T>::get();
			ensure!(leaf_index < leaf_count, Error::<T>::LeafIndexOutOfBounds);

			let depth = Depth::<T>::get();
			tree::generate_proof::<T>(leaf_index, depth)
		}

		/// Verify a Merkle proof against the current root.
		pub fn verify_proof(
			leaf: &ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>,
			proof: &ZkMerkleProof,
		) -> bool {
			let root = Root::<T>::get();
			tree::verify_proof::<T>(leaf, proof, root)
		}
	}
}

// ============================================================================
// Trait for external pallets
// ============================================================================

/// Trait for inserting leaves into the ZK trie.
/// Used by pallet-wormhole to record transfer proofs.
pub trait ZkTrieRecorder<AccountId, AssetId, Balance> {
	/// Insert a transfer into the ZK trie.
	/// Returns the leaf index and new root hash.
	fn record_transfer(
		to: AccountId,
		transfer_count: u64,
		asset_id: AssetId,
		amount: Balance,
	) -> Result<Option<(u64, Hash256)>, ()>;
}

/// No-op implementation for when ZK trie is not configured.
impl<AccountId, AssetId, Balance> ZkTrieRecorder<AccountId, AssetId, Balance> for () {
	fn record_transfer(
		_to: AccountId,
		_transfer_count: u64,
		_asset_id: AssetId,
		_amount: Balance,
	) -> Result<Option<(u64, Hash256)>, ()> {
		Ok(None)
	}
}

impl<T: Config> ZkTrieRecorder<T::AccountId, T::AssetId, T::Balance> for Pallet<T>
where
	T::AccountId: AsRef<[u8]>,
{
	fn record_transfer(
		to: T::AccountId,
		transfer_count: u64,
		asset_id: T::AssetId,
		amount: T::Balance,
	) -> Result<Option<(u64, Hash256)>, ()> {
		Self::insert_leaf(to, transfer_count, asset_id, amount)
			.map(Some)
			.map_err(|_| ())
	}
}

// ============================================================================
// Runtime API
// ============================================================================

/// RPC-friendly Merkle proof structure (no generics).
/// Uses raw bytes for the leaf data to avoid generic type issues in RPC.
#[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, scale_info::TypeInfo, Debug)]
#[cfg_attr(feature = "std", derive(serde::Serialize, serde::Deserialize))]
pub struct ZkMerkleProofRpc {
	/// Index of the leaf
	pub leaf_index: u64,
	/// The leaf data (encoded ZkLeaf)
	pub leaf_data: Vec<u8>,
	/// Leaf hash
	pub leaf_hash: Hash256,
	/// Sibling hashes at each level (3 siblings per level for 4-ary tree)
	pub siblings: Vec<[Hash256; 3]>,
	/// Position within siblings at each level (0-3)
	pub path_indices: Vec<u8>,
	/// Current tree root
	pub root: Hash256,
	/// Current tree depth
	pub depth: u8,
}

sp_api::decl_runtime_apis! {
	/// Runtime API for the ZK Trie pallet.
	///
	/// Provides methods to query the ZK Merkle tree state and generate proofs.
	pub trait ZkTrieApi {
		/// Get the current root hash of the ZK tree.
		fn get_root() -> Hash256;

		/// Get the current number of leaves in the tree.
		fn get_leaf_count() -> u64;

		/// Get the current depth of the tree.
		fn get_depth() -> u8;

		/// Get a Merkle proof for a leaf at the given index.
		///
		/// Returns `None` if the leaf index is out of bounds.
		fn get_merkle_proof(leaf_index: u64) -> Option<ZkMerkleProofRpc>;
	}
}
