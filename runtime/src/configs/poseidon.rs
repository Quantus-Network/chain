use alloc::vec::Vec;
use codec::{Decode, Encode};
use core::hash::Hasher as StdHasher;
use frame_support::__private::serde::{Deserializer, Serializer};
use frame_support::{Deserialize, Serialize};
use scale_info::TypeInfo;
use sp_core::{Hasher, H256};
use sp_runtime::traits::{Hash};
use sp_storage::StateVersion;
use sp_trie::{LayoutV0, LayoutV1, TrieConfiguration};

/// Newtype wrapper around qp_poseidon::PoseidonHasher to implement required traits locally
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
pub struct PoseidonHasher;

#[derive(Default)]
pub struct PoseidonStdHasher(Vec<u8>);

impl StdHasher for PoseidonStdHasher {
	fn finish(&self) -> u64 {
		let hash = qp_poseidon::PoseidonHasher::hash_padded(self.0.as_slice());
		u64::from_le_bytes(hash[0..8].try_into().unwrap())
	}

	fn write(&mut self, bytes: &[u8]) {
		self.0.extend_from_slice(bytes)
	}
}

impl Hasher for PoseidonHasher {
	type Out = H256;
	type StdHasher = PoseidonStdHasher;
	const LENGTH: usize = 32;

	fn hash(x: &[u8]) -> H256 {
		H256::from_slice(&qp_poseidon::PoseidonHasher::hash_padded(x))
	}
}

impl<'de> Deserialize<'de> for PoseidonHasher {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		// For a unit struct, just ignore the deserializer and return the unit value
		let _ = deserializer;
		Ok(PoseidonHasher)
	}
}

impl Serialize for PoseidonHasher {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		// For a unit struct, serialize as unit
		serializer.serialize_unit()
	}
}

impl Hash for PoseidonHasher {
	type Output = H256;

	fn hash(s: &[u8]) -> Self::Output {
		H256::from_slice(&qp_poseidon::PoseidonHasher::hash_padded(s))
	}

	/// Produce the hash of some codec-encodable value.
	fn hash_of<S: Encode>(s: &S) -> Self::Output {
		Encode::using_encoded(s, <Self as Hasher>::hash)
	}

	fn ordered_trie_root(input: Vec<Vec<u8>>, state_version: StateVersion) -> Self::Output {
		log::debug!(target: "poseidon",
			"PoseidonHasher::ordered_trie_root input={input:?} version={state_version:?}",
		);
		let res = match state_version {
			StateVersion::V0 => LayoutV0::<PoseidonHasher>::ordered_trie_root(input),
			StateVersion::V1 => LayoutV1::<PoseidonHasher>::ordered_trie_root(input),
		};
		log::debug!(target: "poseidon", "PoseidonHasher::ordered_trie_root res={res:?}");
		res
	}

	fn trie_root(input: Vec<(Vec<u8>, Vec<u8>)>, version: StateVersion) -> Self::Output {
		log::debug!(target: "poseidon",
			"PoseidonHasher::trie_root input={input:?} version={version:?}"
		);
		let res = match version {
			StateVersion::V0 => LayoutV0::<PoseidonHasher>::trie_root(input),
			StateVersion::V1 => LayoutV1::<PoseidonHasher>::trie_root(input),
		};
		log::debug!(target: "poseidon", "PoseidonHasher::trie_root res={res:?}");
		res
	}
}
