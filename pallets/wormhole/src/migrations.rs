//! Storage migrations for `pallet-wormhole`.

extern crate alloc;

use crate::{Config, Pallet, PotentialWormholeBalance};
use core::marker::PhantomData;
use frame_support::{
	traits::{Currency, Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;

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
