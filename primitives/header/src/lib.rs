//! Fork of sp-runtime's generic implementation of a block header.
//! We override the hashing function to ensure a felt aligned pre-image for the block hash.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, DecodeWithMemTracking, Encode, HasCompact};
use p3_field::integers::QuotientMap;
use p3_goldilocks::Goldilocks;
use qp_poseidon_core::{
	hash_variable_length,
	serialization::{injective_bytes_to_felts, unsafe_digest_bytes_to_felts},
};
use scale_info::TypeInfo;
use sp_core::U256;
use sp_runtime::{
	generic::Digest,
	traits::{AtLeast32BitUnsigned, BlockNumber, Hash as HashT, MaybeDisplay, Member},
	RuntimeDebug,
};
use sp_std::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Custom block header that hashes itself with Poseidon over Goldilocks field elements.
#[derive(Encode, Decode, PartialEq, Eq, Clone, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
#[scale_info(skip_type_params(Hash))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Header<Number: Copy + Into<U256> + TryFrom<U256>, Hash: HashT> {
	pub parent_hash: Hash::Output,
	#[cfg_attr(
		feature = "serde",
		serde(serialize_with = "serialize_number", deserialize_with = "deserialize_number")
	)]
	pub number: Number,
	pub state_root: Hash::Output,
	pub extrinsics_root: Hash::Output,
	pub digest: Digest,
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

impl<Number, Hash> sp_runtime::traits::Header for Header<Number, Hash>
where
	Number: BlockNumber,
	Hash: HashT,
	Hash::Output: From<[u8; 32]>,
{
	type Number = Number;
	type Hash = <Hash as HashT>::Output;
	type Hashing = Hash;

	fn new(
		number: Self::Number,
		extrinsics_root: Self::Hash,
		state_root: Self::Hash,
		parent_hash: Self::Hash,
		digest: Digest,
	) -> Self {
		Self { number, extrinsics_root, state_root, parent_hash, digest }
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
		let max_encoded_felts = 4 * 3 + 1 + 28; // 3 hashout fields + 1 u32 + 28 felts for injective digest encoding
		let mut felts = Vec::with_capacity(max_encoded_felts);

		// parent_hash : 32 bytes → 4 felts
		felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
			self.parent_hash.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// block number as u64 (compact encoded, but we only need the value)
		// constrain the block number to be with u32 range for simplicity
		let number = self.number.into();
		felts.push(Goldilocks::from_int(number.as_u32() as u64));

		// state_root : 32 bytes → 4 felts
		felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
			self.state_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// extrinsics_root : 32 bytes → 4 felts
		felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
			self.extrinsics_root.as_ref().try_into().expect("hash is 32 bytes"),
		));

		// digest – injective encoding
		felts.extend(injective_bytes_to_felts::<Goldilocks>(&self.digest.encode()));

		let poseidon_hash: [u8; 32] = hash_variable_length(felts);
		poseidon_hash.into()
	}
}

#[cfg(all(test, feature = "std"))]
mod tests {
	use super::*;
	use qp_poseidon::PoseidonHasher;
	use sp_core::H256;
	use sp_runtime::{
		traits::{BlakeTwo256, Header as HeaderT},
		DigestItem,
	};

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
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"6".to_vec())] },
		};

		let header_encoded = header.encode();
		assert_eq!(
			header_encoded,
			vec![
				146, 205, 245, 120, 196, 112, 133, 165, 153, 34, 86, 240, 220, 249, 125, 11, 25,
				241, 241, 201, 222, 77, 95, 227, 12, 58, 206, 97, 145, 182, 229, 219, 8, 88, 19,
				72, 51, 123, 15, 62, 20, 134, 32, 23, 61, 170, 165, 249, 77, 0, 216, 129, 112, 93,
				203, 240, 170, 131, 239, 218, 186, 97, 210, 237, 225, 235, 134, 73, 33, 73, 151,
				87, 78, 32, 196, 100, 56, 138, 23, 36, 32, 210, 84, 3, 104, 43, 187, 184, 12, 73,
				104, 49, 200, 204, 31, 143, 13, 4, 0, 4, 54
			],
		);
		assert_eq!(Header::<u32, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap(), header);

		let header = Header::<u32, BlakeTwo256> {
			parent_hash: BlakeTwo256::hash(b"1000"),
			number: 2000,
			state_root: BlakeTwo256::hash(b"3000"),
			extrinsics_root: BlakeTwo256::hash(b"4000"),
			digest: Digest { logs: vec![sp_runtime::generic::DigestItem::Other(b"5000".to_vec())] },
		};

		let header_encoded = header.encode();
		assert_eq!(
			header_encoded,
			vec![
				197, 243, 254, 225, 31, 117, 21, 218, 179, 213, 92, 6, 247, 164, 230, 25, 47, 166,
				140, 117, 142, 159, 195, 202, 67, 196, 238, 26, 44, 18, 33, 92, 65, 31, 219, 225,
				47, 12, 107, 88, 153, 146, 55, 21, 226, 186, 110, 48, 167, 187, 67, 183, 228, 232,
				118, 136, 30, 254, 11, 87, 48, 112, 7, 97, 31, 82, 146, 110, 96, 87, 152, 68, 98,
				162, 227, 222, 78, 14, 244, 194, 120, 154, 112, 97, 222, 144, 174, 101, 220, 44,
				111, 126, 54, 34, 155, 220, 253, 124, 4, 0, 16, 53, 48, 48, 48
			],
		);
		assert_eq!(Header::<u32, BlakeTwo256>::decode(&mut &header_encoded[..]).unwrap(), header);
	}

	fn hash_header(x: &[u8]) -> [u8; 32] {
		let mut y = x;
		if let Ok(header) = Header::<u32, PoseidonHasher>::decode(&mut y) {
			// Only treat this as a header if we consumed the entire input.
			if y.is_empty() {
				let max_encoded_felts = 4 * 3 + 1 + 28; // 3 hashout fields + 1 u32 + 28 felts
				let mut felts = Vec::with_capacity(max_encoded_felts);

				let parent_hash = header.parent_hash.as_bytes();
				let number = header.number;
				let state_root = header.state_root.as_bytes();
				let extrinsics_root = header.extrinsics_root.as_bytes();
				let digest = header.digest.encode();

				felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
					parent_hash.try_into().expect("Parent hash expected to equal 32 bytes"),
				));
				felts.push(Goldilocks::from_int(number as u64));
				felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
					state_root.try_into().expect("State root expected to equal 32 bytes"),
				));
				felts.extend(unsafe_digest_bytes_to_felts::<Goldilocks>(
					extrinsics_root.try_into().expect("Extrinsics root expected to equal 32 bytes"),
				));
				felts.extend(injective_bytes_to_felts::<Goldilocks>(&digest));

				return hash_variable_length(felts);
			}
		}
		// Fallback: canonical bytes hashing for non-header data
		PoseidonHasher::hash_padded(x)
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
		let header = Header::<u32, PoseidonHasher> {
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
			digest,
		};

		let encoded = header.encode();

		let old = hash_header(&encoded); // old path
		let new: [u8; 32] = header.hash().into();
		println!("Old hash: 0x{}", hex::encode(old));

		assert_eq!(old, new);
	}
}
