// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use alloc::{collections::btree_set::BTreeSet, vec::Vec};
use codec::{Decode, DecodeWithMemTracking, Encode};
use core::iter::{DoubleEndedIterator, IntoIterator};
use hash_db::{HashDB, Hasher};
use scale_info::TypeInfo;

/// Error associated with the `storage_proof` module.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug, TypeInfo)]
pub enum StorageProofError {
	/// The proof contains duplicate nodes.
	DuplicateNodes,
}

/// A proof that some set of key-value pairs are included in the storage trie. The proof contains
/// the storage values so that the partial storage backend can be reconstructed by a verifier that
/// does not already have access to the key-value pairs.
///
/// The proof consists of the set of serialized nodes in the storage trie accessed when looking up
/// the keys covered by the proof. Verifying the proof requires constructing the partial trie from
/// the serialized nodes and performing the key lookups.
#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo, DecodeWithMemTracking)]
pub struct StorageProof {
	trie_nodes: BTreeSet<Vec<u8>>,
}

impl StorageProof {
	/// Constructs a storage proof from a subset of encoded trie nodes in a storage backend.
	pub fn new(trie_nodes: impl IntoIterator<Item = Vec<u8>>) -> Self {
		StorageProof { trie_nodes: BTreeSet::from_iter(trie_nodes) }
	}

	/// Constructs a storage proof from a subset of encoded trie nodes in a storage backend.
	///
	/// Returns an error if the provided subset of encoded trie nodes contains duplicates.
	pub fn new_with_duplicate_nodes_check(
		trie_nodes: impl IntoIterator<Item = Vec<u8>>,
	) -> Result<Self, StorageProofError> {
		let mut trie_nodes_set = BTreeSet::new();
		for node in trie_nodes {
			if !trie_nodes_set.insert(node) {
				return Err(StorageProofError::DuplicateNodes);
			}
		}

		Ok(StorageProof { trie_nodes: trie_nodes_set })
	}

	/// Returns a new empty proof.
	///
	/// An empty proof is capable of only proving trivial statements (ie. that an empty set of
	/// key-value pairs exist in storage).
	pub fn empty() -> Self {
		StorageProof { trie_nodes: BTreeSet::new() }
	}

	/// Returns whether this is an empty proof.
	pub fn is_empty(&self) -> bool {
		self.trie_nodes.is_empty()
	}

	/// Returns the number of nodes in the proof.
	pub fn len(&self) -> usize {
		self.trie_nodes.len()
	}

	/// Convert into an iterator over encoded trie nodes in lexicographical order constructed
	/// from the proof.
	pub fn into_iter_nodes(self) -> impl Sized + DoubleEndedIterator<Item = Vec<u8>> {
		self.trie_nodes.into_iter()
	}

	/// Create an iterator over encoded trie nodes in lexicographical order constructed
	/// from the proof.
	pub fn iter_nodes(&self) -> impl Sized + DoubleEndedIterator<Item = &Vec<u8>> {
		self.trie_nodes.iter()
	}

	/// Convert into plain node vector.
	pub fn into_nodes(self) -> BTreeSet<Vec<u8>> {
		self.trie_nodes
	}

	/// Creates a [`MemoryDB`](crate::MemoryDB) from `Self`.
	pub fn into_memory_db<H: Hasher>(self) -> crate::MemoryDB<H> {
		self.into()
	}

	/// Creates a [`MemoryDB`](crate::MemoryDB) from `Self` reference.
	pub fn to_memory_db<H: Hasher>(&self) -> crate::MemoryDB<H> {
		self.into()
	}

	/// Merges multiple storage proofs covering potentially different sets of keys into one proof
	/// covering all keys. The merged proof output may be smaller than the aggregate size of the
	/// input proofs due to deduplication of trie nodes.
	pub fn merge(proofs: impl IntoIterator<Item = Self>) -> Self {
		let trie_nodes = proofs
			.into_iter()
			.flat_map(|proof| proof.into_iter_nodes())
			.collect::<BTreeSet<_>>()
			.into_iter()
			.collect();

		Self { trie_nodes }
	}

	/// Returns the encoded size of the proof.
	pub fn encoded_size(&self) -> usize {
		self.trie_nodes.iter().map(|n| n.len()).sum()
	}

	/// Returns the encoded size as a compact proof.
	///
	/// **DEPRECATED**: Returns `None` since Quantus doesn't support compact proofs.
	pub fn encoded_compact_size<H: Hasher>(self, _root: H::Out) -> Option<usize> {
		// Compact proofs are not supported in Quantus - return None
		None
	}

	/// Convert into a compact proof.
	///
	/// **DEPRECATED**: Quantus uses ZK proofs. This returns a stub CompactProof
	/// that wraps the storage proof nodes but is not actually compact-encoded.
	pub fn into_compact_proof<H: Hasher>(
		self,
		_root: H::Out,
	) -> Result<CompactProof, CompactProofError<H::Out, crate::Error<H::Out>>> {
		Ok(CompactProof::from_storage_proof(self))
	}

	/// Convert to a compact proof.
	///
	/// **DEPRECATED**: Quantus uses ZK proofs. This returns a stub CompactProof
	/// that wraps the storage proof nodes but is not actually compact-encoded.
	pub fn to_compact_proof<H: Hasher>(
		&self,
		_root: H::Out,
	) -> Result<CompactProof, CompactProofError<H::Out, crate::Error<H::Out>>> {
		Ok(CompactProof::from_storage_proof(self.clone()))
	}
}

impl<H: Hasher> From<StorageProof> for crate::MemoryDB<H> {
	fn from(proof: StorageProof) -> Self {
		From::from(&proof)
	}
}

impl<H: Hasher> From<&StorageProof> for crate::MemoryDB<H> {
	fn from(proof: &StorageProof) -> Self {
		let mut db = crate::MemoryDB::new(&0u64.to_le_bytes());
		proof.iter_nodes().for_each(|n| {
			db.insert(crate::EMPTY_PREFIX, n);
			let value_key = crate::injective_value_hash::<H>(n);
			hash_db::HashDB::emplace(&mut db, value_key, crate::EMPTY_PREFIX, n.to_vec());
		});
		db
	}
}

/// Error for compact proof operations.
///
/// **DEPRECATED**: Quantus uses ZK proofs instead of compact Merkle proofs.
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum CompactProofError<H, CodecError> {
	/// Root mismatch.
	#[cfg_attr(feature = "std", error("Invalid root {0:x?}, expected {1:x?}"))]
	RootMismatch(H, H),
	/// Incomplete proof.
	#[cfg_attr(feature = "std", error("Missing nodes in the proof"))]
	IncompleteProof,
	/// Trie error.
	#[cfg_attr(feature = "std", error("Trie error: {0:?}"))]
	TrieError(alloc::boxed::Box<trie_db::TrieError<H, CodecError>>),
}

/// Compact proof stub for API compatibility.
///
/// **DEPRECATED**: Quantus uses ZK proofs instead of compact Merkle proofs.
/// This type exists only for API compatibility with external crates.
/// It should not be used for actual proof operations - all methods will panic.
#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct CompactProof {
	/// Raw encoded nodes (not actually compact-encoded).
	pub encoded_nodes: Vec<Vec<u8>>,
}

impl CompactProof {
	/// Create from a StorageProof.
	///
	/// **DEPRECATED**: This creates a stub that stores raw nodes, not compact-encoded.
	pub(crate) fn from_storage_proof(storage_proof: StorageProof) -> Self {
		Self { encoded_nodes: storage_proof.into_iter_nodes().collect() }
	}

	/// Return an iterator on the encoded nodes.
	pub fn iter_compact_encoded_nodes(&self) -> impl Iterator<Item = &[u8]> {
		self.encoded_nodes.iter().map(Vec::as_slice)
	}

	/// Returns the encoded size of the compact proof.
	pub fn encoded_size(&self) -> usize {
		self.encoded_nodes.iter().map(|n| n.len()).sum()
	}

	/// Decode to a storage proof and memory DB.
	///
	/// **DEPRECATED**: This is a stub that panics. Quantus uses ZK proofs.
	pub fn to_memory_db<H: Hasher>(
		&self,
		_expected_root: Option<&H::Out>,
	) -> Result<(crate::MemoryDB<H>, H::Out), CompactProofError<H::Out, crate::Error<H::Out>>> {
		panic!("CompactProof::to_memory_db is not supported - Quantus uses ZK proofs instead of compact Merkle proofs")
	}

	/// Encode from storage proof.
	///
	/// **DEPRECATED**: This is a stub that panics. Quantus uses ZK proofs.
	pub fn to_storage_proof<H: Hasher>(
		&self,
		_expected_root: Option<&H::Out>,
	) -> Result<(StorageProof, H::Out), CompactProofError<H::Out, crate::Error<H::Out>>> {
		panic!("CompactProof::to_storage_proof is not supported - Quantus uses ZK proofs instead of compact Merkle proofs")
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::{tests::create_storage_proof, StorageProof};

	type Layout = crate::LayoutV1<sp_core::Blake2Hasher>;

	const TEST_DATA: &[(&[u8], &[u8])] =
		&[(b"key1", &[1; 64]), (b"key2", &[2; 64]), (b"key3", &[3; 64]), (b"key11", &[4; 64])];

	#[test]
	fn proof_with_duplicate_nodes_is_rejected() {
		let (raw_proof, _root) = create_storage_proof::<Layout>(TEST_DATA);
		assert!(matches!(
			StorageProof::new_with_duplicate_nodes_check(raw_proof),
			Err(StorageProofError::DuplicateNodes)
		));
	}
}
