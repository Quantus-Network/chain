//! Fork of sp-runtime's generic implementation of a block header.
//!
//! Key differences from the standard Substrate header:
//! - **Block hash**: Computed with Poseidon for ZK circuit compatibility
//! - **State trie**: Uses Blake2 (via `Hashing` type parameter) for efficient native execution
//!
//! This means `HashingFor<Block>` returns `BlakeTwo256`, which is used for:
//! - State trie merkle root computation
//! - Extrinsics root computation
//! - Storage proof verification

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Codec, Decode, DecodeWithMemTracking, Encode};
use qp_poseidon_core::{
	hash_to_bytes,
	serialization::{bytes_to_digest_lossy, bytes_to_felts},
	Goldilocks,
};
use scale_info::TypeInfo;
use sp_core::{H256, U256};
use sp_runtime::{
	generic::{Digest, DigestItem},
	traits::{AtLeast32BitUnsigned, BlockNumber, Hash as HashT, MaybeDisplay, Member},
	RuntimeDebug,
};
extern crate alloc;

use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Size of the fixed digest window (in bytes) committed by [`Header::hash`].
///
/// The SCALE-encoded digest is folded into the Poseidon preimage through a
/// buffer of exactly this size, and the wormhole circuit hashes a digest field
/// of the same width. It is sized to hold exactly one 32-byte pre-runtime
/// preimage plus one 64-byte PoW seal (the canonical post-seal digest), so a
/// well-formed header uses the whole window with no slack.
///
/// A well-formed sealed digest must encode to **exactly** this size. Import
/// rejects any other length rather than padding or truncating; see
/// [`check_digest_commitment_window`]. `Header::hash()` pads short encodings
/// with zeros and drops bytes past this window, so either mismatch would let
/// two distinct headers share a block hash.
///
/// Because the window has no slack, the runtime must never deposit digest items
/// of its own: even a 1-byte item (e.g. upstream frame-system's
/// `RuntimeEnvironmentUpdated` on `set_code`) pushes the sealed digest to 111
/// bytes. The vendored frame-system's deposits were removed for exactly this
/// reason — see the warning on `frame_system::Pallet::deposit_log`. One
/// historical exception is grandfathered at import: see
/// [`check_digest_commitment_window`].
pub const DIGEST_LOGS_SIZE: usize = 110;

/// Maximum accepted length (in bytes) of a SCALE-encoded header digest for
/// blocks at or below [`LEGACY_DIGEST_CUTOFF`].
///
/// One byte beyond [`DIGEST_LOGS_SIZE`], for backwards compatibility:
/// historical blocks minted before the runtime stopped depositing
/// `RuntimeEnvironmentUpdated` on `set_code` carry that 1-byte item between
/// the pre-runtime preimage and the seal, encoding to 111 bytes, and are part
/// of the canonical chain. In that shape the byte past the window is the
/// seal's final byte, which [`Header::hash`] does not commit; that is
/// tolerable for the fixed historical range because a header forged on the
/// uncommitted byte still has to pass PoW seal verification, so a colliding
/// variant can only be a transiently rejected duplicate, never a consensus
/// fork. Blocks above the cutoff get no slack, restoring the full-commitment
/// invariant for the indefinite future; use [`max_encoded_digest_size`].
pub const MAX_ENCODED_DIGEST_SIZE: usize = DIGEST_LOGS_SIZE + 1;

/// Last block height eligible for the 1-byte historical digest allowance.
///
/// All `RuntimeEnvironmentUpdated`-carrying blocks were minted long before
/// this height; it is set slightly above the testnet tip at the time the
/// bound was introduced so already-mined history keeps importing, while
/// every future block is capped at the fully hash-committed
/// [`DIGEST_LOGS_SIZE`].
pub const LEGACY_DIGEST_CUTOFF: u64 = 1_000_000;

/// Maximum accepted encoded digest length for a block at the given height.
pub fn max_encoded_digest_size(block_number: u64) -> usize {
	if block_number <= LEGACY_DIGEST_CUTOFF {
		MAX_ENCODED_DIGEST_SIZE
	} else {
		DIGEST_LOGS_SIZE
	}
}

/// Returns `Ok(())` if `digest` may be imported at `block_number`, or
/// `Err(encoded_len)` if it is not a permitted commitment-window shape.
///
/// The only accepted encodings are:
/// - [`DIGEST_LOGS_SIZE`] — canonical `[PreRuntime, Seal]`
/// - [`DIGEST_LOGS_SIZE`] `+ 1` with exactly one `RuntimeEnvironmentUpdated` item, and only at
///   heights `<=` [`LEGACY_DIGEST_CUTOFF`]. That leftover is from 0.8.x `set_code` deposits: those
///   upgrade blocks are already canonical, and 0.8.x imported them by truncating the Poseidon
///   preimage.
///
/// Anything shorter is padded with zeros by [`Header::hash`] and anything else
/// longer is truncated, so both must be rejected. A 111-byte digest that is
/// not exactly one `RuntimeEnvironmentUpdated` is also rejected, even below
/// the cutoff.
pub fn check_digest_commitment_window(digest: &Digest, block_number: u64) -> Result<(), usize> {
	let encoded_len = digest.encode().len();
	if encoded_len == DIGEST_LOGS_SIZE {
		return Ok(());
	}
	let reu_count = digest
		.logs
		.iter()
		.filter(|item| matches!(item, DigestItem::RuntimeEnvironmentUpdated))
		.count();
	if reu_count == 1 && encoded_len == DIGEST_LOGS_SIZE + 1 && block_number <= LEGACY_DIGEST_CUTOFF
	{
		return Ok(());
	}
	Err(encoded_len)
}

/// Extension trait for headers that support ZK tree root.
///
/// This trait allows frame_system to set the ZK Merkle tree root on headers
/// without knowing the concrete header type.
pub trait ZkTreeRootProvider {
	/// The hash type used for the ZK tree root.
	type Hash;

	/// Set the ZK tree root.
	fn set_zk_tree_root(&mut self, root: Self::Hash);

	/// Get the ZK tree root.
	fn zk_tree_root(&self) -> &Self::Hash;
}

/// Custom block header with Poseidon block hash and configurable state trie hasher.
///
/// - Block hash is computed using Poseidon for ZK circuit compatibility
/// - `Hashing` type parameter is used for state trie / extrinsics root (typically BlakeTwo256)
///
/// ## Field Ordering
///
/// The `zk_tree_root` field is intentionally placed **before** `digest` to ensure
/// a fixed offset in the header preimage. This prevents miners from manipulating
/// the digest to shift the ZK root's position in the felt encoding.
#[derive(Encode, Decode, PartialEq, Eq, Clone, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
#[scale_info(skip_type_params(Hashing))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Header<Number, Hashing: HashT>
where
	Number: Copy + Into<U256> + TryFrom<U256>,
{
	pub parent_hash: H256,
	#[cfg_attr(
		feature = "serde",
		serde(serialize_with = "serialize_number", deserialize_with = "deserialize_number")
	)]
	pub number: Number,
	pub state_root: H256,
	pub extrinsics_root: H256,
	/// Root of the ZK Merkle tree (4-ary Poseidon tree).
	///
	/// This is placed before `digest` to ensure a fixed offset in the header
	/// preimage for ZK circuit verification.
	pub zk_tree_root: H256,
	pub digest: Digest,
	#[codec(skip)]
	#[cfg_attr(feature = "serde", serde(skip))]
	_marker: core::marker::PhantomData<Hashing>,
}

#[cfg(feature = "serde")]
pub fn serialize_number<S, T: Copy + Into<U256> + TryFrom<U256>>(
	val: &T,
	s: S,
) -> Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	let u256: U256 = (*val).into();
	serde::Serialize::serialize(&u256, s)
}

#[cfg(feature = "serde")]
pub fn deserialize_number<'a, D, T: Copy + Into<U256> + TryFrom<U256>>(d: D) -> Result<T, D::Error>
where
	D: serde::Deserializer<'a>,
{
	let u256: U256 = serde::Deserialize::deserialize(d)?;
	TryFrom::try_from(u256).map_err(|_| serde::de::Error::custom("Try from failed"))
}

impl<Number, Hashing> sp_runtime::traits::Header for Header<Number, Hashing>
where
	Number: BlockNumber,
	Hashing: HashT<Output = H256>,
{
	type Number = Number;
	type Hash = H256;
	/// State trie hasher - configurable, defaults to BlakeTwo256.
	type Hashing = Hashing;

	fn new(
		number: Self::Number,
		extrinsics_root: Self::Hash,
		state_root: Self::Hash,
		parent_hash: Self::Hash,
		digest: Digest,
	) -> Self {
		Self {
			number,
			extrinsics_root,
			state_root,
			parent_hash,
			// Initialize with zero; pallet-zk-tree will set the actual root
			zk_tree_root: H256::zero(),
			digest,
			_marker: core::marker::PhantomData,
		}
	}
	fn number(&self) -> &Self::Number {
		&self.number
	}

	fn set_number(&mut self, num: Self::Number) {
		self.number = num
	}
	fn extrinsics_root(&self) -> &Self::Hash {
		&self.extrinsics_root
	}

	fn set_extrinsics_root(&mut self, root: Self::Hash) {
		self.extrinsics_root = root
	}
	fn state_root(&self) -> &Self::Hash {
		&self.state_root
	}

	fn set_state_root(&mut self, root: Self::Hash) {
		self.state_root = root
	}
	fn parent_hash(&self) -> &Self::Hash {
		&self.parent_hash
	}

	fn set_parent_hash(&mut self, hash: Self::Hash) {
		self.parent_hash = hash
	}

	fn digest(&self) -> &Digest {
		&self.digest
	}

	fn digest_mut(&mut self) -> &mut Digest {
		#[cfg(feature = "std")]
		log::debug!(target: "header", "Retrieving mutable reference to digest");
		&mut self.digest
	}
	// We override the default hashing function to use
	// a felt aligned pre-image for poseidon hashing.
	fn hash(&self) -> Self::Hash {
		Header::hash(self)
	}
}

impl<Number, Hashing> Header<Number, Hashing>
where
	Number: Member
		+ core::hash::Hash
		+ Copy
		+ MaybeDisplay
		+ AtLeast32BitUnsigned
		+ Codec
		+ Into<U256>
		+ TryFrom<U256>,
	Hashing: HashT,
{
	/// Convenience helper for computing the hash of the header without having
	/// to import the trait.
	pub fn hash(&self) -> H256 {
		// 4 hash fields (4 felts each) + 1 u32 + 28 felts for injective digest encoding
		let max_encoded_felts = 4 * 4 + 1 + 28;
		let mut felts = Vec::with_capacity(max_encoded_felts);

		// Encoding note: the 8-byte/felt `bytes_to_digest_lossy` is non-injective
		// (limbs ≥ the Goldilocks prime are reduced mod p). It is lossless only for
		// canonical Goldilocks digests (our Poseidon outputs: parent_hash is a prior
		// Poseidon block hash; zk_tree_root is a Poseidon node hash). `state_root`
		// and `extrinsics_root` are substrate Blake2-256 outputs, which are NOT
		// canonical field elements, so this hash is a *lossy* commitment to them.
		// That is acceptable here: only `zk_tree_root` (canonical, fixed offset) and
		// `parent_hash` need an injective binding for the wormhole circuit. We use
		// the explicitly-lossy variant (not the fallible strict `bytes_to_digest`)
		// because `hash()` must be infallible even on adversarial header bytes.

		// parent_hash : 32 bytes → 4 felts (canonical Poseidon block hash)
		felts.extend(bytes_to_digest_lossy(
			self.parent_hash.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// block number as u64 (compact encoded, but we only need the value)
		// constrain the block number to be with u32 range for simplicity
		// NOTE: only the low 32 bits are committed (the wormhole circuit hashes the same
		// single-felt layout). Lossless for the runtime's `BlockNumber = u32`; do NOT
		// instantiate this header with a wider `Number` type, or headers whose heights
		// agree in the low 32 bits would share a block-number hash input.
		let number = self.number.into();
		felts.push(Goldilocks::new(number.as_u32() as u64));

		// state_root : 32 bytes → 4 felts (Blake2-256: non-canonical, lossy — see note above)
		felts.extend(bytes_to_digest_lossy(
			self.state_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// extrinsics_root : 32 bytes → 4 felts (Blake2-256: non-canonical, lossy — see note above)
		felts.extend(bytes_to_digest_lossy(
			self.extrinsics_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// zk_tree_root : 32 bytes → 4 felts (canonical Poseidon node hash)
		// Placed before digest to ensure fixed offset regardless of digest content
		felts.extend(bytes_to_digest_lossy(
			self.zk_tree_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// digest – SCALE encode then pad to the fixed DIGEST_LOGS_SIZE window to
		// match circuit expectation. Bytes past the window are NOT committed by
		// this hash: an encoded digest longer than the window would let two
		// distinct headers collide, which is why the import path rejects such
		// headers outright (see [`check_digest_commitment_window`]).
		// No assertion here, not even a debug one: generic network code hashes
		// completely unverified headers long before any consensus check runs
		// (e.g. block-announce validation in `sc-network-sync` hashes the
		// announced header on receipt), so `hash()` must accept arbitrary
		// adversarial input and the saturating copy below is the required
		// behavior, not a backstop.
		let digest_encoded = self.digest.encode();
		let mut digest_padded = [0u8; DIGEST_LOGS_SIZE];
		let copy_len = digest_encoded.len().min(DIGEST_LOGS_SIZE);
		digest_padded[..copy_len].copy_from_slice(&digest_encoded[..copy_len]);

		// injective encoding (4 bytes/felt + terminator)
		felts.extend(bytes_to_felts(&digest_padded));

		let poseidon_hash: [u8; 32] = hash_to_bytes(&felts);
		H256::from(poseidon_hash)
	}

	/// Create a new header with all fields including zk_tree_root.
	///
	/// This is the preferred constructor when you have the ZK tree root available.
	/// Use this instead of `Header::new` + `set_zk_tree_root`.
	pub fn new_with_zk_root(
		number: Number,
		extrinsics_root: H256,
		state_root: H256,
		parent_hash: H256,
		zk_tree_root: H256,
		digest: Digest,
	) -> Self {
		Self {
			parent_hash,
			number,
			state_root,
			extrinsics_root,
			zk_tree_root,
			digest,
			_marker: core::marker::PhantomData,
		}
	}

	/// Get the ZK tree root.
	pub fn zk_tree_root(&self) -> &H256 {
		&self.zk_tree_root
	}

	/// Set the ZK tree root.
	///
	/// Called by pallet-zk-tree during block finalization.
	pub fn set_zk_tree_root(&mut self, root: H256) {
		self.zk_tree_root = root;
	}
}

impl<Number, Hashing> ZkTreeRootProvider for Header<Number, Hashing>
where
	Number: Copy + Into<U256> + TryFrom<U256>,
	Hashing: HashT,
{
	type Hash = H256;

	fn set_zk_tree_root(&mut self, root: Self::Hash) {
		self.zk_tree_root = root;
	}

	fn zk_tree_root(&self) -> &Self::Hash {
		&self.zk_tree_root
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_core::H256;
	use sp_runtime::{traits::BlakeTwo256, DigestItem};

	#[test]
	fn should_serialize_numbers() {
		fn serialize(num: u128) -> String {
			let mut v = vec![];
			{
				let mut ser = serde_json::Serializer::new(std::io::Cursor::new(&mut v));
				serialize_number(&num, &mut ser).unwrap();
			}
			String::from_utf8(v).unwrap()
		}

		assert_eq!(serialize(0), "\"0x0\"".to_owned());
		assert_eq!(serialize(1), "\"0x1\"".to_owned());
		assert_eq!(serialize(u64::MAX as u128), "\"0xffffffffffffffff\"".to_owned());
		assert_eq!(serialize(u64::MAX as u128 + 1), "\"0x10000000000000000\"".to_owned());
	}

	#[test]
	fn should_deserialize_number() {
		fn deserialize(num: &str) -> u128 {
			let mut der = serde_json::Deserializer::from_str(num);
			deserialize_number(&mut der).unwrap()
		}

		assert_eq!(deserialize("\"0x0\""), 0);
		assert_eq!(deserialize("\"0x1\""), 1);
		assert_eq!(deserialize("\"0xffffffffffffffff\""), u64::MAX as u128);
		assert_eq!(deserialize("\"0x10000000000000000\""), u64::MAX as u128 + 1);
	}

	#[test]
	fn ensure_format_is_unchanged() {
		let header = Header::<u32, BlakeTwo256> {
			parent_hash: BlakeTwo256::hash(b"1"),
			number: 2,
			state_root: BlakeTwo256::hash(b"3"),
			extrinsics_root: BlakeTwo256::hash(b"4"),
			zk_tree_root: Default::default(),
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"6".to_vec())] },
			_marker: core::marker::PhantomData,
		};

		let header_encoded = header.encode();
		let header_decoded = Header::<u32, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap();
		assert_eq!(header_decoded, header);

		let header = Header::<u32, BlakeTwo256> {
			parent_hash: BlakeTwo256::hash(b"1000"),
			number: 2000,
			state_root: BlakeTwo256::hash(b"3000"),
			extrinsics_root: BlakeTwo256::hash(b"4000"),
			zk_tree_root: Default::default(),
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"5000".to_vec())] },
			_marker: core::marker::PhantomData,
		};

		let header_encoded = header.encode();
		let header_decoded = Header::<u32, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap();
		assert_eq!(header_decoded, header);
	}

	#[test]
	fn poseidon_header_hash_is_stable() {
		// Example header from a real block on devnet
		let parent_hash = "839b2d2ac0bf4aa71b18ad1ba5e2880b4ef06452cefacd255cfd76f6ad2c7966";
		let number = 4;
		let state_root = "1688817041c572d6c971681465f401f06d0fdcfaed61d28c06d42dc2d07816d5";
		let extrinsics_root = "7c6cace2e91b6314e05410b91224c11f5dd4a4a2dbf0e39081fddbe4ac9ad252";
		let digest = Digest {
			logs: vec![
				DigestItem::PreRuntime(
					[112, 111, 119, 95],
					[
						233, 182, 183, 107, 158, 1, 115, 19, 219, 126, 253, 86, 30, 208, 176, 70,
						21, 45, 180, 229, 9, 62, 91, 4, 6, 53, 245, 52, 48, 38, 123, 225,
					]
					.to_vec(),
				),
				DigestItem::Seal(
					[112, 111, 119, 95],
					[
						0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
						0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
						0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30, 77, 142,
					]
					.to_vec(),
				),
			],
		};
		let header = Header::<u32, BlakeTwo256> {
			parent_hash: H256::from_slice(
				hex::decode(parent_hash).expect("valid hex parent hash").as_slice(),
			),
			number,
			state_root: H256::from_slice(
				hex::decode(state_root).expect("valid hex state root").as_slice(),
			),
			extrinsics_root: H256::from_slice(
				hex::decode(extrinsics_root).expect("valid hex extrinsics root").as_slice(),
			),
			zk_tree_root: Default::default(),
			digest,
			_marker: core::marker::PhantomData,
		};

		// Known good hash for this header - ensures hash algorithm stability
		let expected_hash = "b7dbfd398bcef7d1c91821eb105c52e87dab78e822779bfd0dc7ab132d659a55";
		let actual_hash: [u8; 32] = header.hash().into();

		assert_eq!(
			hex::encode(actual_hash),
			expected_hash,
			"Header hash changed! This would break consensus."
		);
	}

	/// The canonical post-seal digest (one 32-byte pre-runtime preimage plus a
	/// 64-byte PoW seal) must encode to exactly `DIGEST_LOGS_SIZE`: the hash
	/// window is sized with no slack, so anything larger overflows it and must
	/// be rejected rather than truncated.
	fn canonical_pow_digest() -> Digest {
		Digest {
			logs: vec![
				DigestItem::PreRuntime([112, 111, 119, 95], vec![0u8; 32]),
				DigestItem::Seal([112, 111, 119, 95], vec![0u8; 64]),
			],
		}
	}

	#[test]
	fn canonical_pow_digest_fills_hash_window_exactly() {
		assert_eq!(
			canonical_pow_digest().encode().len(),
			DIGEST_LOGS_SIZE,
			"a well-formed pre-runtime + seal digest must fill the window exactly"
		);
	}

	fn header_with_digest(digest: Digest) -> Header<u32, BlakeTwo256> {
		Header::<u32, BlakeTwo256> {
			parent_hash: Default::default(),
			number: 1,
			state_root: Default::default(),
			extrinsics_root: Default::default(),
			zk_tree_root: Default::default(),
			digest,
			_marker: core::marker::PhantomData,
		}
	}

	/// `hash()` must accept an oversized digest without panicking, even in
	/// debug builds: generic network code (e.g. block-announce validation in
	/// `sc-network-sync`) hashes completely unverified headers before any
	/// consensus check runs, so any assertion here would be a remotely
	/// triggerable node crash. This test also documents the consequence of
	/// that infallibility — bytes past the window are not committed, so two
	/// headers differing only there collide — which is exactly why the import
	/// path in `sc-consensus-qpow` rejects oversized digests outright.
	#[test]
	fn oversized_digest_hashes_infallibly_but_is_not_committed_past_window() {
		// The canonical digest fills the window exactly, so a third item's
		// payload lands entirely past `DIGEST_LOGS_SIZE` (the extra item does
		// not change the compact length prefix: 3 < 64 still encodes in one
		// byte, so the first two items' bytes keep their offsets).
		let mut digest_a = canonical_pow_digest();
		digest_a.logs.push(DigestItem::Other(vec![0x00u8; 8]));
		let mut digest_b = canonical_pow_digest();
		digest_b.logs.push(DigestItem::Other(vec![0xffu8; 8]));

		assert!(digest_a.encode().len() > DIGEST_LOGS_SIZE);
		assert_eq!(digest_a.encode().len(), digest_b.encode().len());

		// Infallible: no panic on either. Colliding: the differing tails are
		// past the committed window.
		let hash_a = header_with_digest(digest_a).hash();
		let hash_b = header_with_digest(digest_b).hash();
		assert_eq!(
			hash_a, hash_b,
			"digest bytes past the window are not committed by the hash; such headers \
			 must be rejected at import instead"
		);

		// Sanity: a within-window digest change does alter the hash.
		let hash_canonical = header_with_digest(canonical_pow_digest()).hash();
		assert_ne!(hash_a, hash_canonical);
	}

	/// Historical runtime-upgrade blocks deposited `RuntimeEnvironmentUpdated`
	/// between the pre-runtime item and the seal, encoding to 111 bytes. Those
	/// headers are already canonical and must pass the import window check.
	#[test]
	fn runtime_environment_updated_upgrade_digest_is_grandfathered() {
		let mut digest = canonical_pow_digest();
		digest.logs.insert(1, DigestItem::RuntimeEnvironmentUpdated);
		assert_eq!(digest.encode().len(), DIGEST_LOGS_SIZE + 1);
		assert_eq!(check_digest_commitment_window(&digest, 1), Ok(()));
	}

	#[test]
	fn runtime_environment_updated_is_rejected_above_legacy_cutoff() {
		let mut digest = canonical_pow_digest();
		digest.logs.insert(1, DigestItem::RuntimeEnvironmentUpdated);
		assert_eq!(
			check_digest_commitment_window(&digest, LEGACY_DIGEST_CUTOFF + 1),
			Err(digest.encode().len())
		);
	}

	#[test]
	fn arbitrary_overflow_is_still_rejected() {
		let mut digest = canonical_pow_digest();
		digest.logs.insert(1, DigestItem::Other(vec![0u8; 64]));
		assert!(digest.encode().len() > DIGEST_LOGS_SIZE);
		assert_eq!(check_digest_commitment_window(&digest, 1), Err(digest.encode().len()));
	}

	#[test]
	fn two_runtime_environment_updated_items_are_rejected() {
		let mut digest = canonical_pow_digest();
		digest.logs.insert(1, DigestItem::RuntimeEnvironmentUpdated);
		digest.logs.insert(1, DigestItem::RuntimeEnvironmentUpdated);
		assert!(digest.encode().len() > DIGEST_LOGS_SIZE + 1);
		assert_eq!(check_digest_commitment_window(&digest, 1), Err(digest.encode().len()));
	}

	#[test]
	fn canonical_digest_passes_window_check() {
		assert_eq!(check_digest_commitment_window(&canonical_pow_digest(), 1), Ok(()));
	}

	#[test]
	fn undersized_digest_is_rejected() {
		let digest = Digest::default();
		assert!(digest.encode().len() < DIGEST_LOGS_SIZE);
		assert_eq!(check_digest_commitment_window(&digest, 1), Err(digest.encode().len()));
	}
}
