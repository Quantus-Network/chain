//! 4-ary Poseidon Merkle tree implementation.
//!
//! This module provides the core tree operations:
//! - Leaf hashing (8 felts, injective for ≤32-bit values)
//! - Node hashing (16 felts, 8 bytes/felt compact encoding)
//! - Batched (once-per-block) folding of appended leaves into the tree
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

/// Quantization factor the chain is migrating to (10^8: 4 decimal places, a 0.0001 QTC
/// leaf quantum and wormhole fee floor). Leaves are already capped against this value
/// so that every leaf recorded from now on stays representable once the switch happens.
pub const TARGET_AMOUNT_SCALE_DOWN_FACTOR: u128 = 100_000_000;

/// Largest amount a single leaf may carry: `u32::MAX` quanta at the target scale,
/// floored to a whole number of current quanta (429,496.72 QTC). The circuit
/// range-checks every leaf and exit-slot amount to 32 bits, so a leaf above the
/// target-scale ceiling would be exitable only for its saturated value. Keeping the
/// cap aligned to the current quantum lets [`credit_chunks`] round chunks up to that
/// quantum without ever crossing the ceiling. Credits above the cap are split into
/// several leaves.
pub const LEAF_AMOUNT_CAP: u128 = ((u32::MAX as u128) * TARGET_AMOUNT_SCALE_DOWN_FACTOR /
	AMOUNT_SCALE_DOWN_FACTOR) *
	AMOUNT_SCALE_DOWN_FACTOR;

/// Upper bound on the leaves one credit can split into. Native credits are bounded by
/// the 21M QTC maximum supply (`ceil(21_000_000 QTC / LEAF_AMOUNT_CAP) = 49`), which
/// the runtime asserts against this constant; the recorder clamps here so an
/// out-of-contract caller can never make a single credit insert unbounded leaves.
pub const MAX_LEAVES_PER_CREDIT: u32 = 49;

const _: () = assert!(
	LEAF_AMOUNT_CAP / TARGET_AMOUNT_SCALE_DOWN_FACTOR <= u32::MAX as u128,
	"a capped leaf must not saturate at the target scale"
);
const _: () = assert!(
	LEAF_AMOUNT_CAP <= (u32::MAX as u128) * AMOUNT_SCALE_DOWN_FACTOR,
	"a capped leaf must not saturate at the current scale"
);
const _: () = assert!(
	LEAF_AMOUNT_CAP.is_multiple_of(AMOUNT_SCALE_DOWN_FACTOR),
	"the leaf cap must be a whole number of current quanta"
);

/// Number of leaves a credit of `amount` is recorded as: one up to
/// [`LEAF_AMOUNT_CAP`], then `ceil(amount / LEAF_AMOUNT_CAP)`, clamped to
/// [`MAX_LEAVES_PER_CREDIT`].
///
/// The clamp is unreachable for native credits (the runtime asserts the supply
/// fits the bound); a clamped split yields chunks above the cap, so it is
/// logged as an error rather than failing a credit that already happened.
pub fn leaves_for_amount(amount: u128) -> u32 {
	if amount <= LEAF_AMOUNT_CAP {
		return 1;
	}
	let n = u32::try_from(amount.div_ceil(LEAF_AMOUNT_CAP)).unwrap_or(u32::MAX);
	if n > MAX_LEAVES_PER_CREDIT {
		log::error!(
			target: "runtime::zk-tree",
			"credit of {amount} needs {n} leaves, clamped to {MAX_LEAVES_PER_CREDIT}: chunks exceed LEAF_AMOUNT_CAP"
		);
		return MAX_LEAVES_PER_CREDIT;
	}
	n
}

/// Per-leaf amounts a credit is split into; see [`credit_chunks`].
pub struct CreditChunks {
	remaining: u32,
	chunk: u128,
	last: u128,
}

impl Iterator for CreditChunks {
	type Item = u128;

	fn next(&mut self) -> Option<u128> {
		match self.remaining {
			0 => None,
			1 => {
				self.remaining = 0;
				Some(self.last)
			},
			_ => {
				self.remaining -= 1;
				Some(self.chunk)
			},
		}
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.remaining as usize, Some(self.remaining as usize))
	}
}

impl ExactSizeIterator for CreditChunks {}

/// Split a credit into [`leaves_for_amount`] balanced leaf amounts that sum to
/// `amount`.
///
/// All but the last chunk are `amount / n` rounded *up* to the current quantum;
/// the last chunk carries the remainder (and any sub-quantum dust). Rounding up
/// keeps every chunk at or below [`LEAF_AMOUNT_CAP`] whenever the split is not
/// clamped (`amount / n ≤ cap` and the cap is quantum-aligned), while balancing
/// avoids a dust-sized final leaf that would cost a whole fee quantum to exit. A
/// credit at or below the cap is returned unchanged as a single chunk.
pub fn credit_chunks(amount: u128) -> CreditChunks {
	let n = leaves_for_amount(amount);
	if n == 1 {
		return CreditChunks { remaining: 1, chunk: amount, last: amount };
	}
	let chunk = (amount / n as u128).div_ceil(AMOUNT_SCALE_DOWN_FACTOR) * AMOUNT_SCALE_DOWN_FACTOR;
	let last = amount.saturating_sub(chunk.saturating_mul((n - 1) as u128));
	CreditChunks { remaining: n, chunk, last }
}

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
pub fn hash_leaf<T: Config>(leaf: &ZkLeaf<AccountIdOf<T>, T::AssetId, T::Balance>) -> Hash256 {
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
fn get_leaf_hash<T: Config>(index: u64) -> Hash256 {
	match crate::Leaves::<T>::get(index) {
		Some(leaf) => hash_leaf::<T>(&leaf),
		None => empty_hash(),
	}
}

/// Get the hash of a node at (level, index), or empty hash if not present.
fn get_node_hash<T: Config>(level: u8, index: u64) -> Hash256 {
	crate::Nodes::<T>::get((level, index)).unwrap_or_else(empty_hash)
}

/// Fold the contiguous, freshly appended leaf range `[start, end)` into the tree
/// in one bottom-up pass and return the new root hash.
///
/// Callers must ensure that every leaf `< start` is already reflected in `Nodes`
/// (and `Root`), that `depth` is large enough to fit `end` leaves, and — when the
/// tree grew to reach `depth` — that [`grow_tree`] has parked the old root first.
///
/// Because the appended leaves are contiguous, the dirty nodes at every level form
/// a contiguous index range `[lo, hi]`. Each dirty node is hashed exactly once, so
/// a batch of `n` leaves costs about `n/4 + n/16 + … + depth` node hashes instead
/// of the `n · depth` a per-insert path update would pay. This is what makes the
/// once-per-block (`on_finalize`) root computation cheap.
pub fn update_range<T: Config>(start: u64, end: u64, depth: u8) -> Hash256 {
	debug_assert!(start < end, "leaf range must be non-empty");
	debug_assert!(capacity_at_depth(depth) >= end, "depth must fit the whole range");

	// Hashes of the dirty nodes at the current level, covering indices [lo, hi].
	let mut lo = start;
	let mut hi = end - 1;
	let mut dirty: Vec<Hash256> = (start..end).map(get_leaf_hash::<T>).collect();

	for level in 1..=depth {
		let parent_lo = lo / (ARITY as u64);
		let parent_hi = hi / (ARITY as u64);
		let mut parents = Vec::with_capacity((parent_hi - parent_lo + 1) as usize);

		for parent in parent_lo..=parent_hi {
			let base = parent * (ARITY as u64);
			let mut children = [empty_hash(); ARITY];
			for (i, child) in children.iter_mut().enumerate() {
				let index = base + i as u64;
				*child = if index >= lo && index <= hi {
					dirty[(index - lo) as usize]
				} else if index > hi {
					// The tree is append-only: everything to the right of the
					// last appended leaf is still empty — skip the storage read.
					empty_hash()
				} else if level == 1 {
					// Pre-existing leaves sharing the boundary parent with `start`.
					get_leaf_hash::<T>(index)
				} else {
					get_node_hash::<T>(level - 1, index)
				};
			}

			let hash = hash_node(&children);
			// The root lives in `Root`, not `Nodes`.
			if level < depth {
				crate::Nodes::<T>::insert((level, parent), hash);
			}
			parents.push(hash);
		}

		dirty = parents;
		lo = parent_lo;
		hi = parent_hi;
	}

	debug_assert_eq!((lo, hi), (0, 0), "range must converge to the root");
	dirty[0]
}

/// Prepare the tree to grow beyond `old_depth`.
///
/// The subtree that used to be the whole tree becomes child 0 of the first new
/// level, so the current `Root` has to move into `Nodes` at `(old_depth, 0)`.
/// Everything above it is computed by the next [`update_range`] pass: every node
/// on the new levels sits on the dirty path of the leaves that triggered the
/// growth. If the pending range also dirties `(old_depth, 0)` itself (i.e. the
/// range starts inside the old subtree), the parked value is simply overwritten
/// by that same pass.
pub fn grow_tree<T: Config>(old_depth: u8) {
	if old_depth == 0 {
		// Tree was empty; there is no old root to park.
		return;
	}
	crate::Nodes::<T>::insert((old_depth, 0), crate::Root::<T>::get());
}

/// Generate a Merkle proof for a leaf at the given index.
///
/// Returns siblings at each level. No path indices needed because children
/// are sorted before hashing - the verifier can reconstruct by sorting.
pub fn generate_proof<T: Config>(leaf_index: u64, depth: u8) -> Result<ZkMerkleProof, Error<T>> {
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
) -> bool {
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

	const UNIT: u128 = 1_000_000_000_000;
	const MAX_SUPPLY: u128 = 21_000_000 * UNIT;

	fn assert_chunks_well_formed(amount: u128) -> Vec<u128> {
		let chunks: Vec<u128> = credit_chunks(amount).collect();
		assert_eq!(chunks.len(), leaves_for_amount(amount) as usize);
		assert_eq!(chunks.iter().sum::<u128>(), amount, "chunks must sum to the credit");
		assert!(chunks.iter().all(|c| *c <= LEAF_AMOUNT_CAP), "every chunk must fit a leaf");
		assert!(
			chunks.iter().all(|c| *c >= AMOUNT_SCALE_DOWN_FACTOR),
			"no chunk may commit a zero-value leaf"
		);
		assert!(
			chunks[..chunks.len() - 1].iter().all(|c| c % AMOUNT_SCALE_DOWN_FACTOR == 0),
			"all but the last chunk must be quantum-aligned"
		);
		chunks
	}

	#[test]
	fn leaf_cap_is_the_largest_aligned_amount_below_u32_max_target_quanta() {
		assert_eq!(LEAF_AMOUNT_CAP, 429_496_720_000_000_000);
		assert!(
			u32::try_from(LEAF_AMOUNT_CAP / TARGET_AMOUNT_SCALE_DOWN_FACTOR).is_ok(),
			"the cap must not saturate at the target scale"
		);
		assert!(
			u32::try_from(
				(LEAF_AMOUNT_CAP + AMOUNT_SCALE_DOWN_FACTOR) / TARGET_AMOUNT_SCALE_DOWN_FACTOR
			)
			.is_err(),
			"one current quantum more would saturate at the target scale"
		);
		assert_eq!(MAX_LEAVES_PER_CREDIT as u128, MAX_SUPPLY.div_ceil(LEAF_AMOUNT_CAP));
	}

	#[test]
	fn credits_at_or_below_cap_are_a_single_unchanged_chunk() {
		for amount in
			[0, 1, AMOUNT_SCALE_DOWN_FACTOR - 1, UNIT, LEAF_AMOUNT_CAP - 1, LEAF_AMOUNT_CAP]
		{
			assert_eq!(leaves_for_amount(amount), 1);
			assert_eq!(credit_chunks(amount).collect::<Vec<_>>(), vec![amount]);
		}
	}

	#[test]
	fn credits_above_cap_split_into_balanced_capped_chunks() {
		let amounts = [
			LEAF_AMOUNT_CAP + 1,
			LEAF_AMOUNT_CAP + AMOUNT_SCALE_DOWN_FACTOR,
			2 * LEAF_AMOUNT_CAP - 1,
			2 * LEAF_AMOUNT_CAP,
			2 * LEAF_AMOUNT_CAP + 1,
			3 * LEAF_AMOUNT_CAP - AMOUNT_SCALE_DOWN_FACTOR + 7,
			5_670_000 * UNIT + 123_456_789,
			49 * LEAF_AMOUNT_CAP - 1,
			MAX_SUPPLY,
			MAX_SUPPLY - 1,
		];
		for amount in amounts {
			let chunks = assert_chunks_well_formed(amount);
			assert!(chunks.len() >= 2, "{amount} must split");
			let spread = chunks.iter().max().unwrap() - chunks.iter().min().unwrap();
			assert!(
				spread <= (chunks.len() as u128) * AMOUNT_SCALE_DOWN_FACTOR,
				"chunks of {amount} must be balanced, spread {spread}"
			);
		}
	}

	#[test]
	fn just_above_cap_splits_in_two_rather_than_cap_plus_dust() {
		let chunks = assert_chunks_well_formed(LEAF_AMOUNT_CAP + 1);
		assert_eq!(chunks.len(), 2);
		// Balanced: each half is ~214,748 QTC, not (cap, 1 planck).
		assert!(chunks[0] < LEAF_AMOUNT_CAP / 2 + AMOUNT_SCALE_DOWN_FACTOR);
		assert!(chunks[1] > LEAF_AMOUNT_CAP / 2 - AMOUNT_SCALE_DOWN_FACTOR);
	}

	#[test]
	fn max_supply_fits_the_leaf_bound() {
		assert_eq!(leaves_for_amount(MAX_SUPPLY), MAX_LEAVES_PER_CREDIT);
		assert!(MAX_SUPPLY <= MAX_LEAVES_PER_CREDIT as u128 * LEAF_AMOUNT_CAP);
	}

	#[test]
	fn leaves_are_clamped_for_out_of_contract_amounts() {
		assert_eq!(leaves_for_amount(u128::MAX), MAX_LEAVES_PER_CREDIT);
		let chunks: Vec<u128> = credit_chunks(u128::MAX).collect();
		assert_eq!(chunks.len(), MAX_LEAVES_PER_CREDIT as usize);
		assert!(chunks.iter().all(|c| *c > 0));
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
