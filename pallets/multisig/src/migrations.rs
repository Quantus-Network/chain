//! Storage migrations for `pallet-multisig`.

extern crate alloc;

use crate::{
	pallet::{Config, Pallet, Proposals},
	ProposalStatus,
};
use codec::{Decode, Encode};
use core::marker::PhantomData;
use frame_support::{
	traits::{Get, UncheckedOnRuntimeUpgrade},
	weights::Weight,
};

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;

/// v0 -> v1: drop the removed `call_weight` field from every stored proposal.
///
/// Storage version 0 stored `call_weight: Weight` positionally between `call` and `expiry`.
/// Version 1 recomputes the inner call weight at execute time and no longer stores it. Without
/// this migration a v0 `Proposals` record fails to decode (stranding its deposit and per-signer
/// count), so this one-shot translate rewrites each record into the current layout.
pub mod v1 {
	use super::*;
	use crate::{
		pallet::{BoundedApprovalsOf, BoundedCallOf, ProposalDataOf},
		BalanceOf,
	};
	use frame_system::pallet_prelude::BlockNumberFor;

	/// Storage-version-0 layout of `ProposalData`, kept only to decode pre-upgrade records so the
	/// `call_weight` field can be dropped. `Encode` is derived for test fixtures.
	#[derive(Encode, Decode)]
	pub struct OldProposalData<AccountId, Balance, BlockNumber, BoundedCall, BoundedApprovals> {
		pub proposer: AccountId,
		pub call: BoundedCall,
		pub call_weight: Weight,
		pub expiry: BlockNumber,
		pub approvals: BoundedApprovals,
		pub deposit: Balance,
		pub status: ProposalStatus,
	}

	pub type OldProposalDataOf<T> = OldProposalData<
		<T as frame_system::Config>::AccountId,
		BalanceOf<T>,
		BlockNumberFor<T>,
		BoundedCallOf<T>,
		BoundedApprovalsOf<T>,
	>;

	/// Rewrites every [`Proposals`] entry from the v0 layout to v1 by dropping `call_weight`.
	pub struct DropCallWeight<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for DropCallWeight<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut count = 0u64;
			Proposals::<T>::translate::<OldProposalDataOf<T>, _>(|_addr, _id, old| {
				count = count.saturating_add(1);
				Some(ProposalDataOf::<T> {
					proposer: old.proposer,
					call: old.call,
					expiry: old.expiry,
					approvals: old.approvals,
					deposit: old.deposit,
					status: old.status,
				})
			});
			log::info!(
				target: "runtime::multisig",
				"Migrated {count} multisig proposal(s) to v1 (dropped call_weight)",
			);
			T::DbWeight::get().reads_writes(count, count)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(Vec::new())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			// Every record must decode under the current layout after the translation.
			let count = Proposals::<T>::iter().count();
			log::info!(target: "runtime::multisig", "post_upgrade: {count} proposal(s) decode under v1");
			Ok(())
		}
	}
}

/// Versioned v0 -> v1 migration. Runs [`v1::DropCallWeight`] only when the on-chain storage
/// version is 0, then bumps the on-chain storage version to 1.
pub type MigrateV0ToV1<T> = frame_support::migrations::VersionedMigration<
	0,
	1,
	v1::DropCallWeight<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;
