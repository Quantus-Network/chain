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

//! Some utilities for helping access storage with arbitrary key types.

use crate::{
	hash::ReversibleStorageHasher,
	storage::{storage_prefix, unhashed},
	StorageHasher, Twox128,
};
use alloc::{vec, vec::Vec};
use codec::{Decode, Encode};

use super::PrefixIterator;

/// Utility to iterate through raw items in storage.
pub struct StorageIterator<T> {
	prefix: Vec<u8>,
	previous_key: Vec<u8>,
	drain: bool,
	_phantom: ::core::marker::PhantomData<T>,
}

impl<T> StorageIterator<T> {
	/// Construct iterator to iterate over map items in `module` for the map called `item`.
	#[deprecated(note = "Will be removed after July 2023; Please use the storage_iter or \
		storage_iter_with_suffix functions instead")]
	pub fn new(module: &[u8], item: &[u8]) -> Self {
		#[allow(deprecated)]
		Self::with_suffix(module, item, &[][..])
	}

	/// Construct iterator to iterate over map items in `module` for the map called `item`.
	#[deprecated(note = "Will be removed after July 2023; Please use the storage_iter or \
		storage_iter_with_suffix functions instead")]
	pub fn with_suffix(module: &[u8], item: &[u8], suffix: &[u8]) -> Self {
		let mut prefix = Vec::new();
		let storage_prefix = storage_prefix(module, item);
		prefix.extend_from_slice(&storage_prefix);
		prefix.extend_from_slice(suffix);
		let previous_key = prefix.clone();
		Self { prefix, previous_key, drain: false, _phantom: Default::default() }
	}

	/// Mutate this iterator into a draining iterator; items iterated are removed from storage.
	pub fn drain(mut self) -> Self {
		self.drain = true;
		self
	}
}

impl<T: Decode + Sized> Iterator for StorageIterator<T> {
	type Item = (Vec<u8>, T);

	fn next(&mut self) -> Option<(Vec<u8>, T)> {
		loop {
			let maybe_next = sp_io::storage::next_key(&self.previous_key)
				.filter(|n| n.starts_with(&self.prefix));
			break match maybe_next {
				Some(next) => {
					self.previous_key = next.clone();
					let maybe_value = frame_support::storage::unhashed::get::<T>(&next);
					match maybe_value {
						Some(value) => {
							if self.drain {
								frame_support::storage::unhashed::kill(&next);
							}
							Some((self.previous_key[self.prefix.len()..].to_vec(), value))
						},
						None => continue,
					}
				},
				None => None,
			}
		}
	}
}

/// Utility to iterate through raw items in storage.
pub struct StorageKeyIterator<K, T, H: ReversibleStorageHasher> {
	prefix: Vec<u8>,
	previous_key: Vec<u8>,
	drain: bool,
	_phantom: ::core::marker::PhantomData<(K, T, H)>,
}

impl<K, T, H: ReversibleStorageHasher> StorageKeyIterator<K, T, H> {
	/// Construct iterator to iterate over map items in `module` for the map called `item`.
	#[deprecated(note = "Will be removed after July 2023; Please use the storage_key_iter or \
		storage_key_iter_with_suffix functions instead")]
	pub fn new(module: &[u8], item: &[u8]) -> Self {
		#[allow(deprecated)]
		Self::with_suffix(module, item, &[][..])
	}

	/// Construct iterator to iterate over map items in `module` for the map called `item`.
	#[deprecated(note = "Will be removed after July 2023; Please use the storage_key_iter or \
		storage_key_iter_with_suffix functions instead")]
	pub fn with_suffix(module: &[u8], item: &[u8], suffix: &[u8]) -> Self {
		let mut prefix = Vec::new();
		let storage_prefix = storage_prefix(module, item);
		prefix.extend_from_slice(&storage_prefix);
		prefix.extend_from_slice(suffix);
		let previous_key = prefix.clone();
		Self { prefix, previous_key, drain: false, _phantom: Default::default() }
	}

	/// Mutate this iterator into a draining iterator; items iterated are removed from storage.
	pub fn drain(mut self) -> Self {
		self.drain = true;
		self
	}
}

impl<K: Decode + Sized, T: Decode + Sized, H: ReversibleStorageHasher> Iterator
	for StorageKeyIterator<K, T, H>
{
	type Item = (K, T);

	fn next(&mut self) -> Option<(K, T)> {
		loop {
			let maybe_next = sp_io::storage::next_key(&self.previous_key)
				.filter(|n| n.starts_with(&self.prefix));
			break match maybe_next {
				Some(next) => {
					self.previous_key = next.clone();
					let mut key_material = H::reverse(&next[self.prefix.len()..]);
					match K::decode(&mut key_material) {
						Ok(key) => {
							let maybe_value = frame_support::storage::unhashed::get::<T>(&next);
							match maybe_value {
								Some(value) => {
									if self.drain {
										frame_support::storage::unhashed::kill(&next);
									}
									Some((key, value))
								},
								None => continue,
							}
						},
						Err(_) => continue,
					}
				},
				None => None,
			}
		}
	}
}

/// Construct iterator to iterate over map items in `module` for the map called `item`.
pub fn storage_iter<T: Decode + Sized>(module: &[u8], item: &[u8]) -> PrefixIterator<(Vec<u8>, T)> {
	storage_iter_with_suffix(module, item, &[][..])
}

/// Construct iterator to iterate over map items in `module` for the map called `item`.
pub fn storage_iter_with_suffix<T: Decode + Sized>(
	module: &[u8],
	item: &[u8],
	suffix: &[u8],
) -> PrefixIterator<(Vec<u8>, T)> {
	let mut prefix = Vec::new();
	let storage_prefix = storage_prefix(module, item);
	prefix.extend_from_slice(&storage_prefix);
	prefix.extend_from_slice(suffix);
	let previous_key = prefix.clone();
	let closure = |raw_key_without_prefix: &[u8], mut raw_value: &[u8]| {
		let value = T::decode(&mut raw_value)?;
		Ok((raw_key_without_prefix.to_vec(), value))
	};

	PrefixIterator { prefix, previous_key, drain: false, closure, phantom: Default::default() }
}

/// Construct iterator to iterate over map items in `module` for the map called `item`.
pub fn storage_key_iter<K: Decode + Sized, T: Decode + Sized, H: ReversibleStorageHasher>(
	module: &[u8],
	item: &[u8],
) -> PrefixIterator<(K, T)> {
	storage_key_iter_with_suffix::<K, T, H>(module, item, &[][..])
}

/// Construct iterator to iterate over map items in `module` for the map called `item`.
pub fn storage_key_iter_with_suffix<
	K: Decode + Sized,
	T: Decode + Sized,
	H: ReversibleStorageHasher,
>(
	module: &[u8],
	item: &[u8],
	suffix: &[u8],
) -> PrefixIterator<(K, T)> {
	let mut prefix = Vec::new();
	let storage_prefix = storage_prefix(module, item);

	prefix.extend_from_slice(&storage_prefix);
	prefix.extend_from_slice(suffix);
	let previous_key = prefix.clone();
	let closure = |raw_key_without_prefix: &[u8], mut raw_value: &[u8]| {
		let mut key_material = H::reverse(raw_key_without_prefix);
		let key = K::decode(&mut key_material)?;
		let value = T::decode(&mut raw_value)?;
		Ok((key, value))
	};
	PrefixIterator { prefix, previous_key, drain: false, closure, phantom: Default::default() }
}

/// Get a particular value in storage by the `module`, the map's `item` name and the key `hash`.
pub fn have_storage_value(module: &[u8], item: &[u8], hash: &[u8]) -> bool {
	get_storage_value::<()>(module, item, hash).is_some()
}

/// Get a particular value in storage by the `module`, the map's `item` name and the key `hash`.
pub fn get_storage_value<T: Decode + Sized>(module: &[u8], item: &[u8], hash: &[u8]) -> Option<T> {
	let mut key = vec![0u8; 32 + hash.len()];
	let storage_prefix = storage_prefix(module, item);
	key[0..32].copy_from_slice(&storage_prefix);
	key[32..].copy_from_slice(hash);
	frame_support::storage::unhashed::get::<T>(&key)
}

/// Take a particular value in storage by the `module`, the map's `item` name and the key `hash`.
pub fn take_storage_value<T: Decode + Sized>(module: &[u8], item: &[u8], hash: &[u8]) -> Option<T> {
	let mut key = vec![0u8; 32 + hash.len()];
	let storage_prefix = storage_prefix(module, item);
	key[0..32].copy_from_slice(&storage_prefix);
	key[32..].copy_from_slice(hash);
	frame_support::storage::unhashed::take::<T>(&key)
}

/// Put a particular value into storage by the `module`, the map's `item` name and the key `hash`.
pub fn put_storage_value<T: Encode>(module: &[u8], item: &[u8], hash: &[u8], value: T) {
	let mut key = vec![0u8; 32 + hash.len()];
	let storage_prefix = storage_prefix(module, item);
	key[0..32].copy_from_slice(&storage_prefix);
	key[32..].copy_from_slice(hash);
	frame_support::storage::unhashed::put(&key, &value);
}

/// Remove all items under a storage prefix by the `module`, the map's `item` name and the key
/// `hash`.
#[deprecated = "Use `clear_storage_prefix` instead"]
pub fn remove_storage_prefix(module: &[u8], item: &[u8], hash: &[u8]) {
	let mut key = vec![0u8; 32 + hash.len()];
	let storage_prefix = storage_prefix(module, item);
	key[0..32].copy_from_slice(&storage_prefix);
	key[32..].copy_from_slice(hash);
	let _ = frame_support::storage::unhashed::clear_prefix(&key, None, None);
}

/// Attempt to remove all values under a storage prefix by the `module`, the map's `item` name and
/// the key `hash`.
///
/// All values in the client overlay will be deleted, if `maybe_limit` is `Some` then up to
/// that number of values are deleted from the client backend by seeking and reading that number of
/// storage values plus one. If `maybe_limit` is `None` then all values in the client backend are
/// deleted. This is potentially unsafe since it's an unbounded operation.
///
/// ## Cursors
///
/// The `maybe_cursor` parameter should be `None` for the first call to initial removal.
/// If the resultant `maybe_cursor` is `Some`, then another call is required to complete the
/// removal operation. This value must be passed in as the subsequent call's `maybe_cursor`
/// parameter. If the resultant `maybe_cursor` is `None`, then the operation is complete and no
/// items remain in storage provided that no items were added between the first calls and the
/// final call.
pub fn clear_storage_prefix(
	module: &[u8],
	item: &[u8],
	hash: &[u8],
	maybe_limit: Option<u32>,
	maybe_cursor: Option<&[u8]>,
) -> sp_io::MultiRemovalResults {
	let mut key = vec![0u8; 32 + hash.len()];
	let storage_prefix = storage_prefix(module, item);
	key[0..32].copy_from_slice(&storage_prefix);
	key[32..].copy_from_slice(hash);
	frame_support::storage::unhashed::clear_prefix(&key, maybe_limit, maybe_cursor)
}

/// Take a particular item in storage by the `module`, the map's `item` name and the key `hash`.
pub fn take_storage_item<K: Encode + Sized, T: Decode + Sized, H: StorageHasher>(
	module: &[u8],
	item: &[u8],
	key: K,
) -> Option<T> {
	take_storage_value(module, item, key.using_encoded(H::hash).as_ref())
}

/// Move a storage from a pallet prefix to another pallet prefix.
///
/// Keys used in pallet storages always start with:
/// `concat(twox_128(pallet_name), twox_128(storage_name))`.
///
/// This function will remove all value for which the key start with
/// `concat(twox_128(old_pallet_name), twox_128(storage_name))` and insert them at the key with
/// the start replaced by `concat(twox_128(new_pallet_name), twox_128(storage_name))`.
///
/// WARNING: This is an unbounded operation (see [`move_prefix`]). If the moved storage item can be
/// grown by users, use [`move_prefix_bounded`] from within a multi-block migration instead.
///
/// # Example
///
/// If a pallet named "my_example" has 2 storages named "Foo" and "Bar" and the pallet is renamed
/// "my_new_example_name", a migration can be:
/// ```
/// # use frame_support::storage::migration::move_storage_from_pallet;
/// # sp_io::TestExternalities::new_empty().execute_with(|| {
/// move_storage_from_pallet(b"Foo", b"my_example", b"my_new_example_name");
/// move_storage_from_pallet(b"Bar", b"my_example", b"my_new_example_name");
/// # })
/// ```
pub fn move_storage_from_pallet(
	storage_name: &[u8],
	old_pallet_name: &[u8],
	new_pallet_name: &[u8],
) {
	let new_prefix = storage_prefix(new_pallet_name, storage_name);
	let old_prefix = storage_prefix(old_pallet_name, storage_name);

	move_prefix(&old_prefix, &new_prefix);

	if let Some(value) = unhashed::get_raw(&old_prefix) {
		unhashed::put_raw(&new_prefix, &value);
		unhashed::kill(&old_prefix);
	}
}

/// Move all storages from a pallet prefix to another pallet prefix.
///
/// Keys used in pallet storages always start with:
/// `concat(twox_128(pallet_name), twox_128(storage_name))`.
///
/// This function will remove all value for which the key start with `twox_128(old_pallet_name)`
/// and insert them at the key with the start replaced by `twox_128(new_pallet_name)`.
///
/// NOTE: The value at the key `twox_128(old_pallet_name)` is not moved.
///
/// # Example
///
/// If a pallet named "my_example" has some storages and the pallet is renamed
/// "my_new_example_name", a migration can be:
/// ```
/// # use frame_support::storage::migration::move_pallet;
/// # sp_io::TestExternalities::new_empty().execute_with(|| {
/// move_pallet(b"my_example", b"my_new_example_name");
/// # })
/// ```
pub fn move_pallet(old_pallet_name: &[u8], new_pallet_name: &[u8]) {
	move_prefix(&Twox128::hash(old_pallet_name), &Twox128::hash(new_pallet_name))
}

/// Result of a bounded [`move_prefix_bounded`] call.
pub struct MovePrefixResult {
	/// The number of keys moved during this call.
	pub moved: u32,
	/// If `Some`, the move is incomplete and another call is required, passing this value back as
	/// the next call's `maybe_cursor`. If `None`, all matching keys have been moved.
	pub maybe_cursor: Option<Vec<u8>>,
}

/// Move all `(key, value)` after some prefix to the another prefix
///
/// This function will remove all value for which the key start with `from_prefix`
/// and insert them at the key with the start replaced by `to_prefix`.
///
/// NOTE: The value at the key `from_prefix` is not moved.
///
/// WARNING: This is an unbounded operation: it reads, deletes, and writes every key below
/// `from_prefix` in a single call. If the prefix can be grown by users, calling this in a
/// (mandatory) runtime-upgrade hook lets an attacker inflate the prefix beforehand and force the
/// first post-upgrade block to perform unbounded work. For such prefixes, use
/// [`move_prefix_bounded`] to move keys in bounded chunks from within a multi-block migration.
pub fn move_prefix(from_prefix: &[u8], to_prefix: &[u8]) {
	// Drive the bounded mover to completion in one shot. `u32::MAX` keys is effectively no bound,
	// preserving the historical all-at-once behaviour for callers that know the prefix is small.
	move_prefix_bounded(from_prefix, to_prefix, u32::MAX, None);
}

/// Move at most `limit` `(key, value)` pairs from `from_prefix` to `to_prefix`, resuming from
/// `maybe_cursor`.
///
/// This is the bounded counterpart to [`move_prefix`]: it moves up to `limit` keys per call and
/// returns a [`MovePrefixResult`] carrying how many keys were moved and, when the move is not yet
/// complete, a cursor to pass to the next call. This lets a multi-block migration spread the work
/// (and charge weight proportional to `moved`) instead of doing an unbounded move in a single
/// mandatory upgrade hook.
///
/// ## Cursors
///
/// Pass `None` as `maybe_cursor` for the first call. If the returned `maybe_cursor` is `Some`,
/// another call is required to continue; pass that value back in. A returned `maybe_cursor` of
/// `None` means the move is complete.
///
/// NOTE: The value at the key `from_prefix` is not moved.
pub fn move_prefix_bounded(
	from_prefix: &[u8],
	to_prefix: &[u8],
	limit: u32,
	maybe_cursor: Option<&[u8]>,
) -> MovePrefixResult {
	if from_prefix == to_prefix {
		return MovePrefixResult { moved: 0, maybe_cursor: None }
	}

	// The source and destination prefixes must be disjoint. If either is a prefix of the
	// other, the destination keys written during the drain can fall back inside the source
	// traversal domain, causing the move to reprocess its own output (unbounded work) or to
	// terminate leaving values under the source prefix (silent partial migration). A bad
	// prefix pair is a migration-author error, so fail loudly rather than corrupt storage.
	assert!(
		!to_prefix.starts_with(from_prefix) && !from_prefix.starts_with(to_prefix),
		"move_prefix: `from_prefix` and `to_prefix` must be disjoint (neither may be a prefix \
		 of the other), otherwise the move would overlap its own traversal domain",
	);

	let previous_key = maybe_cursor.map(|c| c.to_vec()).unwrap_or_else(|| from_prefix.to_vec());
	let mut iter = PrefixIterator::<(Vec<u8>, Vec<u8>)>::new(
		from_prefix.to_vec(),
		previous_key,
		|key, value| Ok((key.to_vec(), value.to_vec())),
	)
	.drain();

	let mut moved = 0u32;
	while moved < limit {
		match iter.next() {
			Some((key, value)) => {
				let full_key = [to_prefix, &key].concat();
				unhashed::put_raw(&full_key, &value);
				moved += 1;
			},
			// Ran out of matching keys before hitting the limit: the move is complete.
			None => return MovePrefixResult { moved, maybe_cursor: None },
		}
	}

	// Hit the limit. Peek past the last moved key to see whether any matching keys remain, and if
	// so hand back a cursor so the caller can resume. The last moved key was drained, but
	// `next_key` still returns the lexicographically next key regardless.
	let last = iter.last_raw_key().to_vec();
	let more_remain = sp_io::storage::next_key(&last)
		.map(|n| n.starts_with(from_prefix))
		.unwrap_or(false);
	MovePrefixResult { moved, maybe_cursor: more_remain.then_some(last) }
}

#[cfg(test)]
mod tests {
	use super::{
		move_pallet, move_prefix, move_prefix_bounded, move_storage_from_pallet, storage_iter,
		storage_key_iter,
	};
	use crate::{
		hash::StorageHasher,
		pallet_prelude::{StorageMap, StorageValue, Twox128, Twox64Concat},
	};
	use sp_io::TestExternalities;

	struct OldPalletStorageValuePrefix;
	impl frame_support::traits::StorageInstance for OldPalletStorageValuePrefix {
		const STORAGE_PREFIX: &'static str = "foo_value";
		fn pallet_prefix() -> &'static str {
			"my_old_pallet"
		}
	}
	type OldStorageValue = StorageValue<OldPalletStorageValuePrefix, u32>;

	struct OldPalletStorageMapPrefix;
	impl frame_support::traits::StorageInstance for OldPalletStorageMapPrefix {
		const STORAGE_PREFIX: &'static str = "foo_map";
		fn pallet_prefix() -> &'static str {
			"my_old_pallet"
		}
	}
	type OldStorageMap = StorageMap<OldPalletStorageMapPrefix, Twox64Concat, u32, u32>;

	struct NewPalletStorageValuePrefix;
	impl frame_support::traits::StorageInstance for NewPalletStorageValuePrefix {
		const STORAGE_PREFIX: &'static str = "foo_value";
		fn pallet_prefix() -> &'static str {
			"my_new_pallet"
		}
	}
	type NewStorageValue = StorageValue<NewPalletStorageValuePrefix, u32>;

	struct NewPalletStorageMapPrefix;
	impl frame_support::traits::StorageInstance for NewPalletStorageMapPrefix {
		const STORAGE_PREFIX: &'static str = "foo_map";
		fn pallet_prefix() -> &'static str {
			"my_new_pallet"
		}
	}
	type NewStorageMap = StorageMap<NewPalletStorageMapPrefix, Twox64Concat, u32, u32>;

	#[test]
	fn test_move_prefix() {
		TestExternalities::new_empty().execute_with(|| {
			OldStorageValue::put(3);
			OldStorageMap::insert(1, 2);
			OldStorageMap::insert(3, 4);

			move_prefix(&Twox128::hash(b"my_old_pallet"), &Twox128::hash(b"my_new_pallet"));

			assert_eq!(OldStorageValue::get(), None);
			assert_eq!(OldStorageMap::iter().collect::<Vec<_>>(), vec![]);
			assert_eq!(NewStorageValue::get(), Some(3));
			assert_eq!(NewStorageMap::iter().collect::<Vec<_>>(), vec![(1, 2), (3, 4)]);
		})
	}

	#[test]
	#[should_panic(expected = "must be disjoint")]
	fn move_prefix_rejects_destination_nested_in_source() {
		TestExternalities::new_empty().execute_with(|| {
			// `to_prefix` starts with `from_prefix`: moved keys would re-enter the source
			// traversal domain and get reprocessed. This must be rejected.
			crate::storage::unhashed::put_raw(b"aab", b"v");
			move_prefix(b"a", b"aa");
		})
	}

	#[test]
	#[should_panic(expected = "must be disjoint")]
	fn move_prefix_rejects_source_nested_in_destination() {
		TestExternalities::new_empty().execute_with(|| {
			crate::storage::unhashed::put_raw(b"aab", b"v");
			move_prefix(b"aa", b"a");
		})
	}

	#[test]
	fn test_move_prefix_bounded_moves_in_chunks() {
		TestExternalities::new_empty().execute_with(|| {
			// Fill the old map with more entries than a single chunk can hold.
			for i in 0..10u32 {
				OldStorageMap::insert(i, i * 10);
			}

			let from = Twox128::hash(b"my_old_pallet");
			let to = Twox128::hash(b"my_new_pallet");

			// Move in bounded chunks of 4, following the returned cursor to completion.
			let mut cursor: Option<Vec<u8>> = None;
			let mut total_moved = 0u32;
			let mut calls = 0u32;
			loop {
				let res = move_prefix_bounded(&from, &to, 4, cursor.as_deref());
				assert!(res.moved <= 4, "a chunk must never exceed the limit");
				total_moved += res.moved;
				calls += 1;
				assert!(calls < 100, "cursor must make progress and terminate");
				match res.maybe_cursor {
					Some(c) => cursor = Some(c),
					None => break,
				}
			}

			// Chunking actually happened (10 keys / 4 per chunk => multiple calls).
			assert!(calls > 1, "the move should have been split across multiple bounded calls");
			assert_eq!(total_moved, 10, "every key is moved exactly once across chunks");

			// Old prefix fully drained.
			assert_eq!(OldStorageMap::iter().count(), 0);

			// New prefix has all entries (Twox64Concat is not key-ordered, so sort before compare).
			let mut got = NewStorageMap::iter().collect::<Vec<_>>();
			got.sort();
			let expected = (0..10u32).map(|i| (i, i * 10)).collect::<Vec<_>>();
			assert_eq!(got, expected);
		})
	}

	#[test]
	fn test_move_storage() {
		TestExternalities::new_empty().execute_with(|| {
			OldStorageValue::put(3);
			OldStorageMap::insert(1, 2);
			OldStorageMap::insert(3, 4);

			move_storage_from_pallet(b"foo_map", b"my_old_pallet", b"my_new_pallet");

			assert_eq!(OldStorageValue::get(), Some(3));
			assert_eq!(OldStorageMap::iter().collect::<Vec<_>>(), vec![]);
			assert_eq!(NewStorageValue::get(), None);
			assert_eq!(NewStorageMap::iter().collect::<Vec<_>>(), vec![(1, 2), (3, 4)]);

			move_storage_from_pallet(b"foo_value", b"my_old_pallet", b"my_new_pallet");

			assert_eq!(OldStorageValue::get(), None);
			assert_eq!(OldStorageMap::iter().collect::<Vec<_>>(), vec![]);
			assert_eq!(NewStorageValue::get(), Some(3));
			assert_eq!(NewStorageMap::iter().collect::<Vec<_>>(), vec![(1, 2), (3, 4)]);
		})
	}

	#[test]
	fn test_move_pallet() {
		TestExternalities::new_empty().execute_with(|| {
			OldStorageValue::put(3);
			OldStorageMap::insert(1, 2);
			OldStorageMap::insert(3, 4);

			move_pallet(b"my_old_pallet", b"my_new_pallet");

			assert_eq!(OldStorageValue::get(), None);
			assert_eq!(OldStorageMap::iter().collect::<Vec<_>>(), vec![]);
			assert_eq!(NewStorageValue::get(), Some(3));
			assert_eq!(NewStorageMap::iter().collect::<Vec<_>>(), vec![(1, 2), (3, 4)]);
		})
	}

	#[test]
	fn test_storage_iter() {
		TestExternalities::new_empty().execute_with(|| {
			OldStorageValue::put(3);
			OldStorageMap::insert(1, 2);
			OldStorageMap::insert(3, 4);

			assert_eq!(
				storage_key_iter::<i32, i32, Twox64Concat>(b"my_old_pallet", b"foo_map")
					.collect::<Vec<_>>(),
				vec![(1, 2), (3, 4)],
			);

			assert_eq!(
				storage_iter(b"my_old_pallet", b"foo_map")
					.drain()
					.map(|t| t.1)
					.collect::<Vec<i32>>(),
				vec![2, 4],
			);
			assert_eq!(OldStorageMap::iter().collect::<Vec<_>>(), vec![]);

			// Empty because storage iterator skips over the entry under the first key
			assert_eq!(storage_iter::<i32>(b"my_old_pallet", b"foo_value").drain().next(), None);
			assert_eq!(OldStorageValue::get(), Some(3));
		});
	}
}
