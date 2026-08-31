//! Storage migrations for `pallet-treasury`.

extern crate alloc;

use crate::pallet::{Config, Pallet};
use core::marker::PhantomData;
use frame_support::{
	traits::{Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;

/// v0 -> v1: previously wrote a 50% `TreasuryPortion`. That storage item is gone;
/// this step is now a no-op version bump so v0 chains still advance to v1 and then
/// run [`v2`].
pub mod v1 {
	use super::*;

	pub struct Noop<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for Noop<T> {
		fn on_runtime_upgrade() -> Weight {
			log::info!(
				target: "runtime::treasury",
				"v1 no-op: TreasuryPortion is no longer part of the mining-reward split",
			);
			T::DbWeight::get().reads(0)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			Ok(())
		}
	}
}

/// v1 -> v2: drop the leftover `TreasuryPortion` key. Mining rewards are no longer
/// split.
pub mod v2 {
	use super::*;

	/// Pallet name as declared in the runtime (`TreasuryPallet`).
	const PALLET: &[u8] = b"TreasuryPallet";
	const ITEM: &[u8] = b"TreasuryPortion";

	pub struct KillTreasuryPortion<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for KillTreasuryPortion<T> {
		fn on_runtime_upgrade() -> Weight {
			let key = frame_support::storage::storage_prefix(PALLET, ITEM);
			frame_support::storage::unhashed::kill(&key);
			log::info!(
				target: "runtime::treasury",
				"Killed leftover TreasuryPortion storage",
			);
			T::DbWeight::get().writes(1)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let key = frame_support::storage::storage_prefix(PALLET, ITEM);
			frame_support::ensure!(
				!frame_support::storage::unhashed::exists(&key),
				"TreasuryPortion must be gone after the v2 migration"
			);
			Ok(())
		}
	}
}

/// Versioned v0 -> v1 migration. No-op besides the storage-version bump.
pub type MigrateV0ToV1<T> = frame_support::migrations::VersionedMigration<
	0,
	1,
	v1::Noop<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

/// Versioned v1 -> v2 migration. Kills leftover `TreasuryPortion` storage.
pub type MigrateV1ToV2<T> = frame_support::migrations::VersionedMigration<
	1,
	2,
	v2::KillTreasuryPortion<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;
