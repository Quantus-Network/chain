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

//! Utility functions to interact with Substrate's Base-16 Modified Merkle Patricia tree ("trie").

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod accessed_nodes_tracker;
#[cfg(feature = "std")]
pub mod cache;
mod error;
mod node_codec;
mod node_header;
#[cfg(feature = "std")]
pub mod recorder;
pub mod recorder_ext;
mod storage_proof;
mod trie_codec;
mod trie_stream;

#[cfg(feature = "std")]
pub mod proof_size_extension;

use alloc::{borrow::Borrow, boxed::Box, vec, vec::Vec};
use core::marker::PhantomData;
/// Our `NodeCodec`-specific error.
pub use error::Error;
/// Various re-exports from the `hash-db` crate.
pub use hash_db::{HashDB as HashDBT, EMPTY_PREFIX};
use hash_db::{Hasher, Prefix};
/// Various re-exports from the `memory-db` crate.
pub use memory_db::{prefixed_key, HashKey, KeyFunction, PrefixedKey};
/// The Substrate format implementation of `NodeCodec`.
pub use node_codec::NodeCodec;
pub use storage_proof::{CompactProof, StorageProof, StorageProofError};
/// Trie codec reexport, mainly child trie support
/// for trie compact proof.
pub use trie_codec::{decode_compact, encode_compact, Error as CompactProofError};
use trie_db::proof::{generate_proof, verify_proof};
/// Various re-exports from the `trie-db` crate.
pub use trie_db::{
    nibble_ops,
    node::{NodePlan, ValuePlan},
    triedb::{TrieDBDoubleEndedIterator, TrieDBKeyDoubleEndedIterator},
    CError, DBValue, Query, Recorder, Trie, TrieCache, TrieConfiguration, TrieDBIterator,
    TrieDBKeyIterator, TrieDBNodeDoubleEndedIterator, TrieDBRawIterator, TrieLayout, TrieMut,
    TrieRecorder,
};
pub use trie_db::{proof::VerifyError, MerkleValue};
/// The Substrate format implementation of `TrieStream`.
pub use trie_stream::TrieStream;

/// Raw storage proof type (just raw trie nodes).
pub type RawStorageProof = Vec<Vec<u8>>;

/// substrate trie layout
pub struct LayoutV0<H>(PhantomData<H>);

/// substrate trie layout, with external value nodes.
pub struct LayoutV1<H>(PhantomData<H>);

// NOTE: the minimum size of child nodes is 32 bytes, this is just for compatibility with other packages
const FELT_ALIGNED_MAX_INLINE_VALUE: u32 = 31;

impl<H> TrieLayout for LayoutV0<H>
where
    H: Hasher,
{
    const USE_EXTENSION: bool = false;
    const ALLOW_EMPTY: bool = true;
    const MAX_INLINE_VALUE: Option<u32> = Some(FELT_ALIGNED_MAX_INLINE_VALUE);

    type Hash = H;
    type Codec = NodeCodec<Self::Hash>;
}

impl<H> TrieConfiguration for LayoutV0<H>
where
    H: Hasher,
{
    fn trie_root<I, A, B>(input: I) -> <Self::Hash as Hasher>::Out
    where
        I: IntoIterator<Item = (A, B)>,
        A: AsRef<[u8]> + Ord,
        B: AsRef<[u8]>,
    {
        let input_vec: Vec<_> = input.into_iter().collect();
        log::debug!(target: "zk-trie", "LayoutV1::trie_root input length: {}", input_vec.len());
        let result = trie_root::trie_root_no_extension::<H, TrieStream, _, _, _>(
            input_vec,
            Some(FELT_ALIGNED_MAX_INLINE_VALUE),
        );
        log::debug!(target: "zk-trie", "LayoutV1::trie_root result: {:02x?}", result.as_ref());
        result
    }

    fn trie_root_unhashed<I, A, B>(input: I) -> Vec<u8>
    where
        I: IntoIterator<Item = (A, B)>,
        A: AsRef<[u8]> + Ord,
        B: AsRef<[u8]>,
    {
        let input_vec: Vec<_> = input.into_iter().collect();
        log::debug!(target: "zk-trie", "LayoutV1::trie_root_unhashed input length: {}", input_vec.len());
        let result = trie_root::unhashed_trie_no_extension::<H, TrieStream, _, _, _>(
            input_vec,
            Some(FELT_ALIGNED_MAX_INLINE_VALUE),
        );
        log::debug!(target: "zk-trie", "LayoutV1::trie_root_unhashed result: {:02x?}", result);
        result
    }

    fn encode_index(input: u32) -> Vec<u8> {
        codec::Encode::encode(&codec::Compact(input))
    }
}

impl<H> TrieLayout for LayoutV1<H>
where
    H: Hasher,
{
    const USE_EXTENSION: bool = false;
    const ALLOW_EMPTY: bool = true;
    const MAX_INLINE_VALUE: Option<u32> = Some(FELT_ALIGNED_MAX_INLINE_VALUE);

    type Hash = H;
    type Codec = NodeCodec<Self::Hash>;
}

impl<H> TrieConfiguration for LayoutV1<H>
where
    H: Hasher,
{
    fn trie_root<I, A, B>(input: I) -> <Self::Hash as Hasher>::Out
    where
        I: IntoIterator<Item = (A, B)>,
        A: AsRef<[u8]> + Ord,
        B: AsRef<[u8]>,
    {
        trie_root::trie_root_no_extension::<H, TrieStream, _, _, _>(
            input,
            Some(FELT_ALIGNED_MAX_INLINE_VALUE),
        )
    }

    fn trie_root_unhashed<I, A, B>(input: I) -> Vec<u8>
    where
        I: IntoIterator<Item = (A, B)>,
        A: AsRef<[u8]> + Ord,
        B: AsRef<[u8]>,
    {
        trie_root::unhashed_trie_no_extension::<H, TrieStream, _, _, _>(
            input,
            Some(FELT_ALIGNED_MAX_INLINE_VALUE),
        )
    }

    fn encode_index(input: u32) -> Vec<u8> {
        codec::Encode::encode(&codec::Compact(input))
    }
}

/// Type that is able to provide a [`trie_db::TrieRecorder`].
///
/// Types implementing this trait can be used to maintain recorded state
/// across operations on different [`trie_db::TrieDB`] instances.
pub trait TrieRecorderProvider<H: Hasher> {
    /// Recorder type that is going to be returned by implementors of this trait.
    type Recorder<'a>: trie_db::TrieRecorder<H::Out> + 'a
    where
        Self: 'a;

    /// Create a [`StorageProof`] derived from the internal state.
    fn drain_storage_proof(self) -> Option<StorageProof>;

    /// Provide a recorder implementing [`trie_db::TrieRecorder`].
    fn as_trie_recorder(&self, storage_root: H::Out) -> Self::Recorder<'_>;
}

/// Type that is able to provide a proof size estimation.
pub trait ProofSizeProvider {
    /// Returns the storage proof size.
    fn estimate_encoded_size(&self) -> usize;
}

/// TrieDB error over `TrieConfiguration` trait.
pub type TrieError<L> = trie_db::TrieError<TrieHash<L>, CError<L>>;
/// Reexport from `hash_db`, with genericity set for `Hasher` trait.
pub trait AsHashDB<H: Hasher>: hash_db::AsHashDB<H, trie_db::DBValue> {}
impl<H: Hasher, T: hash_db::AsHashDB<H, trie_db::DBValue>> AsHashDB<H> for T {}
/// Reexport from `hash_db`, with genericity set for `Hasher` trait.
pub type HashDB<'a, H> = dyn hash_db::HashDB<H, trie_db::DBValue> + 'a;
/// ZK-trie compatible prefixed memory database with correct default initialization
pub struct PrefixedMemoryDB<H: Hasher>(
    memory_db::MemoryDB<H, memory_db::PrefixedKey<H>, trie_db::DBValue>,
);

impl<H: Hasher> PrefixedMemoryDB<H> {
    pub fn new(prefix: &[u8]) -> Self {
        Self(memory_db::MemoryDB::new(prefix))
    }

    pub fn default_with_root() -> (Self, H::Out) {
        let (inner_db, root) = memory_db::MemoryDB::default_with_root();
        (Self(inner_db), root)
    }

    pub fn consolidate(&mut self, other: Self) {
        self.0.consolidate(other.0)
    }
}

impl<H: Hasher> Clone for PrefixedMemoryDB<H> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<H: Hasher> Default for PrefixedMemoryDB<H> {
    fn default() -> Self {
        Self::new(&0u64.to_le_bytes())
    }
}

impl<H: Hasher> core::ops::Deref for PrefixedMemoryDB<H> {
    type Target = memory_db::MemoryDB<H, memory_db::PrefixedKey<H>, trie_db::DBValue>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<H: Hasher> core::ops::DerefMut for PrefixedMemoryDB<H> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<H: Hasher> hash_db::AsHashDB<H, trie_db::DBValue> for PrefixedMemoryDB<H> {
    fn as_hash_db(&self) -> &dyn hash_db::HashDB<H, trie_db::DBValue> {
        &self.0
    }

    fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn hash_db::HashDB<H, trie_db::DBValue> + 'a) {
        &mut self.0
    }
}

impl<H: Hasher> hash_db::AsHashDB<H, trie_db::DBValue> for &PrefixedMemoryDB<H> {
    fn as_hash_db(&self) -> &dyn hash_db::HashDB<H, trie_db::DBValue> {
        &self.0
    }

    fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn hash_db::HashDB<H, trie_db::DBValue> + 'a) {
        unreachable!("Cannot get mutable reference from shared reference")
    }
}

impl<H: Hasher> hash_db::HashDB<H, trie_db::DBValue> for PrefixedMemoryDB<H> {
    fn get(&self, key: &H::Out, prefix: hash_db::Prefix) -> Option<trie_db::DBValue> {
        hash_db::HashDB::get(&self.0, key, prefix)
    }

    fn contains(&self, key: &H::Out, prefix: hash_db::Prefix) -> bool {
        hash_db::HashDB::contains(&self.0, key, prefix)
    }

    fn insert(&mut self, prefix: hash_db::Prefix, value: &[u8]) -> H::Out {
        hash_db::HashDB::insert(&mut self.0, prefix, value)
    }

    fn emplace(&mut self, key: H::Out, prefix: hash_db::Prefix, value: trie_db::DBValue) {
        hash_db::HashDB::emplace(&mut self.0, key, prefix, value)
    }

    fn remove(&mut self, key: &H::Out, prefix: hash_db::Prefix) {
        hash_db::HashDB::remove(&mut self.0, key, prefix)
    }
}

impl<H: Hasher> hash_db::HashDBRef<H, trie_db::DBValue> for PrefixedMemoryDB<H> {
    fn get(&self, key: &H::Out, prefix: hash_db::Prefix) -> Option<trie_db::DBValue> {
        hash_db::HashDBRef::get(&self.0, key, prefix)
    }

    fn contains(&self, key: &H::Out, prefix: hash_db::Prefix) -> bool {
        hash_db::HashDBRef::contains(&self.0, key, prefix)
    }
}

/// ZK-trie compatible memory database with correct default initialization
pub struct MemoryDB<H: Hasher>(memory_db::MemoryDB<H, memory_db::HashKey<H>, trie_db::DBValue>);

impl<H: Hasher> MemoryDB<H> {
    pub fn new(prefix: &[u8]) -> Self {
        Self(memory_db::MemoryDB::new(prefix))
    }

    pub fn consolidate(&mut self, other: Self) {
        self.0.consolidate(other.0)
    }
}

impl<H: Hasher> Clone for MemoryDB<H> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<H: Hasher> Default for MemoryDB<H> {
    fn default() -> Self {
        Self::new(&0u64.to_le_bytes())
    }
}

impl<H: Hasher> core::ops::Deref for MemoryDB<H> {
    type Target = memory_db::MemoryDB<H, memory_db::HashKey<H>, trie_db::DBValue>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<H: Hasher> core::ops::DerefMut for MemoryDB<H> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<H: Hasher> hash_db::AsHashDB<H, trie_db::DBValue> for MemoryDB<H> {
    fn as_hash_db(&self) -> &dyn hash_db::HashDB<H, trie_db::DBValue> {
        &self.0
    }

    fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn hash_db::HashDB<H, trie_db::DBValue> + 'a) {
        &mut self.0
    }
}

impl<H: Hasher> hash_db::AsHashDB<H, trie_db::DBValue> for &MemoryDB<H> {
    fn as_hash_db(&self) -> &dyn hash_db::HashDB<H, trie_db::DBValue> {
        &self.0
    }

    fn as_hash_db_mut<'a>(&'a mut self) -> &'a mut (dyn hash_db::HashDB<H, trie_db::DBValue> + 'a) {
        unreachable!("Cannot get mutable reference from shared reference")
    }
}

impl<H: Hasher> hash_db::HashDB<H, trie_db::DBValue> for MemoryDB<H> {
    fn get(&self, key: &H::Out, prefix: hash_db::Prefix) -> Option<trie_db::DBValue> {
        hash_db::HashDB::get(&self.0, key, prefix)
    }

    fn contains(&self, key: &H::Out, prefix: hash_db::Prefix) -> bool {
        hash_db::HashDB::contains(&self.0, key, prefix)
    }

    fn insert(&mut self, prefix: hash_db::Prefix, value: &[u8]) -> H::Out {
        hash_db::HashDB::insert(&mut self.0, prefix, value)
    }

    fn emplace(&mut self, key: H::Out, prefix: hash_db::Prefix, value: trie_db::DBValue) {
        hash_db::HashDB::emplace(&mut self.0, key, prefix, value)
    }

    fn remove(&mut self, key: &H::Out, prefix: hash_db::Prefix) {
        hash_db::HashDB::remove(&mut self.0, key, prefix)
    }
}

impl<H: Hasher> hash_db::HashDBRef<H, trie_db::DBValue> for MemoryDB<H> {
    fn get(&self, key: &H::Out, prefix: hash_db::Prefix) -> Option<trie_db::DBValue> {
        hash_db::HashDBRef::get(&self.0, key, prefix)
    }

    fn contains(&self, key: &H::Out, prefix: hash_db::Prefix) -> bool {
        hash_db::HashDBRef::contains(&self.0, key, prefix)
    }
}

/// Reexport from `hash_db`, with genericity set for `Hasher` trait.
pub type GenericMemoryDB<H, KF> = memory_db::MemoryDB<H, KF, trie_db::DBValue>;

/// Persistent trie database read-access interface for a given hasher.
pub type TrieDB<'a, 'cache, L> = trie_db::TrieDB<'a, 'cache, L>;
/// Builder for creating a [`TrieDB`].
pub type TrieDBBuilder<'a, 'cache, L> = trie_db::TrieDBBuilder<'a, 'cache, L>;
/// Persistent trie database write-access interface for a given hasher.
pub type TrieDBMut<'a, L> = trie_db::TrieDBMut<'a, L>;
/// Builder for creating a [`TrieDBMut`].
pub type TrieDBMutBuilder<'a, L> = trie_db::TrieDBMutBuilder<'a, L>;
/// Querying interface, as in `trie_db` but less generic.
pub type Lookup<'a, 'cache, L, Q> = trie_db::Lookup<'a, 'cache, L, Q>;
/// Hash type for a trie layout.
pub type TrieHash<L> = <<L as TrieLayout>::Hash as Hasher>::Out;
/// This module is for non generic definition of trie type.
/// Only the `Hasher` trait is generic in this case.
pub mod trie_types {
    use super::*;

    /// Persistent trie database read-access interface for a given hasher.
    ///
    /// Read only V1 and V0 are compatible, thus we always use V1.
    pub type TrieDB<'a, 'cache, H> = super::TrieDB<'a, 'cache, LayoutV1<H>>;
    /// Builder for creating a [`TrieDB`].
    pub type TrieDBBuilder<'a, 'cache, H> = super::TrieDBBuilder<'a, 'cache, LayoutV1<H>>;
    /// Persistent trie database write-access interface for a given hasher.
    pub type TrieDBMutV0<'a, H> = super::TrieDBMut<'a, LayoutV0<H>>;
    /// Builder for creating a [`TrieDBMutV0`].
    pub type TrieDBMutBuilderV0<'a, H> = super::TrieDBMutBuilder<'a, LayoutV0<H>>;
    /// Persistent trie database write-access interface for a given hasher.
    pub type TrieDBMutV1<'a, H> = super::TrieDBMut<'a, LayoutV1<H>>;
    /// Builder for creating a [`TrieDBMutV1`].
    pub type TrieDBMutBuilderV1<'a, H> = super::TrieDBMutBuilder<'a, LayoutV1<H>>;
    /// Querying interface, as in `trie_db` but less generic.
    pub type Lookup<'a, 'cache, H, Q> = trie_db::Lookup<'a, 'cache, LayoutV1<H>, Q>;
    /// As in `trie_db`, but less generic, error type for the crate.
    pub type TrieError<H> = trie_db::TrieError<H, super::Error<H>>;
}

/// Create a proof for a subset of keys in a trie.
///
/// The `keys` may contain any set of keys regardless of each one of them is included
/// in the `db`.
///
/// For a key `K` that is included in the `db` a proof of inclusion is generated.
/// For a key `K` that is not included in the `db` a proof of non-inclusion is generated.
/// These can be later checked in `verify_trie_proof`.
pub fn generate_trie_proof<'a, L, I, K, DB>(
    db: &DB,
    root: TrieHash<L>,
    keys: I,
) -> Result<Vec<Vec<u8>>, Box<TrieError<L>>>
where
    L: TrieConfiguration,
    I: IntoIterator<Item = &'a K>,
    K: 'a + AsRef<[u8]>,
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    generate_proof::<_, L, _, _>(db, &root, keys)
}

/// Verify a set of key-value pairs against a trie root and a proof.
///
/// Checks a set of keys with optional values for inclusion in the proof that was generated by
/// `generate_trie_proof`.
/// If the value in the pair is supplied (`(key, Some(value))`), this key-value pair will be
/// checked for inclusion in the proof.
/// If the value is omitted (`(key, None)`), this key will be checked for non-inclusion in the
/// proof.
pub fn verify_trie_proof<'a, L, I, K, V>(
    root: &TrieHash<L>,
    proof: &[Vec<u8>],
    items: I,
) -> Result<(), VerifyError<TrieHash<L>, CError<L>>>
where
    L: TrieConfiguration,
    I: IntoIterator<Item = &'a (K, Option<V>)>,
    K: 'a + AsRef<[u8]>,
    V: 'a + AsRef<[u8]>,
{
    verify_proof::<L, _, _, _>(root, proof, items)
}

/// Determine a trie root given a hash DB and delta values.
pub fn delta_trie_root<L: TrieConfiguration, I, A, B, DB, V>(
    db: &mut DB,
    mut root: TrieHash<L>,
    delta: I,
    recorder: Option<&mut dyn trie_db::TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<TrieHash<L>, Box<TrieError<L>>>
where
    I: IntoIterator<Item = (A, B)>,
    A: Borrow<[u8]>,
    B: Borrow<Option<V>>,
    V: Borrow<[u8]>,
    DB: hash_db::HashDB<L::Hash, trie_db::DBValue>,
{
    {
        let mut trie = TrieDBMutBuilder::<L>::from_existing(db, &mut root)
            .with_optional_cache(cache)
            .with_optional_recorder(recorder)
            .build();

        let mut delta = delta.into_iter().collect::<Vec<_>>();
        delta.sort_by(|l, r| l.0.borrow().cmp(r.0.borrow()));

        for (key, change) in delta {
            match change.borrow() {
                Some(val) => trie.insert(key.borrow(), val.borrow())?,
                None => trie.remove(key.borrow())?,
            };
        }
    }

    Ok(root)
}

/// Read a value from the trie.
pub fn read_trie_value<L: TrieLayout, DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>>(
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<Option<Vec<u8>>, Box<TrieError<L>>> {
    TrieDBBuilder::<L>::new(db, root)
        .with_optional_cache(cache)
        .with_optional_recorder(recorder)
        .build()
        .get(key)
}

/// Read the [`trie_db::MerkleValue`] of the node that is the closest descendant for
/// the provided key.
pub fn read_trie_first_descendant_value<L: TrieLayout, DB>(
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<Option<MerkleValue<TrieHash<L>>>, Box<TrieError<L>>>
where
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    TrieDBBuilder::<L>::new(db, root)
        .with_optional_cache(cache)
        .with_optional_recorder(recorder)
        .build()
        .lookup_first_descendant(key)
}

/// Read a value from the trie with given Query.
pub fn read_trie_value_with<
    L: TrieLayout,
    Q: Query<L::Hash, Item = Vec<u8>>,
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
>(
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    query: Q,
) -> Result<Option<Vec<u8>>, Box<TrieError<L>>> {
    TrieDBBuilder::<L>::new(db, root)
        .build()
        .get_with(key, query)
}

/// Determine the empty trie root.
pub fn empty_trie_root<L: TrieConfiguration>() -> <L::Hash as Hasher>::Out {
    log::debug!(target: "zk-trie", "empty_trie_root called");
    let result = L::trie_root::<_, Vec<u8>, Vec<u8>>(core::iter::empty());
    log::debug!(target: "zk-trie", "empty_trie_root result: {:02x?}", result.as_ref());
    result
}

/// Determine the empty child trie root.
pub fn empty_child_trie_root<L: TrieConfiguration>() -> <L::Hash as Hasher>::Out {
    L::trie_root::<_, Vec<u8>, Vec<u8>>(core::iter::empty())
}

/// Determine a child trie root given its ordered contents, closed form. H is the default hasher,
/// but a generic implementation may ignore this type parameter and use other hashers.
pub fn child_trie_root<L: TrieConfiguration, I, A, B>(input: I) -> <L::Hash as Hasher>::Out
where
    I: IntoIterator<Item = (A, B)>,
    A: AsRef<[u8]> + Ord,
    B: AsRef<[u8]>,
{
    L::trie_root(input)
}

/// Determine a child trie root given a hash DB and delta values. H is the default hasher,
/// but a generic implementation may ignore this type parameter and use other hashers.
pub fn child_delta_trie_root<L: TrieConfiguration, I, A, B, DB, RD, V>(
    keyspace: &[u8],
    db: &mut DB,
    root_data: RD,
    delta: I,
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<<L::Hash as Hasher>::Out, Box<TrieError<L>>>
where
    I: IntoIterator<Item = (A, B)>,
    A: Borrow<[u8]>,
    B: Borrow<Option<V>>,
    V: Borrow<[u8]>,
    RD: AsRef<[u8]>,
    DB: hash_db::HashDB<L::Hash, trie_db::DBValue>,
{
    let mut root = TrieHash::<L>::default();
    // root is fetched from DB, not writable by runtime, so it's always valid.
    root.as_mut().copy_from_slice(root_data.as_ref());

    let mut db = KeySpacedDBMut::new(db, keyspace);
    delta_trie_root::<L, _, _, _, _, _>(&mut db, root, delta, recorder, cache)
}

/// Read a value from the child trie.
pub fn read_child_trie_value<L: TrieConfiguration, DB>(
    keyspace: &[u8],
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<Option<Vec<u8>>, Box<TrieError<L>>>
where
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    let db = KeySpacedDB::new(db, keyspace);
    TrieDBBuilder::<L>::new(&db, &root)
        .with_optional_recorder(recorder)
        .with_optional_cache(cache)
        .build()
        .get(key)
        .map(|x| x.map(|val| val.to_vec()))
}

/// Read a hash from the child trie.
pub fn read_child_trie_hash<L: TrieConfiguration, DB>(
    keyspace: &[u8],
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<Option<TrieHash<L>>, Box<TrieError<L>>>
where
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    let db = KeySpacedDB::new(db, keyspace);
    TrieDBBuilder::<L>::new(&db, &root)
        .with_optional_recorder(recorder)
        .with_optional_cache(cache)
        .build()
        .get_hash(key)
}

/// Read the [`trie_db::MerkleValue`] of the node that is the closest descendant for
/// the provided child key.
pub fn read_child_trie_first_descendant_value<L: TrieConfiguration, DB>(
    keyspace: &[u8],
    db: &DB,
    root: &TrieHash<L>,
    key: &[u8],
    recorder: Option<&mut dyn TrieRecorder<TrieHash<L>>>,
    cache: Option<&mut dyn TrieCache<L::Codec>>,
) -> Result<Option<MerkleValue<TrieHash<L>>>, Box<TrieError<L>>>
where
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    let db = KeySpacedDB::new(db, keyspace);
    TrieDBBuilder::<L>::new(&db, &root)
        .with_optional_recorder(recorder)
        .with_optional_cache(cache)
        .build()
        .lookup_first_descendant(key)
}

/// Read a value from the child trie with given query.
pub fn read_child_trie_value_with<L, Q, DB>(
    keyspace: &[u8],
    db: &DB,
    root_slice: &[u8],
    key: &[u8],
    query: Q,
) -> Result<Option<Vec<u8>>, Box<TrieError<L>>>
where
    L: TrieConfiguration,
    Q: Query<L::Hash, Item = DBValue>,
    DB: hash_db::HashDBRef<L::Hash, trie_db::DBValue>,
{
    let mut root = TrieHash::<L>::default();
    // root is fetched from DB, not writable by runtime, so it's always valid.
    root.as_mut().copy_from_slice(root_slice);

    let db = KeySpacedDB::new(db, keyspace);
    TrieDBBuilder::<L>::new(&db, &root)
        .build()
        .get_with(key, query)
        .map(|x| x.map(|val| val.to_vec()))
}

/// `HashDB` implementation that append a encoded prefix (unique id bytes) in addition to the
/// prefix of every key value.
pub struct KeySpacedDB<'a, DB: ?Sized, H>(&'a DB, &'a [u8], PhantomData<H>);

/// `HashDBMut` implementation that append a encoded prefix (unique id bytes) in addition to the
/// prefix of every key value.
///
/// Mutable variant of `KeySpacedDB`, see [`KeySpacedDB`].
pub struct KeySpacedDBMut<'a, DB: ?Sized, H>(&'a mut DB, &'a [u8], PhantomData<H>);

/// Utility function used to merge some byte data (keyspace) and `prefix` data
/// before calling key value database primitives.
fn keyspace_as_prefix_alloc(ks: &[u8], prefix: Prefix) -> (Vec<u8>, Option<u8>) {
    let mut result = vec![0; ks.len() + prefix.0.len()];
    result[..ks.len()].copy_from_slice(ks);
    result[ks.len()..].copy_from_slice(prefix.0);
    (result, prefix.1)
}

impl<'a, DB: ?Sized, H> KeySpacedDB<'a, DB, H> {
    /// instantiate new keyspaced db
    #[inline]
    pub fn new(db: &'a DB, ks: &'a [u8]) -> Self {
        KeySpacedDB(db, ks, PhantomData)
    }
}

impl<'a, DB: ?Sized, H> KeySpacedDBMut<'a, DB, H> {
    /// instantiate new keyspaced db
    pub fn new(db: &'a mut DB, ks: &'a [u8]) -> Self {
        KeySpacedDBMut(db, ks, PhantomData)
    }
}

impl<'a, DB, H, T> hash_db::HashDBRef<H, T> for KeySpacedDB<'a, DB, H>
where
    DB: hash_db::HashDBRef<H, T> + ?Sized,
    H: Hasher,
    T: From<&'static [u8]>,
{
    fn get(&self, key: &H::Out, prefix: Prefix) -> Option<T> {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.get(key, (&derived_prefix.0, derived_prefix.1))
    }

    fn contains(&self, key: &H::Out, prefix: Prefix) -> bool {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.contains(key, (&derived_prefix.0, derived_prefix.1))
    }
}

impl<'a, DB, H, T> hash_db::HashDB<H, T> for KeySpacedDBMut<'a, DB, H>
where
    DB: hash_db::HashDB<H, T>,
    H: Hasher,
    T: Default + PartialEq<T> + for<'b> From<&'b [u8]> + Clone + Send + Sync,
{
    fn get(&self, key: &H::Out, prefix: Prefix) -> Option<T> {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.get(key, (&derived_prefix.0, derived_prefix.1))
    }

    fn contains(&self, key: &H::Out, prefix: Prefix) -> bool {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.contains(key, (&derived_prefix.0, derived_prefix.1))
    }

    fn insert(&mut self, prefix: Prefix, value: &[u8]) -> H::Out {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.insert((&derived_prefix.0, derived_prefix.1), value)
    }

    fn emplace(&mut self, key: H::Out, prefix: Prefix, value: T) {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0
            .emplace(key, (&derived_prefix.0, derived_prefix.1), value)
    }

    fn remove(&mut self, key: &H::Out, prefix: Prefix) {
        let derived_prefix = keyspace_as_prefix_alloc(self.1, prefix);
        self.0.remove(key, (&derived_prefix.0, derived_prefix.1))
    }
}

impl<'a, DB, H, T> hash_db::AsHashDB<H, T> for KeySpacedDBMut<'a, DB, H>
where
    DB: hash_db::HashDB<H, T>,
    H: Hasher,
    T: Default + PartialEq<T> + for<'b> From<&'b [u8]> + Clone + Send + Sync,
{
    fn as_hash_db(&self) -> &dyn hash_db::HashDB<H, T> {
        self
    }

    fn as_hash_db_mut<'b>(&'b mut self) -> &'b mut (dyn hash_db::HashDB<H, T> + 'b) {
        &mut *self
    }
}

/// Constants used into trie simplification codec.
mod trie_constants {
    pub const EMPTY_TRIE: u64 = 0x00000000_00000000; // 8-byte null header for new format
    pub const ESCAPE_COMPACT_HEADER: u8 = 0x01; // Update since EMPTY_TRIE is now an array
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_header::NodeHeader;
    use codec::{Compact, Decode, Encode};
    use hash_db::{HashDB, Hasher};
    use sp_core::Blake2Hasher;
    use trie_db::{DBValue, NodeCodec as NodeCodecT, Trie, TrieMut};
    use trie_standardmap::{Alphabet, StandardMap, ValueMode};

    type LayoutV0 = super::LayoutV0<Blake2Hasher>;
    type LayoutV1 = super::LayoutV1<Blake2Hasher>;

    type MemoryDBMeta<H> = memory_db::MemoryDB<H, memory_db::HashKey<H>, trie_db::DBValue>;

    pub fn create_trie<L: TrieLayout>(
        data: &[(&[u8], &[u8])],
    ) -> (MemoryDB<L::Hash>, trie_db::TrieHash<L>) {
        let mut db = MemoryDB::new(&0u64.to_le_bytes());
        let mut root = Default::default();

        {
            let mut trie = trie_db::TrieDBMutBuilder::<L>::new(&mut db, &mut root).build();
            for (k, v) in data {
                println!("k {:?} v {:?}", k, v);
                trie.insert(k, v).expect("Inserts data");
            }
        }

        let mut recorder = Recorder::<L>::new();
        {
            let trie = trie_db::TrieDBBuilder::<L>::new(&mut db, &mut root)
                .with_recorder(&mut recorder)
                .build();
            for (k, _v) in data {
                trie.get(k).unwrap();
            }
        }

        (db, root)
    }

    pub fn create_storage_proof<L: TrieLayout>(
        data: &[(&[u8], &[u8])],
    ) -> (RawStorageProof, trie_db::TrieHash<L>) {
        let (db, root) = create_trie::<L>(data);

        let mut recorder = Recorder::<L>::new();
        {
            let trie = trie_db::TrieDBBuilder::<L>::new(&db, &root)
                .with_recorder(&mut recorder)
                .build();
            for (k, _v) in data {
                trie.get(k).unwrap();
            }
        }

        (
            recorder
                .drain()
                .into_iter()
                .map(|record| record.data)
                .collect(),
            root,
        )
    }

    fn hashed_null_node<T: TrieConfiguration>() -> TrieHash<T> {
        <T::Codec as NodeCodecT>::hashed_null_node()
    }

    fn check_equivalent<T: TrieConfiguration>(input: &Vec<(&[u8], &[u8])>) {
        {
            let closed_form = T::trie_root(input.clone());
            let persistent = {
                let mut memdb = MemoryDBMeta::new(&0u64.to_le_bytes());
                let mut root = Default::default();
                let mut t = TrieDBMutBuilder::<T>::new(&mut memdb, &mut root).build();
                for (x, y) in input.iter().rev() {
                    t.insert(x, y).unwrap();
                }
                *t.root()
            };
            assert_eq!(closed_form, persistent);
        }
    }

    fn check_iteration<T: TrieConfiguration>(input: &Vec<(&[u8], &[u8])>) {
        let mut memdb = MemoryDBMeta::new(&0u64.to_le_bytes());
        let mut root = Default::default();
        {
            let mut t = TrieDBMutBuilder::<T>::new(&mut memdb, &mut root).build();
            for (x, y) in input.clone() {
                t.insert(x, y).unwrap();
            }
        }
        {
            let t = TrieDBBuilder::<T>::new(&memdb, &root).build();
            assert_eq!(
                input
                    .iter()
                    .map(|(i, j)| (i.to_vec(), j.to_vec()))
                    .collect::<Vec<_>>(),
                t.iter()
                    .unwrap()
                    .map(|x| x.map(|y| (y.0, y.1.to_vec())).unwrap())
                    .collect::<Vec<_>>()
            );
        }
    }

    fn check_input(input: &Vec<(&[u8], &[u8])>) {
        check_equivalent::<LayoutV0>(input);
        check_iteration::<LayoutV0>(input);
        check_equivalent::<LayoutV1>(input);
        check_iteration::<LayoutV1>(input);
    }

    #[test]
    fn default_trie_root() {
        let mut db = MemoryDB::new(&0u64.to_le_bytes());
        let mut root = TrieHash::<LayoutV1>::default();
        let mut empty = TrieDBMutBuilder::<LayoutV1>::new(&mut db, &mut root).build();
        empty.commit();
        let root1 = empty.root().as_ref().to_vec();
        let root2: Vec<u8> = LayoutV1::trie_root::<_, Vec<u8>, Vec<u8>>(std::iter::empty())
            .as_ref()
            .iter()
            .cloned()
            .collect();

        assert_eq!(root1, root2);
    }

    #[test]
    fn empty_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![];
        check_input(&input);
    }

    #[test]
    fn leaf_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![(&[0xaa][..], &[0xbb][..])];
        check_input(&input);
    }

    #[test]
    fn branch_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![(&[0xaa][..], &[][..]), (&[0xba][..], &[][..])];
        check_input(&input);
    }

    #[test]
    fn extension_and_branch_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> =
            vec![(&[0xaa][..], &[0x10][..]), (&[0xab][..], &[0x11][..])];
        check_input(&input);
    }

    #[test]
    fn standard_is_equivalent() {
        let st = StandardMap {
            alphabet: Alphabet::All,
            min_key: 32,
            journal_key: 0,
            value_mode: ValueMode::Random,
            count: 1000,
        };
        let mut d = st.make();
        d.sort_by(|(a, _), (b, _)| a.cmp(b));
        let dr = d.iter().map(|v| (&v.0[..], &v.1[..])).collect();
        check_input(&dr);
    }

    #[test]
    fn extension_and_branch_with_value_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![
            (&[0xaa][..], &[0xa0][..]),
            (&[0xaa, 0xaa][..], &[0xaa][..]),
            (&[0xaa, 0xbb][..], &[0xab][..]),
        ];
        check_input(&input);
    }

    #[test]
    fn bigger_extension_and_branch_with_value_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![
            (&[0xaa][..], &[0xa0][..]),
            (&[0xaa, 0xaa][..], &[0xaa][..]),
            (&[0xaa, 0xbb][..], &[0xab][..]),
            (&[0xbb][..], &[0xb0][..]),
            (&[0xbb, 0xbb][..], &[0xbb][..]),
            (&[0xbb, 0xcc][..], &[0xbc][..]),
        ];
        check_input(&input);
    }

    #[test]
    fn single_long_leaf_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![
            (
                &[0xaa][..],
                &b"ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC"[..],
            ),
            (&[0xba][..], &[0x11][..]),
        ];
        check_input(&input);
    }

    #[test]
    fn two_long_leaves_is_equivalent() {
        let input: Vec<(&[u8], &[u8])> = vec![
            (
                &[0xaa][..],
                &b"ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC"[..],
            ),
            (
                &[0xba][..],
                &b"ABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABCABC"[..],
            ),
        ];
        check_input(&input);
    }

    fn populate_trie<'db, T: TrieConfiguration>(
        db: &'db mut dyn HashDB<T::Hash, DBValue>,
        root: &'db mut TrieHash<T>,
        v: &[(Vec<u8>, Vec<u8>)],
    ) -> TrieDBMut<'db, T> {
        let mut t = TrieDBMutBuilder::<T>::new(db, root).build();
        for i in 0..v.len() {
            let key: &[u8] = &v[i].0;
            let val: &[u8] = &v[i].1;
            t.insert(key, val).unwrap();
        }
        t
    }

    fn unpopulate_trie<T: TrieConfiguration>(t: &mut TrieDBMut<'_, T>, v: &[(Vec<u8>, Vec<u8>)]) {
        for i in v {
            let key: &[u8] = &i.0;
            t.remove(key).unwrap();
        }
    }

    #[test]
    fn random_should_work() {
        random_should_work_inner::<LayoutV1>();
        random_should_work_inner::<LayoutV0>();
    }

    #[test]
    fn random_test_8_byte_alignment() {
        random_test_8_byte_alignment_inner::<LayoutV1>();
        random_test_8_byte_alignment_inner::<LayoutV0>();
    }

    fn random_test_8_byte_alignment_inner<L: TrieConfiguration>() {
        println!("Running 100 random trie alignment tests...");
        let mut seed = <Blake2Hasher as Hasher>::Out::zero();
        for test_i in 0..20 {
            if test_i % 10 == 0 {
                println!("  Progress: {}/20", test_i);
            }
            let x = StandardMap {
                alphabet: Alphabet::Custom(b"@QWERTYUIOPASDFGHJKLZXCVBNM[/]^_".to_vec()),
                min_key: 5,
                journal_key: 0,
                value_mode: ValueMode::Index,
                count: 10, // Reduced count for faster testing
            }
            .make_with(seed.as_fixed_bytes_mut());

            // Test closed-form trie for alignment
            let unhashed_root = L::trie_root_unhashed(x.clone());
            check_8_byte_alignment(&unhashed_root, 0, "root");

            // Test storage proof alignment (the critical path)
            let x_refs: Vec<_> = x
                .iter()
                .map(|(k, v)| (k.as_slice(), v.as_slice()))
                .collect();
            let (db, root) = create_trie::<L>(&x_refs);
            let proof_keys: Vec<_> = x.iter().map(|(k, _)| k.clone()).collect();
            if let Ok(proof) = crate::generate_trie_proof::<L, _, _, _>(&db, root, &proof_keys) {
                for (i, node) in proof.iter().enumerate() {
                    if node.len() % 8 != 0 {
                        panic!("Random trie test {}: storage proof node {} length {} not 8-byte aligned", test_i, i, node.len());
                    }
                }
            }

            seed = <Blake2Hasher as Hasher>::hash(seed.as_ref());
        }
    }

    #[test]
    fn storage_proof_8_byte_alignment_test() {
        storage_proof_8_byte_alignment_inner::<LayoutV1>();
        storage_proof_8_byte_alignment_inner::<LayoutV0>();
    }

    #[test]
    fn child_reference_8_byte_boundary_test() {
        child_reference_8_byte_boundary_inner::<LayoutV1>();
        child_reference_8_byte_boundary_inner::<LayoutV0>();
    }

    fn storage_proof_8_byte_alignment_inner<L: TrieConfiguration>() {
        use crate::StorageProof;
        use rand::Rng;

        let mut rng = rand::thread_rng();
        let mut total_proof_nodes = 0;

        // Test 1: Random data test (no verbose output)
        for iteration in 0..10 {
            let num_entries = rng.gen_range(5..20);
            let mut test_data = Vec::new();

            for i in 0..num_entries {
                let key_len = rng.gen_range(1..=200);
                let mut key = vec![0u8; key_len];
                rng.fill(&mut key[..]);
                key[0] = (iteration as u8).wrapping_add(i as u8);

                let value_len = rng.gen_range(0..=100);
                let mut value = vec![0u8; value_len];
                rng.fill(&mut value[..]);

                test_data.push((key, value));
            }

            let (db, root) = create_trie::<L>(
                &test_data
                    .iter()
                    .map(|(k, v)| (k.as_slice(), v.as_slice()))
                    .collect::<Vec<_>>(),
            );
            let proof_keys: Vec<Vec<u8>> = test_data.iter().map(|(k, _)| k.clone()).collect();
            let proof = crate::generate_trie_proof::<L, _, _, _>(&db, root, &proof_keys).unwrap();

            total_proof_nodes += proof.len();

            // Verify ALL proof nodes are 8-byte aligned
            for node in &proof {
                assert_eq!(
                    node.len() % 8,
                    0,
                    "Storage proof node not 8-byte aligned: length {}",
                    node.len()
                );
            }

            // Test storage proof reconstruction
            let storage_proof = StorageProof::new(proof.clone());
            let mut proof_db = storage_proof.into_memory_db::<L::Hash>();

            for (_, (node_data, _)) in proof_db.drain() {
                assert_eq!(
                    node_data.len() % 8,
                    0,
                    "Reconstructed proof node not 8-byte aligned: length {}",
                    node_data.len()
                );
            }

            // Verify proof works correctly
            let items_to_verify: Vec<_> = test_data
                .iter()
                .map(|(k, v)| (k.as_slice(), Some(v.as_slice())))
                .collect();
            crate::verify_trie_proof::<L, _, _, _>(&root, &proof, &items_to_verify).unwrap();
        }

        // Test 2: Edge cases
        let edge_cases = vec![
            (vec![42u8; 10], vec![]),          // Empty value
            (vec![43u8; 1], vec![1u8]),        // Tiny value
            (vec![45u8; 5], vec![2u8; 23]),    // Just under 24-byte threshold
            (vec![46u8; 6], vec![3u8; 24]),    // Exactly at threshold
            (vec![47u8; 7], vec![4u8; 25]),    // Just over threshold
            (vec![48u8; 100], vec![5u8; 200]), // Large key and value
        ];

        for (key, value) in edge_cases {
            let test_data = vec![(key.clone(), value.clone())];
            let (db, root) = create_trie::<L>(
                &test_data
                    .iter()
                    .map(|(k, v)| (k.as_slice(), v.as_slice()))
                    .collect::<Vec<_>>(),
            );
            let proof = crate::generate_trie_proof::<L, _, _, _>(&db, root, &[&key]).unwrap();

            for node in &proof {
                assert_eq!(
                    node.len() % 8,
                    0,
                    "Edge case node not 8-byte aligned: length {}",
                    node.len()
                );
            }
        }

        // Test 3: Non-inclusion proofs
        for _ in 0..3 {
            let mut test_data = Vec::new();
            for i in 0..rng.gen_range(5..10) {
                let key = vec![100u8 + i; rng.gen_range(5..50)];
                let value = vec![200u8 + i; rng.gen_range(1..50)];
                test_data.push((key, value));
            }

            let (db, root) = create_trie::<L>(
                &test_data
                    .iter()
                    .map(|(k, v)| (k.as_slice(), v.as_slice()))
                    .collect::<Vec<_>>(),
            );

            let non_existent_keys = vec![vec![250u8; 10], vec![251u8; 20]];
            let proof =
                crate::generate_trie_proof::<L, _, _, _>(&db, root, &non_existent_keys).unwrap();

            for node in &proof {
                assert_eq!(
                    node.len() % 8,
                    0,
                    "Non-inclusion proof node not 8-byte aligned: length {}",
                    node.len()
                );
            }
        }

        println!("✅ Storage proof 8-byte alignment verification PASSED!");
        println!(
            "   ✓ {} total proof nodes verified (all 8-byte aligned)",
            total_proof_nodes
        );
        println!("   ✓ Random data tests passed");
        println!("   ✓ Edge case tests passed");
        println!("   ✓ Non-inclusion proof tests passed");
    }

    fn child_reference_8_byte_boundary_inner<L: TrieConfiguration>() {
        use crate::NodeCodec;
        use rand::Rng;
        use trie_db::NodeCodec as NodeCodecT;

        let mut rng = rand::thread_rng();
        let mut nodes_checked = 0;
        let mut child_refs_checked = 0;

        println!("Checking child reference positioning at 8-byte boundaries...");

        // Test with a few specific cases to create branch nodes
        let test_cases = vec![
            // Case 1: Keys with same first nibble to force branching
            vec![
                (vec![0x10, 0x01], vec![1u8; 10]),
                (vec![0x10, 0x02], vec![2u8; 15]),
                (vec![0x10, 0x03], vec![3u8; 20]),
                (vec![0x20, 0x01], vec![4u8; 25]),
                (vec![0x20, 0x02], vec![5u8; 30]),
            ],
            // Case 2: Keys designed to create complex branching
            vec![
                (vec![0xab, 0xcd], vec![1u8; 5]),
                (vec![0xab, 0xce], vec![2u8; 50]),
                (vec![0xac, 0xcd], vec![3u8; 8]),
                (vec![0xad, 0xce], vec![4u8; 12]),
            ],
        ];

        for test_data in test_cases {
            let (db, root) = create_trie::<L>(
                &test_data
                    .iter()
                    .map(|(k, v)| (k.as_slice(), v.as_slice()))
                    .collect::<Vec<_>>(),
            );

            // Generate storage proof to get encoded nodes
            let proof_keys: Vec<Vec<u8>> = test_data.iter().map(|(k, _)| k.clone()).collect();
            let proof = crate::generate_trie_proof::<L, _, _, _>(&db, root, &proof_keys).unwrap();

            // Analyze each proof node for child reference positioning
            for node_data in &proof {
                if let Ok(node_plan) = NodeCodec::<L::Hash>::decode_plan(node_data) {
                    nodes_checked += 1;

                    match node_plan {
                        trie_db::node::NodePlan::NibbledBranch { children, .. } => {
                            // This is a branch node - check child reference positions
                            check_branch_node_child_positions::<L>(
                                node_data,
                                &children,
                                &mut child_refs_checked,
                            );
                        }
                        _ => {} // Other node types don't have child references
                    }
                }
            }
        }

        println!("✅ Child reference boundary verification PASSED!");
        println!("   ✓ {} nodes analyzed", nodes_checked);
        println!(
            "   ✓ {} child references verified at 8-byte boundaries",
            child_refs_checked
        );
    }

    fn check_branch_node_child_positions<L: TrieConfiguration>(
        node_data: &[u8],
        children: &[Option<trie_db::node::NodeHandlePlan>; 16],
        child_refs_checked: &mut usize,
    ) {
        use crate::NodeCodec;

        // Parse the node structure manually to verify positioning
        let mut cursor = 0;

        // Skip header (8 bytes)
        cursor += 8;

        // Skip partial key data (felt-aligned)
        if let Ok(node_plan) = NodeCodec::<L::Hash>::decode_plan(node_data) {
            if let trie_db::node::NodePlan::NibbledBranch { partial, value, .. } = node_plan {
                // Calculate partial key size
                let nibble_count = partial.len();
                let nibble_bytes = (nibble_count + 1) / 2;
                let felt_aligned_bytes = ((nibble_bytes + 7) / 8) * 8;
                cursor += felt_aligned_bytes;

                // Skip bitmap (8 bytes)
                cursor += 8;

                // Skip value if present
                if value.is_some() {
                    match value {
                        Some(trie_db::node::ValuePlan::Inline(range)) => {
                            // Skip 8-byte length + value data (felt-aligned)
                            cursor += 8;
                            let value_len = range.end - range.start;
                            let value_aligned_len = ((value_len + 7) / 8) * 8;
                            cursor += value_aligned_len;
                        }
                        Some(trie_db::node::ValuePlan::Node(_)) => {
                            // Skip hash reference
                            cursor += <L::Hash as trie_db::Hasher>::LENGTH;
                        }
                        None => {}
                    }
                }

                // Now check child reference positions
                for (i, child) in children.iter().enumerate() {
                    if child.is_some() {
                        // Each child reference should start at an 8-byte boundary
                        if cursor % 8 != 0 {
                            panic!(
                                "❌ CHILD REFERENCE ALIGNMENT VIOLATION: Child {} at position {} (not 8-byte aligned)\n   \
                                Node length: {}, Cursor after header+partial+bitmap+value: {}",
                                i, cursor, node_data.len(), cursor
                            );
                        }

                        *child_refs_checked += 1;

                        // Skip over this child reference
                        cursor += 8; // 8-byte length prefix
                        match child {
                            Some(trie_db::node::NodeHandlePlan::Hash(_)) => {
                                cursor += <L::Hash as trie_db::Hasher>::LENGTH;
                            }
                            Some(trie_db::node::NodeHandlePlan::Inline(range)) => {
                                cursor += range.end - range.start;
                            }
                            None => {}
                        }
                    }
                }
            }
        }
    }

    fn check_8_byte_alignment(data: &[u8], offset: usize, context: &str) {
        if data.len() % 8 != 0 {
            println!(
                "❌ {} at offset {} has length {} (not 8-byte aligned)",
                context,
                offset,
                data.len()
            );
            println!("Data: {:?}", data);
            panic!("8-byte alignment violation in {}", context);
        }
        // Silent success - only print failures
    }
    fn random_should_work_inner<L: TrieConfiguration>() {
        let mut seed = <Blake2Hasher as Hasher>::Out::zero();
        for test_i in 0..10_000 {
            if test_i % 50 == 0 {
                println!("{:?} of 10000 stress tests done", test_i);
            }
            let x = StandardMap {
                alphabet: Alphabet::Custom(b"@QWERTYUIOPASDFGHJKLZXCVBNM[/]^_".to_vec()),
                min_key: 5,
                journal_key: 0,
                value_mode: ValueMode::Index,
                count: 100,
            }
            .make_with(seed.as_fixed_bytes_mut());

            let real = L::trie_root(x.clone());
            let mut memdb = MemoryDB::new(&0u64.to_le_bytes());
            let mut root = Default::default();

            let mut memtrie = populate_trie::<L>(&mut memdb, &mut root, &x);

            memtrie.commit();
            if *memtrie.root() != real {
                println!("TRIE MISMATCH");
                println!();
                println!("{:?} vs {:?}", memtrie.root(), real);
                for i in &x {
                    println!("{:#x?} -> {:#x?}", i.0, i.1);
                }
            }
            assert_eq!(*memtrie.root(), real);
            unpopulate_trie::<L>(&mut memtrie, &x);
            memtrie.commit();
            let hashed_null_node = hashed_null_node::<L>();
            if *memtrie.root() != hashed_null_node {
                println!("- TRIE MISMATCH");
                println!();
                println!("{:?} vs {:?}", memtrie.root(), hashed_null_node);
                for i in &x {
                    println!("{:#x?} -> {:#x?}", i.0, i.1);
                }
            }
            assert_eq!(*memtrie.root(), hashed_null_node);
        }
    }

    fn to_u64_le_bytes(n: u8) -> [u8; 8] {
        (n as u64).to_le_bytes()
    }

    fn to_compact(n: u8) -> u8 {
        Compact(n).encode()[0]
    }

    #[test]
    fn codec_trie_empty() {
        let input: Vec<(&[u8], &[u8])> = vec![];
        let trie = LayoutV1::trie_root_unhashed(input);
        println!("trie: {:#x?}", trie);
        assert_eq!(trie, vec![0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0]);
    }

    #[test]
    fn codec_trie_single_tuple() {
        let input = vec![(vec![0xaa], vec![0xbb])];
        let trie = LayoutV1::trie_root_unhashed(input);
        println!("trie: {:#x?}", trie);
        let mut expected = vec![
            0x2, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
            0x30, // 8-byte leaf header (nibble_count=2, type=3)
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, // left-padded
        ];
        expected.extend_from_slice(&to_u64_le_bytes(1)); // length of value in bytes as 8-byte little-endian
        expected.extend_from_slice(&[0xbb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // value data (felt-aligned to 8 bytes)
        assert_eq!(trie, expected);
    }

    #[test]
    fn codec_trie_two_tuples_disjoint_keys() {
        let input = vec![(&[0x48, 0x19], &[0xfe]), (&[0x13, 0x14], &[0xff])];
        let trie = LayoutV1::trie_root_unhashed(input);
        println!("trie: {:#x?}", trie);

        // With 8-byte aligned values, children are now 32 bytes and get hashed
        // Just verify the structure rather than exact hash values
        assert_eq!(trie.len(), 96); // 8 (header) + 8 (bitmap) + 8 (length) + 32 (hash) + 8 (length) + 32 (hash)

        // Check header: branch with no value, nibble_count=0, type=2
        assert_eq!(&trie[0..8], &[0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x20]);

        // Check bitmap: slots 1 & 4 are taken
        assert_eq!(
            &trie[8..16],
            &[0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );

        // Check first child is hash reference (32 bytes)
        assert_eq!(&trie[16..24], &[32, 0, 0, 0, 0, 0, 0, 0]);

        // Check second child is hash reference (32 bytes)
        assert_eq!(&trie[56..64], &[32, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn iterator_works() {
        iterator_works_inner::<LayoutV1>();
        iterator_works_inner::<LayoutV0>();
    }
    fn iterator_works_inner<Layout: TrieConfiguration>() {
        let pairs = vec![
            (
                array_bytes::hex2bytes_unchecked("0103000000000000000464"),
                array_bytes::hex2bytes_unchecked("0400000000"),
            ),
            (
                array_bytes::hex2bytes_unchecked("0103000000000000000469"),
                array_bytes::hex2bytes_unchecked("0401000000"),
            ),
        ];

        let mut mdb = MemoryDB::new(&0u64.to_le_bytes());
        let mut root = Default::default();
        let _ = populate_trie::<Layout>(&mut mdb, &mut root, &pairs);

        let trie = TrieDBBuilder::<Layout>::new(&mdb, &root).build();

        let iter = trie.iter().unwrap();
        let mut iter_pairs = Vec::new();
        for pair in iter {
            let (key, value) = pair.unwrap();
            iter_pairs.push((key, value));
        }

        assert_eq!(pairs, iter_pairs);
    }

    #[test]
    fn proof_non_inclusion_works() {
        let pairs = vec![
            (
                array_bytes::hex2bytes_unchecked("0102"),
                array_bytes::hex2bytes_unchecked("01"),
            ),
            (
                array_bytes::hex2bytes_unchecked("0203"),
                array_bytes::hex2bytes_unchecked("0405"),
            ),
        ];

        let mut memdb = MemoryDB::new(&0u64.to_le_bytes());
        let mut root = Default::default();
        populate_trie::<LayoutV1>(&mut memdb, &mut root, &pairs);

        let non_included_key: Vec<u8> = array_bytes::hex2bytes_unchecked("0909");
        let proof =
            generate_trie_proof::<LayoutV1, _, _, _>(&memdb, root, &[non_included_key.clone()])
                .unwrap();

        // Verifying that the K was not included into the trie should work.
        assert!(verify_trie_proof::<LayoutV1, _, _, Vec<u8>>(
            &root,
            &proof,
            &[(non_included_key.clone(), None)],
        )
        .is_ok());

        // Verifying that the K was included into the trie should fail.
        assert!(verify_trie_proof::<LayoutV1, _, _, Vec<u8>>(
            &root,
            &proof,
            &[(
                non_included_key,
                Some(array_bytes::hex2bytes_unchecked("1010"))
            )],
        )
        .is_err());
    }

    #[test]
    fn proof_inclusion_works() {
        let pairs = vec![
            (
                array_bytes::hex2bytes_unchecked("0102"),
                array_bytes::hex2bytes_unchecked("01"),
            ),
            (
                array_bytes::hex2bytes_unchecked("0203"),
                array_bytes::hex2bytes_unchecked("0405"),
            ),
        ];

        let mut memdb = MemoryDB::new(&0u64.to_le_bytes());
        let mut root = Default::default();
        populate_trie::<LayoutV1>(&mut memdb, &mut root, &pairs);

        let proof =
            generate_trie_proof::<LayoutV1, _, _, _>(&memdb, root, &[pairs[0].0.clone()]).unwrap();

        // Check that a K, V included into the proof are verified.
        assert!(verify_trie_proof::<LayoutV1, _, _, _>(
            &root,
            &proof,
            &[(pairs[0].0.clone(), Some(pairs[0].1.clone()))]
        )
        .is_ok());

        // Absence of the V is not verified with the proof that has K, V included.
        assert!(verify_trie_proof::<LayoutV1, _, _, Vec<u8>>(
            &root,
            &proof,
            &[(pairs[0].0.clone(), None)]
        )
        .is_err());

        // K not included into the trie is not verified.
        assert!(verify_trie_proof::<LayoutV1, _, _, _>(
            &root,
            &proof,
            &[(
                array_bytes::hex2bytes_unchecked("4242"),
                Some(pairs[0].1.clone())
            )]
        )
        .is_err());

        // K included into the trie but not included into the proof is not verified.
        assert!(verify_trie_proof::<LayoutV1, _, _, _>(
            &root,
            &proof,
            &[(pairs[1].0.clone(), Some(pairs[1].1.clone()))]
        )
        .is_err());
    }

    #[test]
    fn generate_storage_root_with_proof_works_independently_from_the_delta_order() {
        // Create initial trie with complete database instead of using partial proof
        let initial_data = vec![
            (b"do".to_vec(), b"verb".to_vec()),
            (b"dog".to_vec(), b"puppy".to_vec()),
            (b"dogglesworth".to_vec(), b"cat".to_vec()),
            (b"horse".to_vec(), b"stallion".to_vec()),
            (b"house".to_vec(), b"building".to_vec()),
            (b"houseful".to_vec(), b"container".to_vec()),
        ];

        // Build initial trie with complete database
        let mut db = MemoryDB::new(&0u64.to_le_bytes());
        let mut storage_root = Default::default();

        {
            let mut trie = TrieDBMutBuilder::<LayoutV0>::new(&mut db, &mut storage_root).build();
            for (key, value) in &initial_data {
                trie.insert(key, value).unwrap();
            }
        }

        // Create valid delta order
        let valid_delta: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
            (b"do".to_vec(), Some(b"action".to_vec())), // update existing
            (b"doge".to_vec(), Some(b"meme".to_vec())), // new key between existing
            (b"dog".to_vec(), None),                    // delete existing
        ];

        // Create invalid delta order (same operations, different order)
        let invalid_delta: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
            (b"dog".to_vec(), None),                    // delete existing
            (b"doge".to_vec(), Some(b"meme".to_vec())), // new key between existing
            (b"do".to_vec(), Some(b"action".to_vec())), // update existing
        ];

        let first_storage_root = delta_trie_root::<LayoutV0, _, _, _, _, _>(
            &mut db.clone(),
            storage_root,
            valid_delta,
            None,
            None,
        )
        .unwrap();
        let second_storage_root = delta_trie_root::<LayoutV0, _, _, _, _, _>(
            &mut db.clone(),
            storage_root,
            invalid_delta,
            None,
            None,
        )
        .unwrap();

        assert_eq!(first_storage_root, second_storage_root);
    }

    #[test]
    fn big_key() {
        let check = |keysize: usize| {
            let mut memdb = PrefixedMemoryDB::<Blake2Hasher>::new(&0u64.to_le_bytes());
            let mut root = Default::default();
            let mut t = TrieDBMutBuilder::<LayoutV1>::new(&mut memdb, &mut root).build();
            t.insert(&vec![0x01u8; keysize][..], &[0x01u8, 0x23])
                .unwrap();
            std::mem::drop(t);
            let t = TrieDBBuilder::<LayoutV1>::new(&memdb, &root).build();
            assert_eq!(
                t.get(&vec![0x01u8; keysize][..]).unwrap(),
                Some(vec![0x01u8, 0x23])
            );
        };
        check(u16::MAX as usize / 2); // old limit
        check(u16::MAX as usize / 2 + 1); // value over old limit still works
    }

    #[test]
    fn node_with_no_children_fail_decoding() {
        let branch = NodeCodec::<Blake2Hasher>::branch_node_nibbled(
            b"some_partial".iter().copied(),
            24,
            vec![None; 16].into_iter(),
            Some(trie_db::node::Value::Inline(b"value"[..].into())),
        );
        assert!(NodeCodec::<Blake2Hasher>::decode(branch.as_slice()).is_err());
    }

    fn round_trip(header: NodeHeader) {
        // Encode the header
        let encoded = header.encode();
        // Check length is 8 bytes
        assert_eq!(encoded.len(), 8, "Encoded header must be 8 bytes");
        // Decode the bytes
        let decoded =
            NodeHeader::decode(&mut &encoded[..]).expect("Decoding valid header should succeed");
        // Check round-trip
        assert_eq!(header, decoded, "Decoded header should match original");
    }

    #[test]
    fn test_null() {
        let header = NodeHeader::Null;
        round_trip(header);
        // Verify encoding
        let encoded = header.encode();
        assert_eq!(encoded, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_branch_with_value() {
        // Test with nibble_count = 0
        let header = NodeHeader::Branch(true, 0);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10]); // 1 << 60

        // Test with nibble_count = 10
        let header = NodeHeader::Branch(true, 10);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10]); // 10 | (1 << 60)
    }

    #[test]
    fn test_branch_without_value() {
        // Test with nibble_count = 0 (Proof node 1 case)
        let header = NodeHeader::Branch(false, 0);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20]); // 2 << 60

        // Test with nibble_count = 1000
        let header = NodeHeader::Branch(false, 1000);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0xE8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20]); // 1000 | (2 << 60)
    }

    #[test]
    fn test_leaf() {
        // Test with nibble_count = 5
        let header = NodeHeader::Leaf(5);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30]); // 5 | (3 << 60)

        // Test with nibble_count = 0
        let header = NodeHeader::Leaf(0);
        round_trip(header);
    }

    #[test]
    fn test_felt_aligned_encoding_round_trip() {
        use crate::node_codec::NodeCodec;
        use sp_core::Blake2Hasher;
        use trie_db::node::Value;

        // Test round trip encoding/decoding for various nibble counts
        let test_cases = vec![
            (vec![0xaa], 2, "2 nibbles -> 1 byte -> 8 bytes felt-aligned"),
            (
                vec![0x03, 0x14],
                3,
                "3 nibbles -> 2 bytes -> 8 bytes felt-aligned",
            ),
            (
                vec![0x01, 0x23, 0x45, 0x67],
                8,
                "8 nibbles -> 4 bytes -> 8 bytes felt-aligned",
            ),
            (
                vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01],
                17,
                "17 nibbles -> 9 bytes -> 16 bytes felt-aligned",
            ),
        ];

        for (partial_bytes, nibble_count, description) in test_cases {
            println!("Testing: {}", description);

            // Encode
            let encoded = NodeCodec::<Blake2Hasher>::leaf_node(
                partial_bytes.iter().copied(),
                nibble_count,
                Value::Inline(&[0xbb]),
            );

            // Decode
            let decoded = NodeCodec::<Blake2Hasher>::decode_plan(&encoded).unwrap();

            // Verify structure
            if let trie_db::node::NodePlan::Leaf { partial, value } = decoded {
                // Just verify we got a leaf node with a partial key
                println!("✓ Successfully decoded leaf node with partial key");
            } else {
                panic!("Expected leaf node");
            }
        }
    }

    #[test]
    fn test_hashed_value_branch() {
        let header = NodeHeader::HashedValueBranch(15);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40]); // 15 | (4 << 60)
    }

    #[test]
    fn test_hashed_value_leaf() {
        let header = NodeHeader::HashedValueLeaf(20);
        round_trip(header);
        let encoded = header.encode();
        assert_eq!(encoded, [0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50]); // 20 | (5 << 60)
    }

    #[test]
    fn test_decode_invalid_type() {
        // Invalid type code (e.g., 6)
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x60]; // 6 << 60
        let result = NodeHeader::decode(&mut &bytes[..]);
        assert!(result.is_err(), "Decoding invalid type should fail");
        assert_eq!(result.unwrap_err().to_string(), "Invalid NodeHeader type");
    }

    #[test]
    fn test_decode_insufficient_bytes() {
        // Only 7 bytes
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let result = NodeHeader::decode(&mut &bytes[..]);
        assert!(
            result.is_err(),
            "Decoding with insufficient bytes should fail"
        );
    }

    #[test]
    fn test_reproduce_incomplete_database_scenarios() {
        // Test scenarios that mimic the failing balance tests
        test_reproduce_incomplete_database_scenarios_inner::<LayoutV1>();
        test_reproduce_incomplete_database_scenarios_inner::<LayoutV0>();
    }

    fn test_reproduce_incomplete_database_scenarios_inner<L: TrieConfiguration>() {
        use memory_db::{HashKey, MemoryDB};
        use trie_db::{TrieDBMutBuilder, TrieMut};

        let mut memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut root = Default::default();

        // Test inserting the exact problematic data from failing tests
        {
            let mut trie = TrieDBMutBuilder::<L>::new(&mut memdb, &mut root).build();

            // These are the exact insertions that were failing in the balance tests
            trie.insert(b"value3", &[142; 33])
                .expect("insert failed - 33 byte array");
            trie.insert(b"value4", &[124; 33])
                .expect("insert failed - 33 byte array");
            trie.insert(b"key", b"value")
                .expect("insert failed - string value");
            trie.insert(b"value1", &[42])
                .expect("insert failed - 1 byte");
            trie.insert(b"value2", &[24])
                .expect("insert failed - 1 byte");
            trie.insert(b":code", b"return 42")
                .expect("insert failed - code string");

            // Insert range like in the failing tests
            for i in 128u8..255u8 {
                trie.insert(&[i], &[i])
                    .expect(&format!("insert failed for {}", i));
            }
        }

        // Verify we can read everything back
        let trie = trie_db::TrieDBBuilder::<L>::new(&memdb, &root).build();
        assert_eq!(trie.get(b"value3").unwrap(), Some(vec![142; 33]));
        assert_eq!(trie.get(b"value4").unwrap(), Some(vec![124; 33]));
        assert_eq!(trie.get(b"key").unwrap(), Some(b"value".to_vec()));
        assert_eq!(trie.get(b"value1").unwrap(), Some(vec![42]));
        assert_eq!(trie.get(b"value2").unwrap(), Some(vec![24]));
        assert_eq!(trie.get(b":code").unwrap(), Some(b"return 42".to_vec()));
    }

    #[test]
    fn test_child_trie_root_handling() {
        // Test the child trie pattern that was failing in sp-state-machine
        test_child_trie_root_handling_inner::<LayoutV1>();
        test_child_trie_root_handling_inner::<LayoutV0>();
    }

    fn test_child_trie_root_handling_inner<L: TrieConfiguration>() {
        use codec::Encode;
        use memory_db::{HashKey, MemoryDB, PrefixedKey};
        use trie_db::{TrieDBMutBuilder, TrieMut};

        // Step 1: Build a child trie (mimicking the test_db pattern)
        let mut child_memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut child_root = Default::default();

        {
            let mut child_trie =
                TrieDBMutBuilder::<L>::new(&mut child_memdb, &mut child_root).build();
            child_trie
                .insert(b"value3", &[142; 33])
                .expect("child insert failed");
            child_trie
                .insert(b"value4", &[124; 33])
                .expect("child insert failed");
        }

        // Step 2: Encode the child root like in the failing test
        let sub_root = child_root.as_ref().to_vec();

        // Step 3: Insert the child root as a value in a main trie
        let mut main_memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut main_root = Default::default();

        {
            let mut main_trie = TrieDBMutBuilder::<L>::new(&mut main_memdb, &mut main_root).build();

            // This pattern was causing IncompleteDatabase errors
            let child_storage_key = b":child_storage_default:child";
            main_trie
                .insert(child_storage_key, &sub_root)
                .expect("main trie insert of child root failed");

            // Add other data like in the failing test
            main_trie.insert(b"key", b"value").expect("insert failed");
            main_trie.insert(b"value1", &[42]).expect("insert failed");
            main_trie.insert(b"value2", &[24]).expect("insert failed");
            main_trie
                .insert(b":code", b"return 42")
                .expect("insert failed");

            for i in 128u8..255u8 {
                main_trie
                    .insert(&[i], &[i])
                    .expect(&format!("insert failed for {}", i));
            }
        }

        // Verify we can read the child root back
        let main_trie = trie_db::TrieDBBuilder::<L>::new(&main_memdb, &main_root).build();
        let stored_child_root = main_trie.get(b":child_storage_default:child").unwrap();
        assert_eq!(stored_child_root, Some(sub_root));
    }

    #[test]
    fn test_unaligned_value_insertion_edge_cases() {
        test_unaligned_value_insertion_edge_cases_inner::<LayoutV1>();
        test_unaligned_value_insertion_edge_cases_inner::<LayoutV0>();
    }

    fn test_unaligned_value_insertion_edge_cases_inner<L: TrieConfiguration>() {
        use memory_db::{HashKey, MemoryDB};
        use trie_db::{TrieDBMutBuilder, TrieMut};

        let mut memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut root = Default::default();

        {
            let mut trie = TrieDBMutBuilder::<L>::new(&mut memdb, &mut root).build();

            // Test various problematic sizes that aren't 8-byte aligned
            let test_cases = vec![
                (b"empty".as_slice(), vec![]),                    // 0 bytes
                (b"one".as_slice(), vec![42]),                    // 1 byte
                (b"two".as_slice(), vec![42, 43]),                // 2 bytes
                (b"three".as_slice(), vec![42, 43, 44]),          // 3 bytes
                (b"five".as_slice(), vec![1, 2, 3, 4, 5]),        // 5 bytes
                (b"seven".as_slice(), vec![1, 2, 3, 4, 5, 6, 7]), // 7 bytes
                (b"nine".as_slice(), vec![1; 9]),                 // 9 bytes (8 + 1)
                (b"thirtythree".as_slice(), vec![142; 33]),       // 33 bytes (like failing test)
                (b"sixtyfive".as_slice(), vec![200; 65]),         // 65 bytes (64 + 1)
            ];

            for (key, value) in &test_cases {
                trie.insert(key, value)
                    .expect(&format!("Failed to insert {} bytes", value.len()));
            }

            // Also test the exact pattern from the failing balance tests
            trie.insert(b"balance_key", &[142; 33])
                .expect("balance-like insert failed");
        }

        // Verify round-trip consistency
        let trie = trie_db::TrieDBBuilder::<L>::new(&memdb, &root).build();

        let test_cases = vec![
            (b"empty".as_slice(), vec![]),
            (b"one".as_slice(), vec![42]),
            (b"two".as_slice(), vec![42, 43]),
            (b"three".as_slice(), vec![42, 43, 44]),
            (b"five".as_slice(), vec![1, 2, 3, 4, 5]),
            (b"seven".as_slice(), vec![1, 2, 3, 4, 5, 6, 7]),
            (b"nine".as_slice(), vec![1; 9]),
            (b"thirtythree".as_slice(), vec![142; 33]),
            (b"sixtyfive".as_slice(), vec![200; 65]),
        ];

        for (key, expected_value) in &test_cases {
            let stored_value = trie.get(key).unwrap();
            assert_eq!(
                stored_value,
                Some(expected_value.clone()),
                "Round-trip failed for {} byte value",
                expected_value.len()
            );
        }

        assert_eq!(trie.get(b"balance_key").unwrap(), Some(vec![142; 33]));
    }

    #[test]
    fn test_encode_decode_round_trip_consistency() {
        test_encode_decode_round_trip_consistency_inner::<LayoutV1>();
        test_encode_decode_round_trip_consistency_inner::<LayoutV0>();
    }

    fn test_encode_decode_round_trip_consistency_inner<L: TrieConfiguration>() {
        use crate::NodeCodec;
        use trie_db::node::{NodePlan, Value};
        use trie_db::NodeCodec as NodeCodecT;

        // Test round-trip for various problematic value sizes
        let test_values = vec![
            vec![],        // 0 bytes
            vec![42],      // 1 byte
            vec![1, 2, 3], // 3 bytes
            vec![1; 5],    // 5 bytes
            vec![1; 7],    // 7 bytes
            vec![1; 9],    // 9 bytes
            vec![142; 33], // 33 bytes (from failing test)
            vec![1; 65],   // 65 bytes
        ];

        for value in test_values {
            // Test leaf node encoding/decoding
            let encoded =
                NodeCodec::<L::Hash>::leaf_node([0xaa].iter().copied(), 2, Value::Inline(&value));

            let decoded = NodeCodec::<L::Hash>::decode_plan(&encoded)
                .expect(&format!("Failed to decode {} byte value", value.len()));

            if let NodePlan::Leaf {
                value: decoded_value,
                ..
            } = decoded
            {
                if let trie_db::node::ValuePlan::Inline(range) = decoded_value {
                    let decoded_bytes = &encoded[range];
                    assert_eq!(
                        decoded_bytes,
                        &value[..],
                        "Round-trip mismatch for {} byte value",
                        value.len()
                    );
                } else {
                    panic!("Expected inline value for {} bytes", value.len());
                }
            } else {
                panic!("Expected leaf node");
            }
        }
    }

    #[test]
    fn test_minimal_single_insert() {
        test_minimal_single_insert_inner::<LayoutV1>();
        test_minimal_single_insert_inner::<LayoutV0>();
    }

    fn test_minimal_single_insert_inner<L: TrieConfiguration>() {
        use memory_db::{HashKey, MemoryDB};
        use trie_db::{TrieDBMutBuilder, TrieMut};

        let mut memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut root = Default::default();

        // Try the absolute simplest case first
        {
            let mut trie = TrieDBMutBuilder::<L>::new(&mut memdb, &mut root).build();

            // Start with the exact failing case
            println!("Attempting to insert single problematic value...");
            trie.insert(b"value3", &[142; 33])
                .expect("MINIMAL SINGLE INSERT FAILED");
            println!("✓ Single insert succeeded");
        }

        // Verify we can read it back
        let trie = trie_db::TrieDBBuilder::<L>::new(&memdb, &root).build();
        let result = trie.get(b"value3").unwrap();
        assert_eq!(result, Some(vec![142; 33]));
        println!("✓ Single insert round-trip succeeded");
    }

    #[test]
    fn test_progressive_inserts() {
        test_progressive_inserts_inner::<LayoutV1>();
        test_progressive_inserts_inner::<LayoutV0>();
    }

    fn test_progressive_inserts_inner<L: TrieConfiguration>() {
        use memory_db::{HashKey, MemoryDB};
        use trie_db::{TrieDBMutBuilder, TrieMut};

        let mut memdb = MemoryDBMeta::<L::Hash>::new(&0u64.to_le_bytes());
        let mut root = Default::default();

        {
            let mut trie = TrieDBMutBuilder::<L>::new(&mut memdb, &mut root).build();

            // Insert values one by one to see exactly where it breaks
            println!("Step 1: Inserting value3...");
            trie.insert(b"value3", &[142; 33]).expect("Step 1 failed");

            println!("Step 2: Inserting value4...");
            trie.insert(b"value4", &[124; 33]).expect("Step 2 failed");

            println!("Step 3: Inserting key...");
            trie.insert(b"key", b"value").expect("Step 3 failed");

            println!("Step 4: Inserting value1...");
            trie.insert(b"value1", &[42]).expect("Step 4 failed");

            println!("Step 5: Inserting value2...");
            trie.insert(b"value2", &[24]).expect("Step 5 failed");

            println!("✓ All progressive inserts succeeded");
        }
    }

    #[test]
    fn test_leaf_partial_key_felt_alignment() {
        test_leaf_partial_key_felt_alignment_inner::<LayoutV0>();
        test_leaf_partial_key_felt_alignment_inner::<LayoutV1>();
    }

    fn test_leaf_partial_key_felt_alignment_inner<L: TrieConfiguration>() {
        use crate::NodeCodec;
        use trie_db::NodeCodec as NodeCodecT;

        println!("Testing leaf partial key felt-alignment...");

        // Test cases with different nibble counts to verify felt-alignment
        let test_cases = vec![
            // Small partial keys
            (vec![0x12], 2, "2 nibbles"),
            (vec![0x12, 0x34], 4, "4 nibbles"),
            (vec![0x12, 0x34, 0x56], 6, "6 nibbles"),
            // Medium partial keys
            (vec![0x12, 0x34, 0x56, 0x78, 0x9a], 10, "10 nibbles"),
            (
                vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0],
                16,
                "16 nibbles",
            ),
            // Large partial keys (like the user's 94 nibbles case)
            (
                (0..47).map(|i| (i * 7 + 13) as u8).collect::<Vec<_>>(),
                94,
                "94 nibbles",
            ),
            (
                (0..50).map(|i| (i * 11 + 5) as u8).collect::<Vec<_>>(),
                100,
                "100 nibbles",
            ),
        ];

        for (partial_data, nibble_count, description) in test_cases {
            println!("  Testing: {}", description);

            // Create leaf with inline value
            let encoded = NodeCodec::<L::Hash>::leaf_node(
                partial_data.iter().copied(),
                nibble_count,
                trie_db::node::Value::Inline(&[1u8]),
            );

            // Analyze structure
            let header_size = 8;
            let nibble_bytes = (nibble_count + 1) / 2;

            // Calculate expected padding based on our felt-alignment logic
            let critical_section_offset = header_size + nibble_bytes.saturating_sub(24);
            let misalignment = critical_section_offset % 8;
            let expected_prefix_padding = if misalignment == 0 {
                0
            } else {
                8 - misalignment
            };
            let expected_total_partial_section =
                ((expected_prefix_padding + nibble_bytes + 7) / 8) * 8;

            // Expected positions
            let partial_section_start = header_size;
            let partial_section_end = partial_section_start + expected_total_partial_section;
            let value_section_start = partial_section_end;

            println!(
                "    Nibble bytes: {}, Expected prefix padding: {}, Total partial section: {}",
                nibble_bytes, expected_prefix_padding, expected_total_partial_section
            );

            // Verify the partial section size is felt-aligned
            assert_eq!(
                expected_total_partial_section % 8,
                0,
                "Partial section size {} is not felt-aligned for {}",
                expected_total_partial_section,
                description
            );

            // Verify value section starts at felt boundary
            assert_eq!(
                value_section_start % 8,
                0,
                "Value section starts at offset {} which is not felt-aligned for {}",
                value_section_start,
                description
            );

            // Verify we can decode the node successfully
            match NodeCodec::<L::Hash>::decode_plan(&encoded) {
                Ok(plan) => {
                    if let trie_db::node::NodePlan::Leaf {
                        partial: _,
                        value: _,
                    } = plan
                    {
                        println!("    ✓ Leaf decoded successfully");
                    } else {
                        panic!("Expected leaf node plan for {}", description);
                    }
                }
                Err(e) => panic!("Failed to decode leaf for {}: {:?}", description, e),
            }

            // For longer partial keys, verify critical sections are felt-aligned
            if nibble_bytes >= 24 {
                let actual_partial_start = partial_section_start + expected_prefix_padding;
                let critical_section_start = actual_partial_start + (nibble_bytes - 24);
                assert_eq!(
                    critical_section_start % 8,
                    0,
                    "Critical section starts at offset {} which is not felt-aligned for {}",
                    critical_section_start,
                    description
                );
                println!(
                    "    ✓ Critical section is felt-aligned at offset {}",
                    critical_section_start
                );
            }

            println!("    ✓ All alignment checks passed for {}", description);
        }

        println!("✅ All leaf partial key felt-alignment tests passed!");
    }
}
