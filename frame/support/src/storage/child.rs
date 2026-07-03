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

//! Operation on runtime child storages.
//!
//! This module is a currently only a variant of unhashed with additional `child_info`.
// NOTE: could replace unhashed by having only one kind of storage (top trie being the child info
// of null length parent storage key).

use alloc::vec::Vec;
use codec::{Codec, Decode, Encode};
pub use sp_core::storage::{ChildInfo, ChildType, StateVersion};
pub use sp_io::{KillStorageResult, MultiRemovalResults};

/// Return the value of the item in storage under `key`, or `None` if there is no explicit entry.
pub fn get<T: Decode + Sized>(child_info: &ChildInfo, key: &[u8]) -> Option<T> {
	match child_info.child_type() {
		ChildType::ParentKeyId => {
			let storage_key = child_info.storage_key();
			sp_io::default_child_storage::get(storage_key, key).and_then(|v| {
				Decode::decode(&mut &v[..]).map(Some).unwrap_or_else(|_| {
					// TODO #3700: error should be handleable.
					log::error!(
						target: "runtime::storage",
						"Corrupted state in child trie at {:?}/{:?}",
						storage_key,
						key,
					);
					None
				})
			})
		},
	}
}

/// Return the value of the item in storage under `key`, or the type's default if there is no
/// explicit entry.
pub fn get_or_default<T: Decode + Sized + Default>(child_info: &ChildInfo, key: &[u8]) -> T {
	get(child_info, key).unwrap_or_default()
}

/// Return the value of the item in storage under `key`, or `default_value` if there is no
/// explicit entry.
pub fn get_or<T: Decode + Sized>(child_info: &ChildInfo, key: &[u8], default_value: T) -> T {
	get(child_info, key).unwrap_or(default_value)
}

/// Return the value of the item in storage under `key`, or `default_value()` if there is no
/// explicit entry.
pub fn get_or_else<T: Decode + Sized, F: FnOnce() -> T>(
	child_info: &ChildInfo,
	key: &[u8],
	default_value: F,
) -> T {
	get(child_info, key).unwrap_or_else(default_value)
}

/// Put `value` in storage under `key`.
pub fn put<T: Encode>(child_info: &ChildInfo, key: &[u8], value: &T) {
	match child_info.child_type() {
		ChildType::ParentKeyId => value.using_encoded(|slice| {
			sp_io::default_child_storage::set(child_info.storage_key(), key, slice)
		}),
	}
}

/// Remove `key` from storage, returning its value if it had an explicit entry or `None` otherwise.
pub fn take<T: Decode + Sized>(child_info: &ChildInfo, key: &[u8]) -> Option<T> {
	// Remove any explicit entry, even one whose bytes fail to decode (in which case `get`
	// returns `None`). Conditioning the removal on a successful decode would leave a
	// corrupted entry in place and break the `take_or*` contract that no explicit entry
	// remains on return.
	let had_explicit_entry = exists(child_info, key);
	let r = get(child_info, key);
	if had_explicit_entry {
		kill(child_info, key);
	}
	r
}

/// Remove `key` from storage, returning its value, or, if there was no explicit entry in storage,
/// the default for its type.
pub fn take_or_default<T: Codec + Sized + Default>(child_info: &ChildInfo, key: &[u8]) -> T {
	take(child_info, key).unwrap_or_default()
}

/// Return the value of the item in storage under `key`, or `default_value` if there is no
/// explicit entry. Ensure there is no explicit entry on return.
pub fn take_or<T: Codec + Sized>(child_info: &ChildInfo, key: &[u8], default_value: T) -> T {
	take(child_info, key).unwrap_or(default_value)
}

/// Return the value of the item in storage under `key`, or `default_value()` if there is no
/// explicit entry. Ensure there is no explicit entry on return.
pub fn take_or_else<T: Codec + Sized, F: FnOnce() -> T>(
	child_info: &ChildInfo,
	key: &[u8],
	default_value: F,
) -> T {
	take(child_info, key).unwrap_or_else(default_value)
}

/// Check to see if `key` has an explicit entry in storage.
pub fn exists(child_info: &ChildInfo, key: &[u8]) -> bool {
	match child_info.child_type() {
		ChildType::ParentKeyId =>
			sp_io::default_child_storage::exists(child_info.storage_key(), key),
	}
}

/// Remove all `storage_key` key/values
///
/// Deletes all keys from the overlay and up to `limit` keys from the backend if
/// it is set to `Some`. No limit is applied when `limit` is set to `None`.
///
/// The limit can be used to partially delete a child trie in case it is too large
/// to delete in one go (block).
///
/// # Note
///
/// Please note that keys that are residing in the overlay for that child trie when
/// issuing this call are all deleted without counting towards the `limit`. Only keys
/// written during the current block are part of the overlay. Deleting with a `limit`
/// mostly makes sense with an empty overlay for that child trie.
///
/// Calling this function multiple times per block for the same `storage_key` does
/// not make much sense because it is not cumulative when called inside the same block.
/// Use this function to distribute the deletion of a single child trie across multiple
/// blocks.
#[deprecated = "Use `clear_storage` instead"]
pub fn kill_storage(child_info: &ChildInfo, limit: Option<u32>) -> KillStorageResult {
	match child_info.child_type() {
		ChildType::ParentKeyId =>
			sp_io::default_child_storage::storage_kill(child_info.storage_key(), limit),
	}
}

/// Partially clear the child storage of each key-value pair.
///
/// # Limit
///
/// A *limit* should always be provided through `maybe_limit`. This is one fewer than the
/// maximum number of backend iterations which may be done by this operation and as such
/// represents the maximum number of backend deletions which may happen. A *limit* of zero
/// implies that no keys will be deleted, though there may be a single iteration done.
///
/// The limit can be used to partially delete storage items in case it is too large or costly
/// to delete all in a single operation.
///
/// # Cursor
///
/// A *cursor* may be passed in to this operation with `maybe_cursor`. `None` should only be
/// passed once (in the initial call) for any attempt to clear storage. In general, subsequent calls
/// operating on the same prefix should pass `Some` and this value should be equal to the
/// previous call result's `maybe_cursor` field. The only exception to this is when you can
/// guarantee that the subsequent call is in a new block; in this case the previous call's result
/// cursor need not be passed in and a `None` may be passed instead. This exception may be useful
/// then making this call solely from a block-hook such as `on_initialize`.

/// Returns [`MultiRemovalResults`] to inform about the result. Once the resultant `maybe_cursor`
/// field is `None`, then no further items remain to be deleted.
///
/// NOTE: After the initial call for any given child storage, it is important that no keys further
/// keys are inserted. If so, then they may or may not be deleted by subsequent calls.
///
/// # Note
///
/// Please note that keys which are residing in the overlay for the child are deleted without
/// counting towards the `limit`.
///
/// When a `limit` is given, the pre/post key walks used to attribute overlay-only removals are
/// themselves bounded (to `limit` plus a fixed allowance), so a child trie holding many more
/// keys than the limit cannot force unbounded counting work. If the trie holds more keys than
/// the bound, overlay-only removals may be under-reported (never over-reported) in
/// `unique`/`loops`; the `backend` count is always exact.
pub fn clear_storage(
	child_info: &ChildInfo,
	maybe_limit: Option<u32>,
	_maybe_cursor: Option<&[u8]>,
) -> MultiRemovalResults {
	// The legacy `storage_kill` host function only reports keys removed from the backend; it
	// also deletes overlay-resident keys (those written earlier in the current block) but does
	// not count them. Count the visible keys before and after so those overlay-only removals
	// are still reflected in `unique`/`loops`, which callers may use for weight or cleanup
	// accounting.
	//
	// Both walks stop after `up_to` keys: without a bound, a limited clear of a large
	// (potentially user-growable) child trie would perform O(total keys) `next_key` host calls
	// just for accounting — the very unbounded-work pattern the `limit` exists to prevent.
	fn key_count(child_info: &ChildInfo, up_to: u32) -> u32 {
		let mut count: u32 = if exists(child_info, &[]) { 1 } else { 0 };
		let mut previous_key = Vec::new();
		while count < up_to {
			match sp_io::default_child_storage::next_key(child_info.storage_key(), &previous_key)
			{
				Some(next_key) => {
					count = count.saturating_add(1);
					previous_key = next_key;
				},
				None => break,
			}
		}
		count
	}

	// Extra headroom on top of `limit` for the counting walks. Overlay-resident keys (written
	// earlier in the current block) do not count towards `limit`, so allow enumerating this many
	// keys beyond it before giving up on exact overlay attribution. When both walks saturate at
	// the cap the computed overlay removals collapse to zero rather than going negative.
	const OVERLAY_ATTRIBUTION_CAP: u32 = 1024;
	// With no limit the host call itself is already O(total keys), so exact counting does not
	// change the asymptotic cost.
	let count_cap =
		maybe_limit.map_or(u32::MAX, |limit| limit.saturating_add(OVERLAY_ATTRIBUTION_CAP));

	let keys_before = key_count(child_info, count_cap);

	// TODO: Once the network has upgraded to include the new host functions, this code can be
	// enabled.
	// sp_io::default_child_storage::storage_kill(prefix, maybe_limit, maybe_cursor)
	let r = match child_info.child_type() {
		ChildType::ParentKeyId =>
			sp_io::default_child_storage::storage_kill(child_info.storage_key(), maybe_limit),
	};
	use sp_io::KillStorageResult::*;
	let (maybe_cursor, backend) = match r {
		AllRemoved(db) => (None, db),
		SomeRemaining(db) => (Some(child_info.storage_key().to_vec()), db),
	};

	// Overlay-only removals = total keys deleted (before - after) minus those the host call
	// already accounted for in `backend`.
	let keys_after = key_count(child_info, count_cap);
	let overlay = keys_before.saturating_sub(keys_after).saturating_sub(backend);
	let total = backend.saturating_add(overlay);

	MultiRemovalResults { maybe_cursor, backend, unique: total, loops: total }
}

/// Ensure `key` has no explicit entry in storage.
pub fn kill(child_info: &ChildInfo, key: &[u8]) {
	match child_info.child_type() {
		ChildType::ParentKeyId => {
			sp_io::default_child_storage::clear(child_info.storage_key(), key);
		},
	}
}

/// Get a Vec of bytes from storage.
pub fn get_raw(child_info: &ChildInfo, key: &[u8]) -> Option<Vec<u8>> {
	match child_info.child_type() {
		ChildType::ParentKeyId => sp_io::default_child_storage::get(child_info.storage_key(), key),
	}
}

/// Put a raw byte slice into storage.
pub fn put_raw(child_info: &ChildInfo, key: &[u8], value: &[u8]) {
	match child_info.child_type() {
		ChildType::ParentKeyId =>
			sp_io::default_child_storage::set(child_info.storage_key(), key, value),
	}
}

/// Calculate current child root value.
pub fn root(child_info: &ChildInfo, version: StateVersion) -> Vec<u8> {
	match child_info.child_type() {
		ChildType::ParentKeyId =>
			sp_io::default_child_storage::root(child_info.storage_key(), version),
	}
}

/// Return the length in bytes of the value without reading it. `None` if it does not exist.
pub fn len(child_info: &ChildInfo, key: &[u8]) -> Option<u32> {
	match child_info.child_type() {
		ChildType::ParentKeyId => {
			let mut buffer = [0; 0];
			sp_io::default_child_storage::read(child_info.storage_key(), key, &mut buffer, 0)
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use sp_io::TestExternalities;

	#[test]
	fn take_removes_undecodable_explicit_entry() {
		TestExternalities::new_empty().execute_with(|| {
			let child_info = ChildInfo::new_default(b"test-child");
			let key = b"slot";
			// `[0x02]` is not a valid SCALE `bool` (only `0x00`/`0x01` decode).
			put_raw(&child_info, key, &[0x02]);
			assert!(exists(&child_info, key));

			// `take` on a corrupted entry returns `None` (decode fails) but must still
			// clear the explicit entry.
			let taken = take::<bool>(&child_info, key);
			assert_eq!(taken, None);
			assert!(!exists(&child_info, key), "corrupt explicit entry must be removed by take");
		});
	}

	#[test]
	fn take_or_clears_corrupt_entry_and_returns_default() {
		TestExternalities::new_empty().execute_with(|| {
			let child_info = ChildInfo::new_default(b"test-child");
			let key = b"one-shot";
			put_raw(&child_info, key, &[0x02]);

			// `take_or` must honor its contract: no explicit entry remains afterwards.
			assert!(!take_or::<bool>(&child_info, key, false));
			assert!(!exists(&child_info, key));
			// A subsequent emptiness check now sees the slot as free.
			assert!(!exists(&child_info, key));
		});
	}

	#[test]
	fn clear_storage_counts_overlay_only_removals() {
		TestExternalities::new_empty().execute_with(|| {
			let child_info = ChildInfo::new_default(b"overlay-child");
			// Stage several keys in the overlay within this block.
			for i in 0u8..5 {
				put_raw(&child_info, &[i], &[i]);
			}

			// Even with a backend limit of zero, the overlay keys are deleted and must be
			// reported in `unique`/`loops` instead of being counted as zero work.
			let res = clear_storage(&child_info, Some(0), None);
			assert_eq!(res.unique, 5);
			assert_eq!(res.loops, 5);
			for i in 0u8..5 {
				assert!(!exists(&child_info, &[i]));
			}
		});
	}

	#[test]
	fn clear_storage_counting_is_bounded_for_large_tries() {
		let child_info = ChildInfo::new_default(b"big-child");
		let mut ext = TestExternalities::new_empty();
		// Commit more backend keys than the counting cap (limit + 1024) can enumerate.
		ext.execute_with(|| {
			for i in 0u32..1_500 {
				put_raw(&child_info, &i.to_le_bytes(), &[1]);
			}
		});
		ext.commit_all().unwrap();

		ext.execute_with(|| {
			// Stage a few overlay-only keys on top.
			for i in 0u8..5 {
				put_raw(&child_info, &[0xff, i], &[i]);
			}

			// The counting walks saturate at the cap, so overlay attribution degrades to zero
			// (documented under-reporting) instead of going negative or walking the whole trie.
			let res = clear_storage(&child_info, Some(10), None);
			assert_eq!(res.backend, 10, "backend removals are reported exactly");
			assert_eq!(res.unique, 10, "saturated counting must not inflate `unique`");
			for i in 0u8..5 {
				assert!(!exists(&child_info, &[0xff, i]), "overlay keys are still deleted");
			}
		});
	}

	#[test]
	fn take_still_returns_and_clears_valid_entry() {
		TestExternalities::new_empty().execute_with(|| {
			let child_info = ChildInfo::new_default(b"test-child");
			let key = b"slot";
			put(&child_info, key, &123u32);
			assert_eq!(take::<u32>(&child_info, key), Some(123));
			assert!(!exists(&child_info, key));
			// Taking an absent key returns `None` and remains absent.
			assert_eq!(take::<u32>(&child_info, key), None);
		});
	}
}
