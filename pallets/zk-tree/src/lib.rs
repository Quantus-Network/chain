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
//! - Inserts only append; the root is recomputed once per block in `on_finalize`, folding all of
//!   the block's leaves in a single bottom-up pass
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

/// The deepest tree the wormhole circuits accept (`qp-zk-circuits-common`,
/// `zk_merkle::MAX_DEPTH`). All insert-cost metering below is a constant priced at
/// this depth — see [`INSERT_LEAF_DB_OPS`].
pub const CIRCUIT_MAX_TREE_DEPTH: u8 = 16;

/// Worst-case `ref_time` (picoseconds) of one Poseidon evaluation
/// ([`tree::hash_node`] / [`tree::hash_leaf`]).
///
/// Native release on the reference host measures ~3.3µs/eval
/// (`measure_hash_node_time`). 10µs is ~3× that — enough headroom for wasm /
/// slower boxes without the previous 15× (50µs) pad that capped blocks at
/// ~500 transfers while prepare only took ~1.5s wall clock.
pub const POSEIDON_EVAL_REF_TIME_PS: u64 = 10_000_000;

/// Flat *marginal* `(reads, writes)` charged per [`Pallet::insert_leaf`].
///
/// Root recomputation is batched: inserts only append (`Leaves`, `LeafCount`,
/// `UnprocessedLeaves`), and `on_finalize` folds all of a block's leaves into the
/// tree in one bottom-up pass. In that pass dirty parents are shared, so `n`
/// leaves touch about `n/4 + n/16 + …` internal nodes — bounded by `n/3` — plus a
/// depth-dependent tail that is the same no matter how many leaves the block has.
///
/// The split keeps `price × n` sound for any multi-insert call:
/// - this constant covers the per-leaf marginal cost: 2 insert-phase reads (`LeafCount`,
///   `UnprocessedLeaves`) + the finalize-phase `Leaves` read, and 3 insert-phase writes (`Leaves`,
///   `LeafCount`, `UnprocessedLeaves`) + the amortized `⌈n/3⌉ ≤ n` internal-node writes;
/// - the depth-dependent tail (boundary siblings, the path above the batch, grow bookkeeping) is
///   charged once per block by [`FINALIZE_BASE_DB_OPS`], reserved unconditionally in this pallet's
///   `on_initialize`.
///
/// Charge together with [`INSERT_LEAF_HASH_REF_TIME_PS`].
pub const INSERT_LEAF_DB_OPS: (u64, u64) = (3, 4);

/// Marginal Poseidon evaluations per insert: `hash_leaf` for the `LeafInserted`
/// event at insert time, `hash_leaf` again in the finalize batch, plus the
/// amortized share (`≤ 1` for any `n ≥ 1`) of the batch's internal-node hashes.
/// The depth-dependent remainder is in [`FINALIZE_BASE_POSEIDON_EVALS`].
pub const INSERT_LEAF_POSEIDON_EVALS: u64 = 3;

/// Marginal insert compute (`ref_time` picoseconds).
pub const INSERT_LEAF_HASH_REF_TIME_PS: u64 =
	INSERT_LEAF_POSEIDON_EVALS * POSEIDON_EVAL_REF_TIME_PS;

/// Once-per-block `(reads, writes)` ceiling for the batched root recomputation in
/// `on_finalize`, priced at [`CIRCUIT_MAX_TREE_DEPTH`] and reserved unconditionally
/// by this pallet's `on_initialize` (see [`Pallet::finalize_base_weight`]).
///
/// This is everything the finalize pass costs *beyond* the per-leaf marginal ops
/// already charged through [`INSERT_LEAF_DB_OPS`]. At `d = CIRCUIT_MAX_TREE_DEPTH`:
/// - reads: `UnprocessedLeaves` + `LeafCount` + `Depth` + `grow_tree`'s `Root` + the header-publish
///   `Root` + up to 3 boundary leaves and `3·(d − 1)` boundary sibling nodes (only the parent
///   containing the batch's first leaf can have pre-existing children; everything right of the
///   batch is empty and skipped);
/// - writes: `UnprocessedLeaves` + `Depth` + `grow_tree`'s parked node + `Root` + the
///   `frame_system` header root + the `≤ d` per-level path tail of node writes not covered by the
///   amortized per-leaf share.
///
/// Out-deepening the circuit ceiling would take ~4.3 billion leaves — see
/// [`CIRCUIT_MAX_TREE_DEPTH`]; the trade-off is a modest overcharge while the tree
/// is young.
pub const FINALIZE_BASE_DB_OPS: (u64, u64) = {
	let d = CIRCUIT_MAX_TREE_DEPTH as u64;
	(3 * d + 8, d + 6)
};

/// Once-per-block Poseidon-evaluation ceiling for the finalize batch beyond the
/// per-leaf marginal share: up to 3 boundary leaf hashes plus the `≤ d` per-level
/// path tail, priced at [`CIRCUIT_MAX_TREE_DEPTH`].
pub const FINALIZE_BASE_POSEIDON_EVALS: u64 = CIRCUIT_MAX_TREE_DEPTH as u64 + 3;

/// Conservative PoV bound (bytes) per ZK-tree storage key touched during a path
/// update. Tree entries are 32-byte hashes with small keys; comparable to the
/// benchmarked `ZkTree::Leaves` / `UsedNullifiers` `added` figures (~2524–2543).
/// Callers that scale insert weight with [`INSERT_LEAF_DB_OPS`] should use this
/// for the proof-size term so all pallets share one assumption.
pub const TREE_KEY_POV: u64 = 2600;

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
	pub trait Config:
		frame_system::Config<RuntimeEvent: From<Event<Self>>, AccountId: AsRef<[u8]>>
	{
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
	///
	/// Covers exactly the first `LeafCount - UnprocessedLeaves` leaves: root
	/// recomputation is batched once per block in `on_finalize`, so during block
	/// execution this is the root as of the end of the previous block.
	#[pallet::storage]
	#[pallet::getter(fn root)]
	pub type Root<T: Config> = StorageValue<_, Hash256, ValueQuery>;

	/// Number of trailing leaves appended this block but not yet folded into
	/// `Nodes`/`Root`. Always drained back to 0 by `on_finalize`.
	#[pallet::storage]
	#[pallet::getter(fn unprocessed_leaves)]
	pub type UnprocessedLeaves<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new leaf was inserted into the tree. The root including this leaf is
		/// computed at the end of the block and published in the block header.
		LeafInserted { index: u64, leaf_hash: Hash256 },
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
		fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
			// Reserve the once-per-block ceiling of the batched root recomputation
			// in on_finalize. The per-leaf marginal cost is charged to whoever
			// triggers each insert (see `INSERT_LEAF_DB_OPS`).
			Self::finalize_base_weight()
		}

		fn on_finalize(_n: BlockNumberFor<T>) {
			// Fold all leaves appended during this block into the tree. This must
			// run after every hook that inserts leaves (e.g. mining-rewards'
			// on_finalize); on_finalize executes in pallet declaration order, and
			// this pallet is declared after all inserters in the runtime.
			Self::process_pending_leaves();

			// Set ZK Merkle tree root in frame_system for inclusion in block header
			let root: Hash256 = Root::<T>::get();
			<frame_system::Pallet<T>>::set_zk_tree_root(root.into());
		}
	}

	impl<T: Config> Pallet<T>
	where
		AccountIdOf<T>: AsRef<[u8]>,
	{
		/// Append a new leaf to the tree.
		///
		/// Returns the leaf index. The leaf is *not* folded into the Merkle root
		/// here: root recomputation is batched once per block in `on_finalize`
		/// (see [`Self::process_pending_leaves`]), which is what puts the root
		/// covering this leaf into this block's header.
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
		) -> u64 {
			let leaf = ZkLeaf { to, transfer_count, asset_id, amount };
			let leaf_index = LeafCount::<T>::get();

			let leaf_hash = tree::hash_leaf::<T>(&leaf);
			Leaves::<T>::insert(leaf_index, leaf);
			LeafCount::<T>::put(leaf_index + 1);
			UnprocessedLeaves::<T>::mutate(|pending| *pending = pending.saturating_add(1));

			Self::deposit_event(Event::LeafInserted { index: leaf_index, leaf_hash });

			leaf_index
		}

		/// Fold all leaves appended since the last call into `Nodes` and `Root`.
		///
		/// Called from `on_finalize`; public so tests can settle the tree without
		/// running the whole block lifecycle. No-op when nothing is pending.
		pub fn process_pending_leaves() {
			let pending = UnprocessedLeaves::<T>::take();
			if pending == 0 {
				return;
			}

			let leaf_count = LeafCount::<T>::get();
			let start = leaf_count.saturating_sub(pending);

			// Grow the tree enough to fit every pending leaf. saturating_add can
			// never overflow in practice; MAX_TREE_DEPTH=32 means 4^32 leaves,
			// far beyond any practical blockchain state.
			let old_depth = Depth::<T>::get();
			let mut depth = old_depth;
			while tree::capacity_at_depth(depth) < leaf_count {
				depth = depth.saturating_add(1);
			}
			debug_assert!(
				depth <= MAX_TREE_DEPTH,
				"ZK tree exceeded max depth - this should never happen in practice"
			);
			if depth > old_depth {
				tree::grow_tree::<T>(old_depth);
				Depth::<T>::put(depth);
				Self::deposit_event(Event::TreeGrew { new_depth: depth });
			}

			let new_root = tree::update_range::<T>(start, leaf_count, depth);
			Root::<T>::put(new_root);
		}

		/// Once-per-block weight ceiling of [`Self::process_pending_leaves`] beyond
		/// the marginal cost charged per insert; reserved in `on_initialize`.
		pub fn finalize_base_weight() -> Weight {
			let (reads, writes) = FINALIZE_BASE_DB_OPS;
			<T as frame_system::Config>::DbWeight::get()
				.reads_writes(reads, writes)
				.saturating_add(Weight::from_parts(
					FINALIZE_BASE_POSEIDON_EVALS.saturating_mul(POSEIDON_EVAL_REF_TIME_PS),
					reads.saturating_mul(TREE_KEY_POV),
				))
		}

		/// Get a Merkle proof for a leaf at the given index.
		///
		/// Only leaves already folded into the root (everything up to the end of
		/// the previous block; all leaves once `on_finalize` has run) are provable
		/// — `Nodes` does not yet cover leaves still pending in this block.
		pub fn get_merkle_proof(leaf_index: u64) -> Result<ZkMerkleProof, Error<T>> {
			let processed = LeafCount::<T>::get().saturating_sub(UnprocessedLeaves::<T>::get());
			ensure!(leaf_index < processed, Error::<T>::LeafIndexOutOfBounds);

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
		Self::insert_leaf(to, transfer_count, asset_id, amount)
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
