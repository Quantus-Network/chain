//! Storage migrations for `pallet-wormhole`.

extern crate alloc;

use crate::{Config, Pallet};
#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
use core::marker::PhantomData;
use frame_support::{
	traits::{Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};

/// v1 -> v2: remove the wormhole soundness counters.
pub mod v2 {
	use super::*;
	use frame_support::storage_alias;

	/// Alias for the removed `PotentialWormholeBalance` storage value, kept only so the
	/// migration can delete the key. The value type is irrelevant for deletion.
	#[storage_alias]
	pub(crate) type PotentialWormholeBalance<T: Config> = StorageValue<Pallet<T>, u128>;

	/// Alias for the removed `TotalWormholeExits` storage value (see above).
	#[storage_alias]
	pub(crate) type TotalWormholeExits<T: Config> = StorageValue<Pallet<T>, u128>;

	/// Deletes the soundness counters introduced in v1 (`PotentialWormholeBalance` and
	/// `TotalWormholeExits`). The mechanism they backed — capping cumulative exits at the
	/// sum that could plausibly have been deposited — only bounded the rate of a soundness
	/// attacker rather than stopping one, and has been removed.
	pub struct RemoveSoundnessCounters<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for RemoveSoundnessCounters<T> {
		fn on_runtime_upgrade() -> Weight {
			PotentialWormholeBalance::<T>::kill();
			TotalWormholeExits::<T>::kill();

			log::info!(
				target: "runtime::wormhole",
				"Removed wormhole soundness counters (PotentialWormholeBalance, TotalWormholeExits)",
			);

			T::DbWeight::get().writes(2)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			frame_support::ensure!(
				!PotentialWormholeBalance::<T>::exists() && !TotalWormholeExits::<T>::exists(),
				"soundness counters must be deleted"
			);
			Ok(())
		}
	}
}

/// Versioned v1 -> v2 migration. Runs [`v2::RemoveSoundnessCounters`] only when the on-chain
/// storage version is 1, then bumps the on-chain storage version to 2.
pub type MigrateV1ToV2<T> = frame_support::migrations::VersionedMigration<
	1,
	2,
	v2::RemoveSoundnessCounters<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

// There is intentionally no v1 -> v2 `TransferCount` re-keying migration. `TransferCount` is
// keyed on the Goldilocks-canonical recipient form, but that is enforced in `record_transfer`
// at write time (see `Pallet::canonical_leaf_recipient`), not by a storage migration — the
// layout is unchanged. An earlier design added a migration that merged any pre-upgrade entries
// keyed on a non-canonical recipient into their canonical key; it iterated the entire
// `TransferCount` map (an unbounded, permissionlessly-grown `StorageMap`) and collected matches
// into a `Vec`, all in a single upgrade block with no batching, cursor, or weight bound. Because
// the map size is attacker-controlled, an inflated map could push that migration past the block
// execution/memory budget and stall the upgrade. It was removed rather than bounded because it
// had no work to do on any real deployment: a live audit of the only pre-canonicalization chain
// (Planck testnet) found 0 non-canonical keys out of 2738 entries, fresh chains never run it,
// and `record_transfer` canonicalizes at write time so no new non-canonical keys can appear.
