//! # ZK Tree Pallet
//!
//! A 4-ary Poseidon Merkle tree for storing ZK transfer proofs.
//!
//! ## Overview
//!
//! This pallet provides a separate Merkle tree structure optimized for ZK circuits:
//! - 4-ary tree (4 children per node) for optimal ZK circuit efficiency
//! - Leaves hashed as 8 field elements (injective: values are ≤32 bits)
//! - Internal nodes hashed as 16 field elements (8 bytes/felt compact encoding)
//! - Tree root published in block header for ZK verification
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
//! Leaf hash: poseidon(8 felts from leaf encoding)
//! Node hash: poseidon(sorted children concatenated → 16 felts)

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use frame_support::weights::{RuntimeDbWeight, Weight};

pub use pallet::*;

pub mod tree;

#[cfg(test)]
mod tests;

/// Maximum depth the on-chain tree may grow to (weight-metering / growth cap).
/// A tree of depth 32 can hold 4^32 leaves.
///
/// NOTE (known, accepted limitation): this is intentionally *larger* than the depth the
/// wormhole circuits accept. The circuits fix `MAX_DEPTH = 16` (`qp-zk-circuits-common`,
/// `zk_merkle.rs`) because every leaf proof pays the proving cost of a full
/// `MAX_DEPTH`-level Merkle path regardless of the tree's current depth — keeping it at
/// 16 keeps proving fast for everyone. If the tree ever grows past depth 16
/// (4^16 ≈ 4.3 billion leaves), Merkle proofs gain a 17th sibling level and the prover
/// and verifier reject them, so wormhole proof generation halts until a circuit update
/// raises `MAX_DEPTH` and a runtime upgrade embeds the regenerated verifiers.
///
/// This is a deliberate "fix it when we get close" trade-off, not an oversight:
/// - Timeline: at one leaf per block (the mining-reward floor, 12s blocks) depth 16 lasts ~1,600
///   years; at a sustained 10 transfers/sec chain-wide it lasts ~13 years; even permanently
///   saturated blocks (~50 tps) give ~2.5 years. Each +1 of circuit depth quadruples capacity.
/// - Observability: `LeafCount` is public storage, so exhaustion is visible years in advance; alert
///   well before 4^16 leaves.
/// - The update itself: bump `MAX_DEPTH` in `qp-zk-circuits-common`, release the circuit crates,
///   let `pallets/wormhole/build.rs` regenerate the embedded verifier binaries, regenerate proof
///   fixtures, re-benchmark, and ship a runtime upgrade — days of engineering inside a normal
///   release cycle. Old proofs are invalidated by the circuit change; nullifier state is
///   unaffected, so nothing can double-spend across the upgrade.
pub const MAX_TREE_DEPTH: u8 = 32;

/// Worst-case `(reads, writes)` storage-operation counts for one [`Pallet::insert_leaf`]
/// call when the tree is currently at `depth`.
///
/// The insert is depth-dependent: `update_path` reads the 3 sibling hashes at every
/// level and writes one internal node per level below the root. Costing is done at
/// `depth + 1` (capped at [`MAX_TREE_DEPTH`]) so an insert that triggers tree growth is
/// covered. At effective depth `d`:
/// - reads: `LeafCount` + `Depth` (twice) + `grow_tree`'s `Root` + `3·d` siblings
/// - writes: `Leaves` + `LeafCount` + `Root` + `d − 1` internal nodes, plus `grow_tree`'s
///   `Nodes`/`Root`/`Depth` writes
///
/// Weight/fee metering for anything that inserts leaves must use this (via
/// [`Pallet::insert_leaf_db_ops`]) rather than a flat constant, otherwise the declared
/// weight silently falls behind the real database work as the tree deepens.
pub fn insert_leaf_db_ops_at_depth(depth: u8) -> (u64, u64) {
	let d = depth.saturating_add(1).min(MAX_TREE_DEPTH) as u64;
	(3 * d + 4, d + 5)
}

/// Worst-case `ref_time` (picoseconds) of one Poseidon evaluation
/// ([`tree::hash_node`] / [`tree::hash_leaf`]). Padded for wasm / slower hardware.
pub const POSEIDON_EVAL_REF_TIME_PS: u64 = 50_000_000;

/// Poseidon evaluations for one [`Pallet::insert_leaf`] at `depth`
/// (`hash_leaf` + per-level `hash_node` + grow). Costed at `depth + 1` like
/// [`insert_leaf_db_ops_at_depth`].
pub fn insert_leaf_poseidon_evals_at_depth(depth: u8) -> u64 {
	let d = depth.saturating_add(1).min(MAX_TREE_DEPTH) as u64;
	d + 2
}

/// Depth-dependent insert compute (`ref_time`); scales with tree depth the same
/// way as [`insert_leaf_db_ops_at_depth`].
pub fn insert_leaf_hash_ref_time_at_depth(depth: u8) -> u64 {
	insert_leaf_poseidon_evals_at_depth(depth).saturating_mul(POSEIDON_EVAL_REF_TIME_PS)
}

/// Conservative PoV bound (bytes) per ZK-tree storage key touched during a path
/// update. Tree entries are 32-byte hashes with small keys; comparable to the
/// benchmarked `ZkTree::Leaves` / `UsedNullifiers` `added` figures (~2524–2543).
/// Callers that scale insert weight with [`insert_leaf_db_ops_at_depth`] should
/// use this for the proof-size term so all pallets share one assumption.
pub const TREE_KEY_POV: u64 = 2600;

/// Complete worst-case weight of one [`Pallet::insert_leaf`] at `depth`: the storage
/// I/O ([`insert_leaf_db_ops_at_depth`]), the per-level Poseidon path hashing
/// ([`insert_leaf_hash_ref_time_at_depth`]) and the per-key PoV ([`TREE_KEY_POV`]).
///
/// Everything that prices a leaf insert must compose it through here so one change to
/// the cost model reaches every caller — pricing an insert by hand risks charging the
/// DB ops while silently dropping the hashing or the proof size.
pub fn insert_leaf_weight_at_depth(db: RuntimeDbWeight, depth: u8) -> Weight {
	let (reads, writes) = insert_leaf_db_ops_at_depth(depth);
	Weight::from_parts(
		insert_leaf_hash_ref_time_at_depth(depth),
		reads.saturating_mul(TREE_KEY_POV),
	)
	.saturating_add(db.reads(reads))
	.saturating_add(db.writes(writes))
}

/// Branching factor of the tree.
pub const ARITY: usize = 4;

/// A 32-byte hash output.
pub type Hash256 = [u8; 32];

/// Leaf data for the ZK tree.
///
/// # Why `from` is not included
///
/// The ZK circuit needs to verify two things about a transfer:
/// 1. The transfer amount (to compute balances)
/// 2. The transfer is unique (to prevent double-spending)
///
/// Uniqueness is guaranteed by `(to, transfer_count)` - each recipient has a
/// monotonically increasing counter, so every transfer to that recipient gets
/// a unique index. The `from` address is irrelevant for proving ownership of
/// received funds; what matters is that the transfer happened exactly once.
///
/// Omitting `from` reduces the leaf size and simplifies the ZK circuit without
/// sacrificing security properties.
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
	/// Transfer count for this recipient (ensures uniqueness via `(to, transfer_count)`)
	pub transfer_count: u64,
	/// Asset ID (0 for native token)
	pub asset_id: AssetId,
	/// Transfer amount
	pub amount: Balance,
}

/// Merkle proof for a leaf in the 4-ary tree.
///
/// # Index-free verification
///
/// Because internal nodes sort their children before hashing, proofs don't need
/// path indices. The verifier simply combines the current hash with the 3 siblings,
/// sorts all 4, and hashes to get the parent. This simplifies ZK circuit verification.
#[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, scale_info::TypeInfo, Debug)]
pub struct ZkMerkleProof {
	/// Index of the leaf (for reference, not needed for verification)
	pub leaf_index: u64,
	/// Sibling hashes at each level (3 siblings per level for 4-ary tree)
	pub siblings: alloc::vec::Vec<[Hash256; 3]>,
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
		/// Leaf index out of bounds.
		LeafIndexOutOfBounds,
		/// Leaf not found.
		LeafNotFound,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_finalize(_n: BlockNumberFor<T>) {
			// Set ZK Merkle tree root in frame_system for inclusion in block header
			let root: Hash256 = Root::<T>::get();
			<frame_system::Pallet<T>>::set_zk_tree_root(root.into());
		}
	}

	impl<T: Config> Pallet<T> {
		/// Worst-case `(reads, writes)` for one `insert_leaf` at the tree's *current*
		/// depth. See [`insert_leaf_db_ops_at_depth`].
		pub fn insert_leaf_db_ops() -> (u64, u64) {
			crate::insert_leaf_db_ops_at_depth(Depth::<T>::get())
		}

		/// Worst-case Poseidon-hashing `ref_time` for one `insert_leaf` at the tree's
		/// *current* depth. See [`insert_leaf_hash_ref_time_at_depth`]. Anything that
		/// prices a leaf insert must charge this *in addition to*
		/// [`Self::insert_leaf_db_ops`]: the DB ops cover storage I/O only, while the
		/// path update also computes one Poseidon hash per tree level.
		pub fn insert_leaf_hash_ref_time() -> u64 {
			crate::insert_leaf_hash_ref_time_at_depth(Depth::<T>::get())
		}

		/// [`insert_leaf_weight_at_depth`] at the tree's *current* depth, reading
		/// `Depth` once. Prefer this over composing the parts by hand: each part
		/// reads `Depth` again, and weight functions run on every dispatch-info
		/// evaluation.
		pub fn insert_leaf_weight(db: crate::RuntimeDbWeight) -> crate::Weight {
			crate::insert_leaf_weight_at_depth(db, Depth::<T>::get())
		}
	}

	impl<T: Config> Pallet<T>
	where
		AccountIdOf<T>: AsRef<[u8]>,
	{
		/// Insert a new leaf into the tree.
		///
		/// Returns the leaf index and new root hash.
		///
		/// # Infallibility
		///
		/// This function is infallible because the only theoretical failure mode
		/// (exceeding MAX_TREE_DEPTH of 32) would require 4^32 leaves, which is
		/// astronomically larger than any practical blockchain state.
		pub fn insert_leaf(
			to: AccountIdOf<T>,
			transfer_count: u64,
			asset_id: T::AssetId,
			amount: T::Balance,
		) -> (u64, Hash256) {
			let leaf = ZkLeaf { to, transfer_count, asset_id, amount };
			let leaf_index = LeafCount::<T>::get();

			// Check if we need to grow the tree
			let current_depth = Depth::<T>::get();
			let capacity = tree::capacity_at_depth(current_depth);

			if leaf_index >= capacity {
				// Need to grow the tree
				// saturating_add ensures we never overflow; MAX_TREE_DEPTH=32 means 4^32 leaves
				// which is ~18 quintillion - far beyond any practical blockchain state
				let new_depth = current_depth.saturating_add(1);
				debug_assert!(
					new_depth <= MAX_TREE_DEPTH,
					"ZK tree exceeded max depth - this should never happen in practice"
				);

				tree::grow_tree::<T>(current_depth, new_depth);
				Depth::<T>::put(new_depth);

				Self::deposit_event(Event::TreeGrew { new_depth });
			}

			// Store the leaf
			Leaves::<T>::insert(leaf_index, leaf.clone());
			LeafCount::<T>::put(leaf_index + 1);

			// Compute leaf hash and update tree
			let leaf_hash = tree::hash_leaf::<T>(&leaf);
			let new_root = tree::update_path::<T>(leaf_index, leaf_hash);

			Root::<T>::put(new_root);

			Self::deposit_event(Event::LeafInserted { index: leaf_index, leaf_hash, new_root });

			(leaf_index, new_root)
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

/// Trait for inserting leaves into the ZK tree.
/// Used by pallet-wormhole to record transfer proofs.
pub trait ZkTreeRecorder<AccountId, AssetId, Balance> {
	/// Insert a transfer into the ZK tree.
	///
	/// Returns the leaf index, which can be used to fetch Merkle proofs via RPC.
	/// This operation is infallible. Implementations must always succeed.
	fn record_transfer(
		to: AccountId,
		transfer_count: u64,
		asset_id: AssetId,
		amount: Balance,
	) -> u64;
}

/// No-op implementation for when ZK tree is not configured.
impl<AccountId, AssetId, Balance> ZkTreeRecorder<AccountId, AssetId, Balance> for () {
	fn record_transfer(
		_to: AccountId,
		_transfer_count: u64,
		_asset_id: AssetId,
		_amount: Balance,
	) -> u64 {
		0 // No-op returns 0
	}
}

impl<T: Config> ZkTreeRecorder<T::AccountId, T::AssetId, T::Balance> for Pallet<T>
where
	T::AccountId: AsRef<[u8]>,
{
	fn record_transfer(
		to: T::AccountId,
		transfer_count: u64,
		asset_id: T::AssetId,
		amount: T::Balance,
	) -> u64 {
		let (leaf_index, _root) = Self::insert_leaf(to, transfer_count, asset_id, amount);
		leaf_index
	}
}

// ============================================================================
// Runtime API
// ============================================================================

/// RPC-friendly Merkle proof structure (no generics).
///
/// Uses raw bytes for the leaf data to avoid generic type issues in RPC.
/// No path indices needed - children are sorted before hashing, so verification
/// just requires combining current hash with siblings, sorting, and hashing.
#[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, scale_info::TypeInfo, Debug)]
#[cfg_attr(feature = "std", derive(serde::Serialize, serde::Deserialize))]
pub struct ZkMerkleProofRpc {
	/// Index of the leaf (for reference, not needed for verification)
	pub leaf_index: u64,
	/// The leaf data (encoded ZkLeaf)
	pub leaf_data: Vec<u8>,
	/// Leaf hash
	pub leaf_hash: Hash256,
	/// Sibling hashes at each level (3 siblings per level for 4-ary tree)
	pub siblings: Vec<[Hash256; 3]>,
	/// Current tree root
	pub root: Hash256,
	/// Current tree depth
	pub depth: u8,
}

sp_api::decl_runtime_apis! {
	/// Runtime API for the ZK Tree pallet.
	///
	/// Provides methods to query the ZK Merkle tree state and generate proofs.
	pub trait ZkTreeApi {
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
