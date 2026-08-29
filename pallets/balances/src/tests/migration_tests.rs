// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

//! Tests for balances storage migrations.

use super::*;
use crate::{migration::MigrateManyToTrackInactive, InactiveIssuance};
use frame_support::traits::{
	fungible::Inspect, Get, GetStorageVersion, OnRuntimeUpgrade, StorageVersion,
};

/// A configured account list that intentionally repeats the same ID. Runtime upgrades
/// feed this shape into `MigrateManyToTrackInactive` via `Get<Vec<_>>`.
struct DuplicateInactiveAccounts;
impl Get<Vec<u64>> for DuplicateInactiveAccounts {
	fn get() -> Vec<u64> {
		vec![1u64, 1u64, 1u64]
	}
}

/// `MigrateManyToTrackInactive` must sum each account's balance once. Counting a
/// repeated ID repeatedly overstates `InactiveIssuance` (capped only by
/// `TotalIssuance`), which permanently deflates `active_issuance()` after the
/// one-shot version bump.
#[test]
fn migrate_many_to_track_inactive_deduplicates_account_ids() {
	ExtBuilder::default().build_and_execute_with(|| {
		// Account 1 is the inactive target (balance 100). Account 2 keeps the
		// remaining active supply so an overstated deactivation is observable
		// below the TotalIssuance cap rather than clamped to TI.
		set_free_balance(1, 100);
		set_free_balance(2, 100);
		assert_eq!(TotalIssuance::<Test>::get(), 200);
		assert_eq!(InactiveIssuance::<Test>::get(), 0);

		// The migration only runs at on-chain version 0; the pallet's in-code
		// version is already 1, so reset explicitly.
		StorageVersion::new(0).put::<Balances>();
		assert_eq!(Balances::on_chain_storage_version(), 0);

		let _ = MigrateManyToTrackInactive::<Test, DuplicateInactiveAccounts>::on_runtime_upgrade();

		assert_eq!(Balances::on_chain_storage_version(), 1);
		assert_eq!(
			InactiveIssuance::<Test>::get(),
			100,
			"duplicate account IDs must not inflate the deactivated total"
		);
		assert_eq!(
			Balances::active_issuance(),
			100,
			"active issuance must remain the unique-account residual"
		);
	});
}
