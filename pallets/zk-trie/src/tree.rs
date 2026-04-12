//! 4-ary Poseidon Merkle tree implementation.
//!
//! This module provides the core tree operations:
//! - Leaf hashing (injective, 4 bytes/felt)
//! - Node hashing (non-injective, 8 bytes/felt)
//! - Path updates on insert
//! - Proof generation and verification
//! - Tree growth when capacity is exceeded

use crate::{
	pallet::{AccountIdOf, Config, Error},
	Hash256, ZkLeaf, ZkMerkleProof, ARITY,
};
use alloc::vec::Vec;

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

/// Hash a leaf using injective Poseidon (4 bytes/felt).
///
/// This ensures collision resistance for user-controlled data.
pub fn hash_leaf<T: Config>(leaf: &ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>) -> Hash256
where
	AccountIdOf<T>: AsRef<[u8]>,
{
	// Encode leaf data to bytes
	let mut data = Vec::new();

	// to: account bytes (should be 32 bytes)
	data.extend_from_slice(leaf.to.as_ref());

	// transfer_count: 8 bytes LE
	data.extend_from_slice(&leaf.transfer_count.to_le_bytes());

	// asset_id: as u128, 16 bytes LE
	let asset_id_u128: u128 = leaf.asset_id.into();
	data.extend_from_slice(&asset_id_u128.to_le_bytes());

	// amount: as u128, 16 bytes LE
	let amount_u128: u128 = leaf.amount.into();
	data.extend_from_slice(&amount_u128.to_le_bytes());

	// Use injective hash (4 bytes/felt)
	qp_poseidon_core::hash_bytes(&data)
}

/// Hash 4 child hashes into a parent node hash.
///
/// Children are sorted before hashing to eliminate the need for path indices
/// in Merkle proofs. This makes verification simpler in ZK circuits - the
/// verifier just needs the siblings, sorts all 4 children, and hashes.
///
/// Uses non-injective Poseidon (8 bytes/felt) since internal nodes
/// only contain fixed-size hash outputs, not user-controlled data.
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
	// 128 bytes -> 16 felts
	let felts = qp_poseidon_core::serialization::bytes_to_felts_compact(&data);

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
			for i in 0..ARITY {
				let child_leaf_index = base_leaf_index + (i as u64);
				if child_leaf_index == leaf_index {
					children[i] = current_hash;
				} else {
					children[i] = get_leaf_hash::<T>(child_leaf_index);
				}
			}
		} else {
			// Children are internal nodes
			let base_child_index = parent_index * (ARITY as u64);
			for i in 0..ARITY {
				let child_index = base_child_index + (i as u64);
				if child_index == current_index {
					children[i] = current_hash;
				} else {
					children[i] = get_node_hash::<T>(level - 1, child_index);
				}
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

	let mut siblings = Vec::with_capacity(depth as usize);
	let mut current_index = leaf_index;

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
}
