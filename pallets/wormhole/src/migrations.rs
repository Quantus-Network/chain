//! Storage migrations for `pallet-wormhole`.

extern crate alloc;

use crate::{Config, Pallet, PotentialWormholeBalance, TransferCount};
use alloc::vec::Vec;
use core::marker::PhantomData;
use frame_support::{
	traits::{Currency, Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};

/// v0 -> v1: introduce the wormhole soundness counters.
pub mod v1 {
	use super::*;

	/// Seeds [`PotentialWormholeBalance`] on an already-running chain so that wormhole deposits
	/// made before the soundness tracking existed remain exitable.
	///
	/// The seed is `total_issuance()`. Every balance held by an ambiguous (never-signed) address
	/// is necessarily backed by issued tokens, so total issuance is an upper bound on the value
	/// that could legitimately be exited. Seeding to it therefore guarantees the upgrade can never
	/// accidentally trip the soundness invariant on the first post-upgrade exit and brick the
	/// wormhole. As accounts reveal themselves the counter tightens toward the true ambiguous sum.
	///
	/// On a fresh chain this migration does not run (genesis sets the storage version to the
	/// current value), so `PotentialWormholeBalance` is instead seeded by the block-1
	/// `record_transfer` calls for genesis endowments.
	pub struct InitSoundnessCounters<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for InitSoundnessCounters<T> {
		fn on_runtime_upgrade() -> Weight {
			let seed = T::Currency::total_issuance();

			PotentialWormholeBalance::<T>::put(seed);

			log::info!(
				target: "runtime::wormhole",
				"Seeded PotentialWormholeBalance to total issuance: {:?}",
				seed,
			);

			// 1 read (total issuance) + 1 write (PotentialWormholeBalance).
			T::DbWeight::get().reads_writes(1, 1)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			frame_support::ensure!(
				PotentialWormholeBalance::<T>::get() >= T::Currency::total_issuance(),
				"PotentialWormholeBalance must be seeded to at least total issuance"
			);
			Ok(())
		}
	}
}

/// Versioned v0 -> v1 migration. Runs [`v1::InitSoundnessCounters`] only when the on-chain
/// storage version is 0, then bumps the on-chain storage version to 1.
pub type MigrateV0ToV1<T> = frame_support::migrations::VersionedMigration<
	0,
	1,
	v1::InitSoundnessCounters<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

/// v1 -> v2: re-key `TransferCount` onto the Goldilocks-canonical recipient form.
pub mod v2 {
	use super::*;
	use pallet_zk_tree::tree::canonicalize_account_bytes;

	fn max_count<T: Config>(a: T::TransferCount, b: T::TransferCount) -> T::TransferCount {
		if a.into() >= b.into() {
			a
		} else {
			b
		}
	}

	/// Merges any pre-upgrade `TransferCount` entries keyed on non-canonical recipients into
	/// their canonical key (max of the counts), then removes the raw keys.
	///
	/// Prospective `record_transfer` calls key on the canonical form. Without this merge, a
	/// pre-upgrade deposit to a non-canonical alias leaves count state under the raw key while
	/// post-upgrade deposits to that leaf-encoding class restart from the (empty) canonical
	/// key — recreating the leaf/nullifier collision the canonical re-keying was meant to
	/// fix. Taking the max count ensures the next deposit uses a fresh index past every
	/// already-committed leaf in the class. Already-colliding pre-upgrade leaves (if any)
	/// cannot be rewritten here; this only prevents *new* collisions.
	pub struct CanonicalizeTransferCountKeys<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for CanonicalizeTransferCountKeys<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut reads: u64 = 0;
			let mut writes: u64 = 0;
			let mut merged: u64 = 0;

			// Collect first — mutating the map while iterating is unsafe.
			let mut aliases: Vec<(T::WormholeAccountId, T::TransferCount)> = Vec::new();
			for (key, count) in TransferCount::<T>::iter() {
				reads = reads.saturating_add(1);
				let Ok(bytes) = <[u8; 32]>::try_from(key.as_ref()) else {
					continue;
				};
				let canonical = canonicalize_account_bytes(bytes);
				if canonical != bytes {
					aliases.push((key, count));
				}
			}

			for (raw_key, raw_count) in aliases {
				let Ok(bytes) = <[u8; 32]>::try_from(raw_key.as_ref()) else {
					continue;
				};
				let canonical_key: T::WormholeAccountId = canonicalize_account_bytes(bytes).into();
				let existing = TransferCount::<T>::get(&canonical_key);
				reads = reads.saturating_add(1);
				let merged_count = max_count::<T>(existing, raw_count);
				TransferCount::<T>::insert(&canonical_key, merged_count);
				TransferCount::<T>::remove(&raw_key);
				writes = writes.saturating_add(2);
				merged = merged.saturating_add(1);
			}

			log::info!(
				target: "runtime::wormhole",
				"Canonicalized TransferCount keys: merged {} non-canonical entries",
				merged,
			);

			T::DbWeight::get().reads_writes(reads, writes)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			use codec::Encode;
			let non_canonical = TransferCount::<T>::iter()
				.filter(|(key, _)| {
					<[u8; 32]>::try_from(key.as_ref())
						.map(|bytes| canonicalize_account_bytes(bytes) != bytes)
						.unwrap_or(false)
				})
				.count() as u64;
			Ok(non_canonical.encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use codec::Decode;
			let _pre: u64 = u64::decode(&mut &state[..])
				.map_err(|_| "failed to decode pre_upgrade TransferCount alias count")?;
			let remaining = TransferCount::<T>::iter()
				.filter(|(key, _)| {
					<[u8; 32]>::try_from(key.as_ref())
						.map(|bytes| canonicalize_account_bytes(bytes) != bytes)
						.unwrap_or(false)
				})
				.count();
			frame_support::ensure!(
				remaining == 0,
				"TransferCount must have no non-canonical keys after v2 migration"
			);
			Ok(())
		}
	}
}

/// Versioned v1 -> v2 migration. Runs [`v2::CanonicalizeTransferCountKeys`] only when the
/// on-chain storage version is 1, then bumps the on-chain storage version to 2.
pub type MigrateV1ToV2<T> = frame_support::migrations::VersionedMigration<
	1,
	2,
	v2::CanonicalizeTransferCountKeys<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;
