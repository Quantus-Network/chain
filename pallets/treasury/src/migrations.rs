//! Storage migrations for `pallet-treasury`.

extern crate alloc;

use crate::pallet::{Config, Pallet, TreasuryPortion};
use core::marker::PhantomData;
use frame_support::{
	traits::{Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};
use sp_runtime::Permill;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;

/// v0 -> v1: tokenomics change to a 50/50 treasury/miner split of block rewards.
pub mod v1 {
	use super::*;

	/// The treasury portion enforced by this migration (50% treasury / 50% miner).
	pub fn treasury_portion() -> Permill {
		Permill::from_percent(50)
	}

	/// Sets [`TreasuryPortion`] to 50% on an already-running chain.
	///
	/// This accompanies the emission-divisor change in the runtime config: together they
	/// implement the updated tokenomics where roughly half the mineable supply is emitted
	/// over the first four years, split equally between miners and the treasury.
	pub struct SetTreasuryPortionToHalf<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for SetTreasuryPortionToHalf<T> {
		fn on_runtime_upgrade() -> Weight {
			let portion = treasury_portion();
			TreasuryPortion::<T>::put(portion);

			log::info!(
				target: "runtime::treasury",
				"Set TreasuryPortion to {:?} (50/50 treasury/miner split)",
				portion,
			);

			T::DbWeight::get().reads_writes(0, 1)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			frame_support::ensure!(
				TreasuryPortion::<T>::get() == Some(treasury_portion()),
				"TreasuryPortion must be 50% after the v1 migration"
			);
			Ok(())
		}
	}
}

/// Versioned v0 -> v1 migration. Runs [`v1::SetTreasuryPortionToHalf`] only when the on-chain
/// storage version is 0, then bumps the on-chain storage version to 1.
pub type MigrateV0ToV1<T> = frame_support::migrations::VersionedMigration<
	0,
	1,
	v1::SetTreasuryPortionToHalf<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;
