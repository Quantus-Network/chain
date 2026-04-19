//! Fork of sp-runtime's generic implementation of a block header.
//!
//! Key differences from the standard Substrate header:
//! - **Block hash**: Computed with Poseidon (via the `Hash` type parameter) for ZK circuit
//!   compatibility
//! - **State trie**: Uses Blake2 (hardcoded as `Hashing` type) for efficient native execution
//!
//! This means `HashingFor<Block>` returns `BlakeTwo256`, which is used for:
//! - State trie merkle root computation
//! - Extrinsics root computation
//! - Storage proof verification

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Codec, Decode, DecodeWithMemTracking, Encode};
use p3_field::integers::QuotientMap;
use p3_goldilocks::Goldilocks;
use qp_poseidon_core::{
	hash_to_bytes,
	serialization::{bytes_to_digest, bytes_to_felts},
};
use scale_info::TypeInfo;
use sp_core::U256;
use sp_runtime::{
	generic::Digest,
	traits::{AtLeast32BitUnsigned, BlockNumber, Hash as HashT, MaybeDisplay, Member},
	RuntimeDebug,
};
extern crate alloc;

use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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

/// Custom block header with separate hashers for block hash and state trie.
///
/// - `Hash`: Used for block hash computation (Poseidon for ZK compatibility)
/// - `StateHash`: Used for state trie / extrinsics root via `Header::Hashing` trait
///
/// ## Field Ordering
///
/// The `zk_tree_root` field is intentionally placed **before** `digest` to ensure
/// a fixed offset in the header preimage. This prevents miners from manipulating
/// the digest to shift the ZK root's position in the felt encoding.
#[derive(Encode, Decode, PartialEq, Eq, Clone, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
#[scale_info(skip_type_params(Hash, StateHash))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
#[cfg_attr(
	feature = "serde",
	serde(bound = "Hash::Output: Serialize + serde::de::DeserializeOwned")
)]
pub struct Header<Number, Hash: HashT, StateHash: HashT>
where
	Number: Copy + Into<U256> + TryFrom<U256>,
{
	pub parent_hash: Hash::Output,
	#[cfg_attr(
		feature = "serde",
		serde(serialize_with = "serialize_number", deserialize_with = "deserialize_number")
	)]
	pub number: Number,
	pub state_root: Hash::Output,
	pub extrinsics_root: Hash::Output,
	/// Root of the ZK Merkle tree (4-ary Poseidon tree).
	///
	/// This is placed before `digest` to ensure a fixed offset in the header
	/// preimage for ZK circuit verification.
	pub zk_tree_root: Hash::Output,
	pub digest: Digest,
	#[codec(skip)]
	#[cfg_attr(feature = "serde", serde(skip))]
	_marker: core::marker::PhantomData<StateHash>,
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

impl<Number, Hash, StateHash> sp_runtime::traits::Header for Header<Number, Hash, StateHash>
where
	Number: BlockNumber,
	Hash: HashT<Output = sp_core::H256>,
	StateHash: HashT<Output = sp_core::H256>,
{
	type Number = Number;
	type Hash = sp_core::H256;
	/// State trie hasher - configurable, defaults to BlakeTwo256.
	type Hashing = StateHash;

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
			zk_tree_root: sp_core::H256::zero(),
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

impl<Number, Hash, StateHash> Header<Number, Hash, StateHash>
where
	Number: Member
		+ core::hash::Hash
		+ Copy
		+ MaybeDisplay
		+ AtLeast32BitUnsigned
		+ Codec
		+ Into<U256>
		+ TryFrom<U256>,
	Hash: HashT,
	Hash::Output: From<[u8; 32]>,
	StateHash: HashT,
{
	/// Convenience helper for computing the hash of the header without having
	/// to import the trait.
	pub fn hash(&self) -> Hash::Output {
		/// Fixed size for digest encoding - must match circuit expectation
		const DIGEST_LOGS_SIZE: usize = 110;

		// 4 hash fields (4 felts each) + 1 u32 + 28 felts for injective digest encoding
		let max_encoded_felts = 4 * 4 + 1 + 28;
		let mut felts = Vec::with_capacity(max_encoded_felts);

		// parent_hash : 32 bytes → 4 felts (8 bytes/felt for hash outputs)
		felts.extend(bytes_to_digest::<Goldilocks>(
			self.parent_hash.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// block number as u64 (compact encoded, but we only need the value)
		// constrain the block number to be with u32 range for simplicity
		let number = self.number.into();
		felts.push(Goldilocks::from_int(number.as_u32() as u64));

		// state_root : 32 bytes → 4 felts (8 bytes/felt for hash outputs)
		felts.extend(bytes_to_digest::<Goldilocks>(
			self.state_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// extrinsics_root : 32 bytes → 4 felts (8 bytes/felt for hash outputs)
		felts.extend(bytes_to_digest::<Goldilocks>(
			self.extrinsics_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// zk_tree_root : 32 bytes → 4 felts (8 bytes/felt for hash outputs)
		// Placed before digest to ensure fixed offset regardless of digest content
		felts.extend(bytes_to_digest::<Goldilocks>(
			self.zk_tree_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// digest – SCALE encode then pad to fixed 110 bytes to match circuit expectation
		let digest_encoded = self.digest.encode();
		let mut digest_padded = [0u8; DIGEST_LOGS_SIZE];
		let copy_len = digest_encoded.len().min(DIGEST_LOGS_SIZE);
		digest_padded[..copy_len].copy_from_slice(&digest_encoded[..copy_len]);

		// injective encoding (4 bytes/felt + terminator)
		felts.extend(bytes_to_felts(&digest_padded));

		let poseidon_hash: [u8; 32] = hash_to_bytes(&felts);
		poseidon_hash.into()
	}

	/// Create a new header with all fields including zk_tree_root.
	///
	/// This is the preferred constructor when you have the ZK tree root available.
	/// Use this instead of `Header::new` + `set_zk_tree_root`.
	pub fn new_with_zk_root(
		number: Number,
		extrinsics_root: Hash::Output,
		state_root: Hash::Output,
		parent_hash: Hash::Output,
		zk_tree_root: Hash::Output,
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
	pub fn zk_tree_root(&self) -> &Hash::Output {
		&self.zk_tree_root
	}

	/// Set the ZK tree root.
	///
	/// Called by pallet-zk-tree during block finalization.
	pub fn set_zk_tree_root(&mut self, root: Hash::Output) {
		self.zk_tree_root = root;
	}
}

impl<Number, Hash, StateHash> ZkTreeRootProvider for Header<Number, Hash, StateHash>
where
	Number: Copy + Into<U256> + TryFrom<U256>,
	Hash: HashT,
	StateHash: HashT,
{
	type Hash = Hash::Output;

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
	use qp_poseidon::PoseidonHasher;
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
		let header = Header::<u32, BlakeTwo256, BlakeTwo256> {
			parent_hash: BlakeTwo256::hash(b"1"),
			number: 2,
			state_root: BlakeTwo256::hash(b"3"),
			extrinsics_root: BlakeTwo256::hash(b"4"),
			zk_tree_root: Default::default(),
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"6".to_vec())] },
			_marker: core::marker::PhantomData,
		};

		let header_encoded = header.encode();
		let header_decoded =
			Header::<u32, BlakeTwo256, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap();
		assert_eq!(header_decoded, header);

		let header = Header::<u32, BlakeTwo256, BlakeTwo256> {
			parent_hash: BlakeTwo256::hash(b"1000"),
			number: 2000,
			state_root: BlakeTwo256::hash(b"3000"),
			extrinsics_root: BlakeTwo256::hash(b"4000"),
			zk_tree_root: Default::default(),
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"5000".to_vec())] },
			_marker: core::marker::PhantomData,
		};

		let header_encoded = header.encode();
		let header_decoded =
			Header::<u32, BlakeTwo256, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap();
		assert_eq!(header_decoded, header);
	}

	fn hash_header(x: &[u8]) -> [u8; 32] {
		let mut y = x;
		if let Ok(header) = Header::<u32, PoseidonHasher, BlakeTwo256>::decode(&mut y) {
			if y.is_empty() {
				const DIGEST_LOGS_SIZE: usize = 110;
				let max_encoded_felts = 4 * 4 + 1 + 28;
				let mut felts = Vec::with_capacity(max_encoded_felts);

				felts.extend(bytes_to_digest::<Goldilocks>(
					header.parent_hash.as_bytes().try_into().unwrap(),
				));
				felts.push(Goldilocks::from_int(header.number as u64));
				felts.extend(bytes_to_digest::<Goldilocks>(
					header.state_root.as_bytes().try_into().unwrap(),
				));
				felts.extend(bytes_to_digest::<Goldilocks>(
					header.extrinsics_root.as_bytes().try_into().unwrap(),
				));
				felts.extend(bytes_to_digest::<Goldilocks>(
					header.zk_tree_root.as_bytes().try_into().unwrap(),
				));

				let digest_encoded = header.digest.encode();
				let mut digest_padded = [0u8; DIGEST_LOGS_SIZE];
				let copy_len = digest_encoded.len().min(DIGEST_LOGS_SIZE);
				digest_padded[..copy_len].copy_from_slice(&digest_encoded[..copy_len]);
				felts.extend(bytes_to_felts(&digest_padded));

				return hash_to_bytes(&felts);
			}
		}
		PoseidonHasher::hash_for_circuit(x)
	}

	#[test]
	fn poseidon_header_hash_matches_old_path() {
		use codec::Encode;

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
		let header = Header::<u32, PoseidonHasher, BlakeTwo256> {
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

		let encoded = header.encode();

		let old = hash_header(&encoded); // old path
		let new: [u8; 32] = header.hash().into();
		println!("Old hash: 0x{}", hex::encode(old));

		assert_eq!(old, new);
	}
}
