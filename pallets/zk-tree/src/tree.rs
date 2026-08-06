//! 4-ary Poseidon Merkle tree implementation.
//!
//! This module provides the core tree operations:
//! - Leaf hashing (8 felts, injective for ≤32-bit values)
//! - Node hashing (16 felts, 8 bytes/felt compact encoding)
//! - Path updates on insert
//! - Proof generation and verification
//! - Tree growth when capacity is exceeded

use crate::{
	pallet::{AccountIdOf, Config, Error},
	Hash256, ZkLeaf, ZkMerkleProof, ARITY,
};
use alloc::vec::Vec;
use qp_poseidon_core::Goldilocks;

/// Compute the capacity (max leaves) at a given depth.
/// Depth 0 = 0 leaves (empty tree)
/// Depth 1 = 4 leaves
/// Depth 2 = 16 leaves
/// etc.
pub fn capacity_at_depth(depth: u8) -> u64 {
	if depth == 0 {
		0
	} else {
		(ARITY as u64).saturating_pow(depth as u32)
	}
}

/// Quantization factor for amounts in ZK leaves.
/// Amounts are divided by this factor before storing in the leaf hash.
/// This matches the circuit's expectation of 2 decimal places of precision.
/// 10^10 means 1 DEV (10^12 planck) becomes 100 in the leaf.
pub const AMOUNT_SCALE_DOWN_FACTOR: u128 = 10_000_000_000;

/// The Goldilocks prime `p = 2^64 - 2^32 + 1`.
pub const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

/// Reduce each 8-byte little-endian limb of a 32-byte account mod the Goldilocks prime.
///
/// This is the same reduction the lossy leaf encoding applies inside [`hash_leaf`], so
/// the returned bytes are the canonical representative of the input's leaf-encoding
/// class: two accounts produce the same `to_account` felts iff they canonicalize
/// identically. Callers that key state on the leaf recipient (e.g. the wormhole's
/// per-recipient transfer count) must key on this canonical form so that two distinct
/// deposits can never commit to identical leaves (see the encoding-safety invariant on
/// [`hash_leaf`]).
pub fn canonicalize_account_bytes(bytes: [u8; 32]) -> [u8; 32] {
	let mut out = bytes;
	for limb in out.chunks_exact_mut(8) {
		let value = u64::from_le_bytes(limb.try_into().expect("8-byte limb"));
		if value >= GOLDILOCKS_P {
			// A u64 is < 2p, so a single subtraction fully reduces.
			limb.copy_from_slice(&(value - GOLDILOCKS_P).to_le_bytes());
		}
	}
	out
}

/// Compact 8-byte/felt encoding that *reduces* non-canonical limbs mod P.
///
/// The circuit encodes compact bytes with plonky2's `from_noncanonical_u64`, i.e.
/// lossily, and these hashes must be infallible and stay byte-compatible with it.
/// The strict `bytes_to_felts_compact` (which rejects non-canonical limbs) can
/// therefore not be used here — see the encoding-safety invariant on `hash_leaf`
/// for why the lossiness is acceptable.
fn bytes_to_felts_compact_lossy(input: &[u8]) -> impl Iterator<Item = Goldilocks> + '_ {
	qp_poseidon_core::serialization::bytes_to_u64s_compact(input)
		.into_iter()
		.map(Goldilocks::from_u64)
}

/// Hash a leaf using Poseidon with field element encoding.
///
/// The encoding matches the ZK circuit's leaf hash computation:
/// - to_account: 4 felts (32 bytes using 8 bytes/felt compact encoding)
/// - transfer_count: 2 felts (u64 split into two 32-bit limbs, high then low)
/// - asset_id: 1 felt (u32 as u64, then to felt via 8 bytes compact)
/// - amount: 1 felt (u32 quantized, as u64, then to felt via 8 bytes compact)
///
/// Total: 8 felts
///
/// This encoding must exactly match `ZkLeafTargets::collect_for_hash()` in the circuit.
///
/// ## Encoding-safety invariant (see `qp-zk-circuits/formal` audit, gap 7)
///
/// The 8-byte/felt compact encoding is *non-injective*: an 8-byte limb `≥ p`
/// (the Goldilocks prime) is reduced mod `p`, so `to` bytes and their canonical
/// reduction hash identically. Callers must therefore pass a recipient that is
/// already canonical — i.e. run arbitrary recipients through
/// [`canonicalize_account_bytes`] first, and key any per-recipient state (such as
/// the wormhole's transfer count) on that canonical form. Otherwise a recipient
/// and its non-canonical alias get independent count sequences and two distinct
/// deposits can commit to the *same* leaf, which in the wormhole means a shared
/// nullifier and one permanently unexitable deposit. The withdrawal circuit binds
/// the leaf's `to_account` felts to `WA(secret)` (a canonical Poseidon output), so
/// canonicalizing the recipient never changes who can exit a leaf. The
/// `debug_assert` below enforces the caller contract in test/dev builds.
pub fn hash_leaf<T: Config>(leaf: &ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>) -> Hash256
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	use qp_poseidon_core::serialization::u64_to_felts;

	let mut felts = Vec::with_capacity(8);

	// to_account: 4 felts (32 bytes -> 4 felts at 8 bytes/felt)
	let to_bytes = leaf.to.as_ref();
	debug_assert_eq!(to_bytes.len(), 32, "Account must be 32 bytes");
	// Encoding-safety guard: each 8-byte little-endian limb must be canonical
	// (< Goldilocks prime) so the non-injective compact encoding does not silently
	// reduce the recipient. See the invariant note above.
	debug_assert!(
		to_bytes
			.chunks_exact(8)
			.all(|limb| u64::from_le_bytes(limb.try_into().expect("8-byte limb")) < GOLDILOCKS_P),
		"recipient account is non-canonical for the 8-byte/felt leaf encoding"
	);
	felts.extend(bytes_to_felts_compact_lossy(to_bytes));

	// transfer_count: 2 felts (u64 as two 32-bit limbs, high then low)
	felts.extend(u64_to_felts(leaf.transfer_count));

	// asset_id: 1 felt (u32 -> u64 -> 8 bytes LE -> 1 felt via compact encoding)
	// The circuit commits asset IDs as u32. The runtime's asset ID type is u32, so the
	// narrowing is lossless today; saturate (rather than truncate) as hardening for any
	// future runtime with a wider type, so distinct assets can never wrap onto each
	// other's low 32 bits. The debug_assert keeps dev builds loud about it.
	let asset_id_u128: u128 = leaf.asset_id.into();
	let asset_id_u32 = u32::try_from(asset_id_u128).unwrap_or(u32::MAX);
	debug_assert_eq!(asset_id_u128, asset_id_u32 as u128, "Asset ID must fit in u32");
	felts.extend(bytes_to_felts_compact_lossy(&(asset_id_u32 as u64).to_le_bytes()));

	// amount: 1 felt (u32 quantized -> u64 -> 8 bytes LE -> 1 felt via compact encoding)
	// Quantize by dividing by AMOUNT_SCALE_DOWN_FACTOR (10^10)
	// This gives 2 decimal places of precision (1 DEV = 100 quantized units)
	//
	// The circuit commits amounts as u32, so quantized values above u32::MAX are
	// unrepresentable. Saturate instead of truncating: a wrapping cast would let an
	// oversized transfer commit to the same leaf data as a small one (amount mod 2^32),
	// making the authoritative root ambiguous between economically different transfers.
	// Saturation pins every oversized amount to the same transparent cap, from which a
	// prover can only ever claim *at most* the cap. Unreachable for the native token
	// (the 2.1e19-planck supply cap quantizes to ~2.1e9 < u32::MAX) but reachable for
	// permissionlessly minted pallet-assets amounts, which are also recorded here.
	let amount_u128: u128 = leaf.amount.into();
	let amount_quantized =
		u32::try_from(amount_u128 / AMOUNT_SCALE_DOWN_FACTOR).unwrap_or(u32::MAX);
	felts.extend(bytes_to_felts_compact_lossy(&(amount_quantized as u64).to_le_bytes()));

	debug_assert_eq!(felts.len(), 8, "Leaf preimage must be exactly 8 felts");

	// Hash the felts
	qp_poseidon_core::hash_to_bytes(&felts)
}

/// Hash 4 child hashes into a parent node hash.
///
/// Children are sorted before hashing to eliminate the need for path indices
/// in Merkle proofs. This makes verification simpler in ZK circuits - the
/// verifier just needs the siblings, sorts all 4 children, and hashes.
///
/// Uses compact Poseidon encoding (8 bytes/felt) - 128 bytes → 16 felts.
pub fn hash_node(children: &[Hash256; ARITY]) -> Hash256 {
	// Sort children to make hash order-independent
	let mut sorted = *children;
	sorted.sort();

	// Concatenate all 4 child hashes (128 bytes total)
	let mut data = Vec::with_capacity(32 * ARITY);
	for child in &sorted {
		data.extend_from_slice(child);
	}

	// Convert to felts using compact encoding (8 bytes/felt)
	// 128 bytes -> 16 felts. Lossy encoding to match the circuit's `hash_node`;
	// children are Poseidon outputs (canonical) in honest operation, but proof
	// verification may see attacker-supplied siblings and must not fail.
	let felts: Vec<Goldilocks> = bytes_to_felts_compact_lossy(&data).collect();

	// Hash the felts
	qp_poseidon_core::hash_to_bytes(&felts)
}

/// The default hash for an empty subtree.
pub fn empty_hash() -> Hash256 {
	[0u8; 32]
}

/// Get the hash of a leaf by index, or empty hash if not present.
fn get_leaf_hash<T: Config>(index: u64) -> Hash256
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	match crate::Leaves::<T>::get(index) {
		Some(leaf) => hash_leaf::<T>(&leaf),
		None => empty_hash(),
	}
}

/// Get the hash of a node at (level, index), or empty hash if not present.
fn get_node_hash<T: Config>(level: u8, index: u64) -> Hash256 {
	crate::Nodes::<T>::get((level, index)).unwrap_or_else(empty_hash)
}

/// Update the path from a leaf to the root after insertion.
///
/// Returns the new root hash.
pub fn update_path<T: Config>(leaf_index: u64, leaf_hash: Hash256) -> Hash256
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	let depth = crate::Depth::<T>::get();

	if depth == 0 {
		// Special case: first leaf in empty tree - need to initialize
		crate::Depth::<T>::put(1);
		return leaf_hash;
	}

	// Start from leaf level and work up to root
	let mut current_index = leaf_index;
	let mut current_hash = leaf_hash;

	for level in 1..=depth {
		// Find which group of 4 this node belongs to
		let parent_index = current_index / (ARITY as u64);

		// Get all 4 children for this parent
		let mut children = [empty_hash(); ARITY];

		if level == 1 {
			// Children are leaves
			let base_leaf_index = parent_index * (ARITY as u64);
			for (i, child) in children.iter_mut().enumerate() {
				let child_leaf_index = base_leaf_index + (i as u64);
				*child = if child_leaf_index == leaf_index {
					current_hash
				} else {
					get_leaf_hash::<T>(child_leaf_index)
				};
			}
		} else {
			// Children are internal nodes
			let base_child_index = parent_index * (ARITY as u64);
			for (i, child) in children.iter_mut().enumerate() {
				let child_index = base_child_index + (i as u64);
				*child = if child_index == current_index {
					current_hash
				} else {
					get_node_hash::<T>(level - 1, child_index)
				};
			}
		}

		// Compute parent hash
		current_hash = hash_node(&children);

		// Store the node (except at root level, which is stored separately)
		if level < depth {
			crate::Nodes::<T>::insert((level, parent_index), current_hash);
		}

		current_index = parent_index;
	}

	current_hash
}

/// Grow the tree by one level.
///
/// The current root becomes one of the children of the new root.
pub fn grow_tree<T: Config>(old_depth: u8, _new_depth: u8) {
	if old_depth == 0 {
		// Tree was empty, just set depth
		return;
	}

	// The old root hash becomes child[0] of the new root
	let old_root = crate::Root::<T>::get();

	// Store the old root as a node at the old depth level, index 0
	crate::Nodes::<T>::insert((old_depth, 0), old_root);

	// The new root will be computed when the next leaf triggers update_path
	// For now, compute it with empty siblings
	let mut children = [empty_hash(); ARITY];
	children[0] = old_root;
	let new_root = hash_node(&children);

	crate::Root::<T>::put(new_root);
}

/// Generate a Merkle proof for a leaf at the given index.
///
/// Returns siblings at each level. No path indices needed because children
/// are sorted before hashing - the verifier can reconstruct by sorting.
pub fn generate_proof<T: Config>(leaf_index: u64, depth: u8) -> Result<ZkMerkleProof, Error<T>>
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	if depth == 0 {
		return Err(Error::<T>::LeafNotFound);
	}

	let leaf_hash = get_leaf_hash::<T>(leaf_index);

	let mut siblings = Vec::with_capacity(depth as usize);
	let mut current_index = leaf_index;
	let mut current_hash = leaf_hash;

	for level in 1..=depth {
		let parent_index = current_index / (ARITY as u64);

		// Get sibling hashes (the other 3 children)
		let mut level_siblings = [empty_hash(); 3];
		let mut sibling_idx = 0;

		let base_index = parent_index * (ARITY as u64);
		for i in 0..ARITY {
			let child_index = base_index + (i as u64);
			if child_index == current_index {
				continue; // Skip self
			}

			let hash = if level == 1 {
				get_leaf_hash::<T>(child_index)
			} else {
				get_node_hash::<T>(level - 1, child_index)
			};

			level_siblings[sibling_idx] = hash;
			sibling_idx += 1;
		}

		// Compute parent hash for next iteration
		let children: [Hash256; ARITY] =
			[current_hash, level_siblings[0], level_siblings[1], level_siblings[2]];
		current_hash = hash_node(&children);

		siblings.push(level_siblings);
		current_index = parent_index;
	}

	Ok(ZkMerkleProof { leaf_index, siblings })
}

/// Verify a Merkle proof against a given root.
///
/// No path indices needed - we combine current hash with siblings, sort all 4,
/// and hash. This works because `hash_node` sorts children before hashing.
pub fn verify_proof<T: Config>(
	leaf: &ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>,
	proof: &ZkMerkleProof,
	expected_root: Hash256,
) -> bool
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	let mut current_hash = hash_leaf::<T>(leaf);

	for level_siblings in &proof.siblings {
		// Combine current hash with 3 siblings to get all 4 children
		let children: [Hash256; ARITY] =
			[current_hash, level_siblings[0], level_siblings[1], level_siblings[2]];

		// hash_node sorts internally, so order doesn't matter
		current_hash = hash_node(&children);
	}

	current_hash == expected_root
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_empty_hash() {
		let h = empty_hash();
		assert_eq!(h, [0u8; 32]);
	}

	#[test]
	#[ignore]
	fn measure_hash_node_time() {
		let mut acc = [7u8; 32];
		let siblings = [[1u8; 32], [2u8; 32], [3u8; 32]];
		// Warm up
		for _ in 0..1000 {
			acc = hash_node(&[acc, siblings[0], siblings[1], siblings[2]]);
		}
		let n = 100_000u32;
		let start = std::time::Instant::now();
		for _ in 0..n {
			acc = hash_node(&[acc, siblings[0], siblings[1], siblings[2]]);
		}
		let elapsed = start.elapsed();
		println!(
			"hash_node: {} ns/call over {} calls (sink {:?})",
			elapsed.as_nanos() / n as u128,
			n,
			acc[0]
		);
	}

	#[test]
	fn canonicalize_is_identity_on_canonical_accounts() {
		// All limbs < p (largest canonical limb is p - 1).
		let mut bytes = [0x11u8; 32];
		bytes[24..32].copy_from_slice(&(GOLDILOCKS_P - 1).to_le_bytes());
		assert_eq!(canonicalize_account_bytes(bytes), bytes);
	}

	#[test]
	fn canonicalize_reduces_each_non_canonical_limb() {
		// Each limb i holds p + i (fits in u64 since i < 2^32 - 1) and must reduce to i.
		let mut bytes = [0u8; 32];
		for i in 0..4u64 {
			let start = (i as usize) * 8;
			bytes[start..start + 8].copy_from_slice(&(GOLDILOCKS_P + i).to_le_bytes());
		}
		let mut expected = [0u8; 32];
		for i in 0..4u64 {
			let start = (i as usize) * 8;
			expected[start..start + 8].copy_from_slice(&i.to_le_bytes());
		}
		assert_eq!(canonicalize_account_bytes(bytes), expected);
	}

	#[test]
	fn canonicalize_reduces_max_limb() {
		// u64::MAX = p + (2^32 - 2): the largest possible limb still reduces correctly.
		let mut bytes = [0u8; 32];
		bytes[0..8].copy_from_slice(&u64::MAX.to_le_bytes());
		let out = canonicalize_account_bytes(bytes);
		let reduced = u64::from_le_bytes(out[0..8].try_into().unwrap());
		assert_eq!(reduced, u64::MAX - GOLDILOCKS_P);
		assert!(reduced < GOLDILOCKS_P);
		// Other limbs untouched.
		assert_eq!(&out[8..], &bytes[8..]);
	}
}
