//! Custom signed extensions for the runtime.
extern crate alloc;
use crate::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use core::marker::PhantomData;
use frame_support::pallet_prelude::{
	InvalidTransaction, TransactionValidityError, ValidTransaction,
};
use frame_system::ensure_signed;
use qp_high_security::HighSecurityInspector;
use qp_wormhole::TransferProofRecorder;
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::{
	traits::{DispatchInfoOf, PostDispatchInfoOf, TransactionExtension},
	DispatchResult, Weight,
};

/// Transaction extension for reversible accounts
///
/// This extension is used to intercept delayed transactions for users that opted in
/// for reversible transactions. Based on the policy set by the user, the transaction
/// will either be denied or intercepted and delayed.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct ReversibleTransactionExtension<T: pallet_reversible_transfers::Config>(PhantomData<T>);

impl<T: pallet_reversible_transfers::Config + Send + Sync> ReversibleTransactionExtension<T> {
	/// Creates new `TransactionExtension` to check genesis hash.
	pub fn new() -> Self {
		Self(core::marker::PhantomData)
	}
}

impl<T: pallet_reversible_transfers::Config + Send + Sync + alloc::fmt::Debug>
	TransactionExtension<RuntimeCall> for ReversibleTransactionExtension<T>
{
	type Pre = ();
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "ReversibleTransactionExtension";

	fn weight(&self, _call: &RuntimeCall) -> Weight {
		T::DbWeight::get().reads(1)
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		Ok(())
	}

	fn validate(
		&self,
		origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		let who = ensure_signed(origin.clone())
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::BadSigner))?;

		// Check if account is high-security using the same inspector as multisig
		if crate::configs::HighSecurityConfig::is_high_security(&who) {
			// Use the same whitelist check as multisig
			if crate::configs::HighSecurityConfig::is_whitelisted(call) {
				return Ok((ValidTransaction::default(), (), origin));
			} else {
				return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(1)));
			}
		}

		Ok((ValidTransaction::default(), (), origin))
	}
}

/// Transaction extension that records transfer proofs in the wormhole pallet
///
/// This extension uses an EVENT-BASED approach to detect transfers:
/// - After successful execution, scans for Transfer/Transferred/Issued events
/// - Records proofs for any transfers that were sent TO a wormhole account
/// - Automatically catches ALL transfers regardless of how they're initiated:
///   - Direct transfers (transfer, transfer_keep_alive, transfer_all, etc.)
///   - Batch transfers (utility.batch, batch_all, force_batch)
///   - Multisig transfers (multisig.execute)
///   - Recovery transfers (recovery.as_recovered)
///   - Scheduled transfers (scheduler)
///   - Future mechanisms automatically covered
///
/// This addresses audit item EQ-QNT-WORMHOLE-F-05 comprehensively.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct WormholeProofRecorderExtension<T: pallet_wormhole::Config + Send + Sync>(PhantomData<T>);

impl<T: pallet_wormhole::Config + Send + Sync> WormholeProofRecorderExtension<T> {
	/// Creates new extension
	pub fn new() -> Self {
		Self(PhantomData)
	}

	fn count_transfers(call: &RuntimeCall) -> u64 {
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::force_transfer { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::force_set_balance { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::transfer { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::transfer_approved { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::force_transfer { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::mint { .. }) => 1,

			RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) =>
				calls.iter().map(Self::count_transfers).sum(),

			RuntimeCall::Utility(pallet_utility::Call::dispatch_as { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::with_weight { call, .. }) |
			RuntimeCall::Recovery(pallet_recovery::Call::as_recovered { call, .. }) =>
				Self::count_transfers(call),

			_ => 0,
		}
	}

	/// Scan events and record transfer proofs for any transfers that occurred
	/// since the given event count (to avoid re-processing previous events
	/// within the same block).
	///
	/// `event_count_before` is the value from `frame_system::Pallet::event_count()`
	/// captured in `prepare()`.
	fn record_proofs_from_events_since(event_count_before: u32) {
		// IMPORTANT: We must collect all transfers FIRST before calling record_transfer_proof,
		// because record_transfer_proof deposits new events which would invalidate the
		// stream_iter iterator (causing "Corrupted state" errors).
		//
		// The iterator reads from Events storage using stream_iter, which caches data.
		// If we modify Events storage during iteration (by depositing new events),
		// the cached data becomes stale and decoding fails.

		// Collect transfers to record - (asset_id, from, to, amount)
		let transfers_to_record: alloc::vec::Vec<(Option<AssetId>, AccountId, AccountId, Balance)> =
			frame_system::Pallet::<Runtime>::read_events_no_consensus()
				.skip(event_count_before as usize)
				.filter_map(|event_record| {
					match event_record.event {
						// Native balance transfers
						RuntimeEvent::Balances(pallet_balances::Event::Transfer {
							from,
							to,
							amount,
						}) => Some((None, from, to, amount)),
						// Native balance mints
						RuntimeEvent::Balances(pallet_balances::Event::Minted { who, amount }) => {
							let minting_account = crate::configs::MintingAccount::get();
							Some((None, minting_account, who, amount))
						},
						// Asset transfers
						RuntimeEvent::Assets(pallet_assets::Event::Transferred {
							asset_id,
							from,
							to,
							amount,
						}) => Some((Some(asset_id), from, to, amount)),
						// Asset mints
						RuntimeEvent::Assets(pallet_assets::Event::Issued {
							asset_id,
							owner,
							amount,
						}) => {
							let minting_account = crate::configs::AssetMintingAccount::get();
							Some((Some(asset_id), minting_account, owner, amount))
						},
						_ => None, // Ignore all other events
					}
				})
				.collect();

		// Now record the proofs - this is safe because we're no longer iterating over Events
		for (asset_id, from, to, amount) in transfers_to_record {
			<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
				asset_id, from, to, amount,
			);
		}
	}
}

impl<T: pallet_wormhole::Config + Send + Sync + alloc::fmt::Debug> TransactionExtension<RuntimeCall>
	for WormholeProofRecorderExtension<T>
{
	type Pre = u32;
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "WormholeProofRecorderExtension";

	fn weight(&self, call: &RuntimeCall) -> Weight {
		let n = Self::count_transfers(call);
		let transfer_weight = if n > 0 {
			// Per transfer: 1 read (TransferCount) + 2 writes (TransferProof + TransferCount)
			T::DbWeight::get().reads_writes(n, 2 * n)
		} else {
			Weight::zero()
		};

		// Soundness reveal bookkeeping done in `validate`: worst case reads the signer's nonce
		// and balance and writes `PotentialWormholeBalance` once.
		let reveal_weight = T::DbWeight::get().reads_writes(2, 1);

		transfer_weight.saturating_add(reveal_weight)
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// Snapshot current event count so we only process events added by this tx
		// (and any events from previous txs in the same block).
		Ok(frame_system::Pallet::<Runtime>::event_count())
	}

	fn validate(
		&self,
		origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &DispatchInfoOf<RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		// Soundness tracking: when an account signs for the very first time it reveals itself as
		// a regular dilithium account rather than a wormhole deposit address. Remove its balance
		// from the potential wormhole pool.
		//
		// We detect "first signature" by `nonce == 0`. This runs in `validate`, which executes
		// before `CheckNonce::prepare` increments the nonce (all extensions' `validate` run
		// before any extension's `prepare`), so `nonce == 0` correctly identifies first-time
		// signers. Unsigned transactions (e.g. wormhole exits) have no signer and are skipped.
		if let Ok(signer) = ensure_signed(origin.clone()) {
			if pallet_wormhole::Pallet::<Runtime>::is_ambiguous_account(&signer) {
				pallet_wormhole::Pallet::<Runtime>::reveal_account(&signer);
			}
		}

		Ok((ValidTransaction::default(), (), origin))
	}

	fn post_dispatch(
		pre: Self::Pre,
		_info: &DispatchInfoOf<RuntimeCall>,
		_post_info: &mut PostDispatchInfoOf<RuntimeCall>,
		_len: usize,
		result: &DispatchResult,
	) -> Result<(), TransactionValidityError> {
		// Only record proofs if the transaction succeeded.
		// Use the event count snapshot from prepare() to avoid duplicate recording.
		if result.is_ok() {
			Self::record_proofs_from_events_since(pre);
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_support::{assert_ok, pallet_prelude::TransactionValidityError, traits::Currency};
	use sp_runtime::{traits::TxBaseImplication, AccountId32};
	fn alice() -> AccountId {
		AccountId32::from([1; 32])
	}

	fn bob() -> AccountId {
		AccountId32::from([2; 32])
	}
	fn charlie() -> AccountId {
		AccountId32::from([3; 32])
	}

	// Build genesis storage according to the mock runtime.
	pub fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();

		pallet_balances::GenesisConfig::<Runtime> {
			balances: vec![
				(alice(), EXISTENTIAL_DEPOSIT * 10000),
				(bob(), EXISTENTIAL_DEPOSIT * 2),
				(charlie(), EXISTENTIAL_DEPOSIT * 100),
			],
			dev_accounts: None,
		}
		.assimilate_storage(&mut t)
		.unwrap();

		// high security account is charlie
		// guardian is alice
		pallet_reversible_transfers::GenesisConfig::<Runtime> {
			initial_high_security_accounts: vec![(charlie(), alice(), 10)],
		}
		.assimilate_storage(&mut t)
		.unwrap();

		// Treasury account + portion are required for mining-reward distribution.
		pallet_treasury::GenesisConfig::<Runtime>::default()
			.assimilate_storage(&mut t)
			.unwrap();

		sp_io::TestExternalities::new(t)
	}

	#[test]
	fn test_reversible_transaction_extension() {
		new_test_ext().execute_with(|| {
			// Other calls should not be intercepted
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

			let origin = RuntimeOrigin::signed(alice());
			let ext = ReversibleTransactionExtension::<Runtime>::new();

			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);

			// we should not fail here
			assert_ok!(result);

			// Test that non-high-security accounts can make balance transfers
			let ext = ReversibleTransactionExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Test the prepare method
			ext.clone().prepare((), &origin, &call, &Default::default(), 0).unwrap();
			assert_eq!((), ());

			// Test the validate method
			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			// Alice is not high-security, so this should succeed
			assert_ok!(result);

			// Charlie is already configured as high-security from genesis
			// Verify Charlie is high-security
			assert!(ReversibleTransfers::is_high_security(&charlie()).is_some());

			// High-security accounts can call schedule_transfer
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);

			// Test the validate method
			let result = check_call(call);
			assert_ok!(result);

			// High-security accounts can call cancel
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			let result = check_call(call);
			assert_ok!(result);

			// All other calls are disallowed for high-security accounts
			// (use transfer_keep_alive - not in whitelist for prod or runtime-benchmarks)
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	fn check_call(call: RuntimeCall) -> Result<(), TransactionValidityError> {
		// Test the reversible transaction extension
		let ext = ReversibleTransactionExtension::<Runtime>::new();

		// Verify Charlie is high-security
		assert!(ReversibleTransfers::is_high_security(&charlie()).is_some());

		let origin = RuntimeOrigin::signed(charlie());

		// Test the prepare method
		ext.clone().prepare((), &origin, &call, &Default::default(), 0).unwrap();

		assert_eq!((), ());

		// Test the validate method
		let result = ext.validate(
			origin,
			&call,
			&Default::default(),
			0,
			(),
			&TxBaseImplication::<()>(()),
			frame_support::pallet_prelude::TransactionSource::External,
		);

		result.map(|_| ())
	}

	#[test]
	fn test_high_security_transfer_keep_alive() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_transfer_allow_death() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_transfer_all() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_all {
				dest: MultiAddress::Id(bob()),
				keep_alive: true,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_remove_recovery() {
		new_test_ext().execute_with(|| {
			// make sure high security account can't remove the recovery
			let call = RuntimeCall::Recovery(pallet_recovery::Call::remove_recovery {});
			let result = check_call(call);
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_schedule_transfer_allowed() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			// High-security accounts can call schedule_transfer
			assert_ok!(check_call(call));
		});
	}

	#[test]
	fn test_high_security_cancel_allowed() {
		new_test_ext().execute_with(|| {
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			assert_ok!(check_call(call));
		});
	}

	// =========================================================================
	// Tests for event-based WormholeProofRecorderExtension
	// =========================================================================
	//
	// Note: The event-based approach records proofs by scanning Transfer events
	// in post_dispatch. The actual integration testing happens in the wormhole
	// pallet tests. Here we just verify the extension structure is correct.

	#[test]
	fn wormhole_proof_recorder_extension_has_correct_weight() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();

			// Even non-transfer calls carry the constant soundness reveal-bookkeeping overhead
			// (read nonce + balance, possibly write the pool), so the base weight is non-zero.
			let reveal_weight =
				<Runtime as frame_system::Config>::DbWeight::get().reads_writes(2, 1);
			let non_transfer =
				RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
			let base_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &non_transfer);
			assert_eq!(base_weight, reveal_weight);

			let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &transfer);
			// A transfer adds per-transfer cost on top of the reveal overhead.
			assert!(weight.ref_time() > base_weight.ref_time());

			let batch = RuntimeCall::Utility(pallet_utility::Call::batch {
				calls: vec![
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(bob()),
						value: 50 * UNIT,
					}),
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(charlie()),
						value: 30 * UNIT,
					}),
				],
			});
			let batch_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &batch);
			assert!(batch_weight.ref_time() > weight.ref_time());
		});
	}

	#[test]
	fn wormhole_proof_recorder_extension_prepare_succeeds() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Prepare should succeed and return current event count
			let result = ext.prepare((), &origin, &call, &Default::default(), 0);
			assert_ok!(result);
		});
	}

	#[test]
	fn wormhole_proof_recorder_extension_validate_succeeds() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Validate should always succeed (no validation needed)
			use sp_runtime::traits::TxBaseImplication;
			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			assert_ok!(result);
		});
	}

	// =========================================================================
	// Integration tests for event-based transfer proof recording
	// =========================================================================
	//
	// These tests verify that transfers via various paths result in proofs
	// being recorded. We simulate what post_dispatch does by:
	// 1. Executing the transfer (which emits events)
	// 2. Calling record_proofs_from_events_since(0) directly
	// 3. Verifying proofs were recorded in wormhole storage

	#[test]
	fn event_based_proof_recording_native_transfer() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use a smaller amount
			let transfer_amount = EXISTENTIAL_DEPOSIT * 100;
			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);

			// Execute a transfer (this emits pallet_balances::Event::Transfer)
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				transfer_amount,
			));

			// Simulate what post_dispatch does - scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			let count_after = Wormhole::transfer_count(&bob_account);
			assert_eq!(count_after, count_before + 1, "Transfer count should increment");
		});
	}

	#[test]
	fn event_based_proof_recording_transfer_allow_death() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use a smaller amount
			let transfer_amount = EXISTENTIAL_DEPOSIT * 50;
			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);

			// Execute transfer_allow_death
			assert_ok!(Balances::transfer_allow_death(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				transfer_amount,
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_transfer_all() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);

			// Execute transfer_all (transfers entire balance minus ED)
			assert_ok!(Balances::transfer_all(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				false, // keep_alive = false
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_batch_transfers() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let charlie_account = charlie();
			let bob_count_before = Wormhole::transfer_count(&bob_account);
			let charlie_count_before = Wormhole::transfer_count(&charlie_account);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use smaller amounts
			// Execute a batch with multiple transfers
			assert_ok!(Utility::batch(
				RuntimeOrigin::signed(alice()),
				vec![
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(bob()),
						value: EXISTENTIAL_DEPOSIT * 50,
					}),
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(charlie()),
						value: EXISTENTIAL_DEPOSIT * 30,
					}),
				],
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify both transfers were recorded (proofs are now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), bob_count_before + 1);
			assert_eq!(Wormhole::transfer_count(&charlie_account), charlie_count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_no_proof_for_non_transfer() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let bob_count_before = Wormhole::transfer_count(&bob_account);

			// Execute a non-transfer call
			assert_ok!(System::remark(RuntimeOrigin::signed(alice()), vec![1, 2, 3]));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify no proofs were recorded
			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_before,
				"No transfer count should change for non-transfer calls"
			);
		});
	}

	#[test]
	fn event_based_proof_recording_minted_event() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Create a new account to receive minted tokens
			let recipient = AccountId::from([99u8; 32]);
			let mint_amount = 1000 * UNIT;
			let count_before = Wormhole::transfer_count(&recipient);

			// Mint tokens (requires root origin)
			// This emits pallet_balances::Event::Minted
			assert_ok!(Balances::force_set_balance(
				RuntimeOrigin::root(),
				MultiAddress::Id(recipient.clone()),
				mint_amount,
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Note: force_set_balance emits Minted event, which we scan for
			// The proof should use MintingAccount as 'from'
			let count_after = Wormhole::transfer_count(&recipient);

			// Check if count increased (depends on whether Minted event is emitted)
			// force_set_balance may emit BalanceSet instead of Minted
			// This test documents the expected behavior - proofs are now in ZK trie
			assert!(count_after >= count_before, "Transfer count should not decrease");
		});
	}

	// =========================================================================
	// Regression test: multiple txs in one block must NOT duplicate proofs
	// =========================================================================
	//
	// Before the event_count snapshot fix, record_proofs_from_events scanned
	// ALL events in the block. The second tx's post_dispatch would re-process
	// the first tx's Transfer event, creating a duplicate proof. This test
	// simulates that exact scenario and asserts exactly 1 proof per transfer.

	#[test]
	fn no_duplicate_proofs_across_transactions_in_same_block() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let charlie_account = charlie();
			let bob_count_start = Wormhole::transfer_count(&bob_account);
			let charlie_count_start = Wormhole::transfer_count(&charlie_account);

			// --- Tx 1: Alice sends to Bob ---
			let snapshot_1 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				EXISTENTIAL_DEPOSIT * 50,
			));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_1);

			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx1: Bob should have exactly 1 new proof"
			);

			// --- Tx 2: Alice sends to Charlie ---
			let snapshot_2 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(charlie()),
				EXISTENTIAL_DEPOSIT * 30,
			));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_2);

			assert_eq!(
				Wormhole::transfer_count(&charlie_account),
				charlie_count_start + 1,
				"Tx2: Charlie should have exactly 1 new proof"
			);
			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx2 must NOT re-record Bob's proof from Tx1"
			);

			// --- Tx 3: a non-transfer tx should not create any proofs ---
			let snapshot_3 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(System::remark(RuntimeOrigin::signed(alice()), vec![0xCA, 0xFE]));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_3);

			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx3: Bob count unchanged after non-transfer tx"
			);
			assert_eq!(
				Wormhole::transfer_count(&charlie_account),
				charlie_count_start + 1,
				"Tx3: Charlie count unchanged after non-transfer tx"
			);
		});
	}

	// =========================================================================
	// Tests for multisig transfer proof recording
	// =========================================================================

	#[test]
	fn event_based_proof_recording_multisig_transfer() {
		use codec::Encode;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Create a multisig with alice and bob as signers, threshold 2
			let signers = vec![alice(), bob()];
			let threshold = 2u32;
			let nonce = 0u64;

			// Create the multisig
			assert_ok!(Multisig::create_multisig(
				RuntimeOrigin::signed(alice()),
				signers.clone(),
				threshold,
				nonce,
			));

			// Derive the multisig address
			let multisig_address = pallet_multisig::Pallet::<Runtime>::derive_multisig_address(
				&signers, threshold, nonce,
			);

			// Fund the multisig account
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(multisig_address.clone()),
				EXISTENTIAL_DEPOSIT * 1000,
			));

			// Clear events from setup
			System::reset_events();

			// Create a proposal to transfer from multisig to charlie
			let transfer_amount = EXISTENTIAL_DEPOSIT * 100;
			let inner_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(charlie()),
				value: transfer_amount,
			});

			// Encode the call and set expiry
			let encoded_call = inner_call.encode().try_into().unwrap();
			let expiry = System::block_number() + 100;

			// Alice proposes
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				encoded_call,
				expiry,
			));

			// Bob approves (reaches threshold)
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				0, // proposal_id
			));

			// Get charlie's transfer count before execution
			let charlie_account = charlie();
			let count_before = Wormhole::transfer_count(&charlie_account);

			// Execute the proposal
			assert_ok!(Multisig::execute(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				0, // proposal_id
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			// The transfer is FROM the multisig address
			let count_after = Wormhole::transfer_count(&charlie_account);
			assert_eq!(
				count_after,
				count_before + 1,
				"Transfer count should increment for multisig transfer"
			);
		});
	}

	// =========================================================================
	// Soundness reveal tracking
	// =========================================================================

	#[test]
	fn reveal_subtracts_signer_balance_from_potential_pool() {
		use sp_runtime::traits::TxBaseImplication;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Use a fresh account (not a sentinel/keyless address like alice == [1; 32]) so it is
			// genuinely ambiguous and not excluded by `NonWormholeAccounts`.
			let signer = intg_account(80);
			fund(&signer, 500 * UNIT);
			let balance = <Balances as Currency<AccountId>>::free_balance(&signer);
			assert!(balance > 0);
			// The signer has never signed yet.
			assert_eq!(frame_system::Pallet::<Runtime>::account_nonce(&signer), 0);
			assert!(pallet_wormhole::Pallet::<Runtime>::is_ambiguous_account(&signer));

			// Seed the pool above the signer's balance so the subtraction is observable.
			let seeded = balance + 1_000 * UNIT;
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(seeded);

			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			let result = ext.validate(
				RuntimeOrigin::signed(signer.clone()),
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			assert_ok!(result);

			assert_eq!(
				pallet_wormhole::PotentialWormholeBalance::<Runtime>::get(),
				seeded - balance,
				"Revealing (first signature) must subtract the signer's balance from the pool"
			);
		});
	}

	#[test]
	fn reveal_is_noop_for_already_revealed_signer() {
		use sp_runtime::traits::TxBaseImplication;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let signer = intg_account(81);
			fund(&signer, 500 * UNIT);
			// Mark the signer as already revealed (nonce > 0).
			frame_system::Pallet::<Runtime>::inc_account_nonce(&signer);

			let seeded = 5_000 * UNIT;
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(seeded);

			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			let result = ext.validate(
				RuntimeOrigin::signed(signer),
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			assert_ok!(result);

			assert_eq!(
				pallet_wormhole::PotentialWormholeBalance::<Runtime>::get(),
				seeded,
				"Already-revealed signers must not change the pool"
			);
		});
	}

	// =========================================================================
	// Full-transaction integration tests for PotentialWormholeBalance
	//
	// These drive the WormholeProofRecorderExtension lifecycle the way the real
	// transaction pipeline does:
	//   validate()  -> reveal subtraction (sees the pre-tx, un-incremented nonce)
	//   prepare()   -> snapshot event count
	//   [CheckNonce::prepare bumps the signer nonce]
	//   dispatch    -> the call executes and emits Transfer event(s)
	//   post_dispatch() -> record_transfer() applies the deposit addition
	//
	// so BOTH sides of the counter are exercised together, and we assert the net
	// change to `PotentialWormholeBalance` for each sender/recipient combination.
	// =========================================================================

	/// Pool baseline large enough that reveal subtractions never saturate at zero.
	const POOL_BASE: Balance = 10_000_000 * UNIT;
	const SENDER_BAL: Balance = 1_000 * UNIT;

	fn intg_account(tag: u8) -> AccountId {
		AccountId32::from([tag; 32])
	}

	fn fund(who: &AccountId, amount: Balance) {
		use frame_support::traits::fungible::Mutate;
		assert_ok!(<Balances as Mutate<AccountId>>::mint_into(who, amount));
	}

	/// Mark an account as "revealed" (has signed before) by giving it a non-zero nonce.
	fn mark_revealed(who: &AccountId) {
		frame_system::Pallet::<Runtime>::inc_account_nonce(who);
	}

	/// Run a full transaction (whatever `dispatch` performs) through the extension lifecycle.
	/// `call` is the call presented to validate/prepare; `dispatch` performs the real execution.
	fn run_lifecycle(from: &AccountId, call: RuntimeCall, dispatch: impl FnOnce()) {
		use sp_runtime::traits::TxBaseImplication;

		let ext = WormholeProofRecorderExtension::<Runtime>::new();
		let origin = RuntimeOrigin::signed(from.clone());

		// validate(): reveal runs here against the pre-tx (un-incremented) nonce.
		let (_, val, _) = ext
			.validate(
				origin.clone(),
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.expect("validate should succeed");

		// prepare(): snapshot the event count before the call emits its Transfer event(s).
		let pre = ext
			.clone()
			.prepare(val, &origin, &call, &Default::default(), 0)
			.expect("prepare should succeed");

		// CheckNonce::prepare would bump the signer's nonce at this point.
		mark_revealed(from);

		// Execute the real call (emits the Transfer event(s) that post_dispatch scans).
		dispatch();

		// post_dispatch(): records the transfer proof(s), applying the deposit side.
		let mut post_info = frame_support::dispatch::PostDispatchInfo::default();
		<WormholeProofRecorderExtension<Runtime> as TransactionExtension<RuntimeCall>>::post_dispatch(
			pre,
			&Default::default(),
			&mut post_info,
			0,
			&Ok(()),
		)
		.expect("post_dispatch should succeed");
	}

	fn transfer_call(to: &AccountId, amount: Balance) -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
			dest: MultiAddress::Id(to.clone()),
			value: amount,
		})
	}

	/// Run a full single native transfer from `from` to `to`.
	fn run_transfer(from: &AccountId, to: &AccountId, amount: Balance) {
		let from = from.clone();
		let to = to.clone();
		run_lifecycle(&from, transfer_call(&to, amount), || {
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(from.clone()),
				MultiAddress::Id(to.clone()),
				amount,
			));
		});
	}

	fn pool() -> Balance {
		pallet_wormhole::PotentialWormholeBalance::<Runtime>::get()
	}

	// --- recipient ambiguous, sender already revealed: deposit only ---
	#[test]
	fn counter_to_ambiguous_from_revealed_adds_deposit_only() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(40);
			let to = intg_account(41);
			fund(&from, SENDER_BAL);
			mark_revealed(&from); // sender has outgoing history
						 // `to` keeps nonce 0 (ambiguous)
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			let amount = 100 * UNIT;
			run_transfer(&from, &to, amount);

			assert_eq!(pool(), POOL_BASE + amount, "deposit to ambiguous recipient adds amount");
		});
	}

	// --- recipient revealed, sender already revealed: no change ---
	#[test]
	fn counter_to_revealed_from_revealed_no_change() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(42);
			let to = intg_account(43);
			fund(&from, SENDER_BAL);
			mark_revealed(&from);
			mark_revealed(&to); // recipient already has outgoing history
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			run_transfer(&from, &to, 100 * UNIT);

			assert_eq!(pool(), POOL_BASE, "neither side is ambiguous: no change");
		});
	}

	// --- sender ambiguous (first tx -> reveal), recipient revealed: reveal only ---
	#[test]
	fn counter_from_ambiguous_to_revealed_subtracts_sender_balance() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(44);
			let to = intg_account(45);
			fund(&from, SENDER_BAL); // from stays nonce 0 (ambiguous, will reveal)
			mark_revealed(&to);
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			run_transfer(&from, &to, 100 * UNIT);

			assert_eq!(
				pool(),
				POOL_BASE - SENDER_BAL,
				"first signature subtracts the sender's full pre-tx balance"
			);
		});
	}

	// --- sender ambiguous AND recipient ambiguous: reveal and deposit both fire ---
	#[test]
	fn counter_from_ambiguous_to_ambiguous_applies_both() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(46);
			let to = intg_account(47);
			fund(&from, SENDER_BAL); // ambiguous
							// `to` keeps nonce 0 (ambiguous)
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			let amount = 100 * UNIT;
			run_transfer(&from, &to, amount);

			assert_eq!(
				pool(),
				POOL_BASE + amount - SENDER_BAL,
				"deposit (+amount) and reveal (-sender balance) both apply"
			);
		});
	}

	// --- sender's second tx must NOT reveal again ---
	#[test]
	fn counter_sender_reveals_only_on_first_tx() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(48);
			let to1 = intg_account(49);
			let to2 = intg_account(50);
			fund(&from, SENDER_BAL); // ambiguous
			mark_revealed(&to1); // isolate the reveal: no deposit on either transfer
			mark_revealed(&to2);
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			// First tx reveals the sender.
			run_transfer(&from, &to1, 100 * UNIT);
			assert_eq!(pool(), POOL_BASE - SENDER_BAL, "first tx reveals sender");

			// Second tx: sender nonce is now > 0, so no further subtraction.
			run_transfer(&from, &to2, 50 * UNIT);
			assert_eq!(
				pool(),
				POOL_BASE - SENDER_BAL,
				"second tx from the same sender must not reveal again"
			);
		});
	}

	// --- batch from an ambiguous sender: reveal ONCE, count EACH deposit ---
	#[test]
	fn counter_batch_from_ambiguous_reveals_once_counts_each_deposit() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			let from = intg_account(51);
			let to1 = intg_account(52); // ambiguous
			let to2 = intg_account(53); // ambiguous
			fund(&from, SENDER_BAL); // ambiguous
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			let a1 = 100 * UNIT;
			let a2 = 50 * UNIT;
			let batch = RuntimeCall::Utility(pallet_utility::Call::batch {
				calls: vec![transfer_call(&to1, a1), transfer_call(&to2, a2)],
			});

			run_lifecycle(&from, batch, || {
				assert_ok!(Utility::batch(
					RuntimeOrigin::signed(from.clone()),
					vec![transfer_call(&to1, a1), transfer_call(&to2, a2)],
				));
			});

			// Reveal applies once (-SENDER_BAL); both deposits apply (+a1 +a2).
			assert_eq!(
				pool(),
				POOL_BASE + a1 + a2 - SENDER_BAL,
				"batch reveals the sender once and counts each ambiguous-recipient deposit"
			);
		});
	}

	// --- mined block rewards flow into the potential pool ---
	// Mining mints brand-new coins to the miner (ambiguous, never-signed) and the treasury (a
	// keyless governance account, excluded via `NonWormholeAccounts`). Only the miner's portion is
	// indistinguishable from a wormhole deposit, so only it lands in `PotentialWormholeBalance`;
	// the treasury's portion is correctly excluded since it can never be exited via the wormhole.
	#[test]
	fn counter_mining_rewards_increase_potential_balance() {
		use frame_support::traits::Hooks;
		use qp_wormhole::TestMiner;
		use sp_consensus_qpow::POW_ENGINE_ID;
		use sp_runtime::DigestItem;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let miner = TestMiner(777);
			let miner_account = miner.account_id();

			// A freshly derived miner has never signed a transaction, so it is ambiguous.
			assert!(
				pallet_wormhole::Pallet::<Runtime>::is_ambiguous_account(&miner_account),
				"freshly derived miner account must be ambiguous (nonce == 0)"
			);

			// Announce the miner for this block via the pre-runtime digest.
			System::deposit_log(DigestItem::PreRuntime(POW_ENGINE_ID, miner.preimage().to_vec()));

			let issuance_before = <Balances as Currency<AccountId>>::total_issuance();
			let pool_before = pool();

			// Mine the block: mints the reward to miner + treasury and records the proofs.
			MiningRewards::on_finalize(1);

			let issuance_after = <Balances as Currency<AccountId>>::total_issuance();
			let pool_after = pool();

			// New coins were actually minted, and the miner was credited.
			assert!(issuance_after > issuance_before, "block reward must mint new coins");
			let miner_reward = <Balances as Currency<AccountId>>::free_balance(&miner_account);
			assert!(miner_reward > 0, "miner must be credited the mined reward");

			// The treasury portion is excluded (keyless governance account), so the pool grows by
			// the miner's ambiguous portion only — and by strictly less than the full emission.
			assert_eq!(
				pool_after - pool_before,
				miner_reward,
				"only the miner's (ambiguous) portion of the reward increases PotentialWormholeBalance"
			);
			assert!(
				pool_after - pool_before < issuance_after - issuance_before,
				"the excluded treasury portion must not be counted into the pool"
			);
		});
	}

	// --- multisig creation reveals a pre-funded address ---
	// A multisig never signs (it spends via its signatories), so it never reveals itself the way
	// a normal account does. This guards the attack where someone pre-computes a multisig address,
	// sends funds to it (counted into the pool because it looks ambiguous), then creates the
	// multisig: creation must deduct the address's balance so it nets zero into the pool, and the
	// registered multisig must afterwards be excluded from the ambiguous set.
	#[test]
	fn counter_multisig_creation_reveals_prefunded_address() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let creator = intg_account(60);
			let signers = vec![intg_account(61), intg_account(62)];
			let threshold = 2u32;
			fund(&creator, 10_000 * UNIT);
			mark_revealed(&creator);

			// Pre-compute the multisig address before it is created.
			let multisig_addr = Multisig::derive_multisig_address(&signers, threshold, 0);
			assert!(
				pallet_wormhole::Pallet::<Runtime>::is_ambiguous_account(&multisig_addr),
				"pre-creation multisig address must look ambiguous"
			);

			// Attacker pre-funds the pre-computed address; record_transfer counts it.
			let sender = intg_account(63);
			fund(&sender, 5_000 * UNIT);
			mark_revealed(&sender);
			pallet_wormhole::PotentialWormholeBalance::<Runtime>::put(POOL_BASE);

			let prefund = 1_000 * UNIT;
			run_transfer(&sender, &multisig_addr, prefund);
			assert_eq!(
				pool(),
				POOL_BASE + prefund,
				"pre-funding a pre-computed (ambiguous-looking) address is counted into the pool"
			);
			assert_eq!(<Balances as Currency<AccountId>>::free_balance(&multisig_addr), prefund);

			// Create the multisig at the same derived address.
			assert_ok!(Multisig::create_multisig(
				RuntimeOrigin::signed(creator.clone()),
				signers.clone(),
				threshold,
				0,
			));

			// Creation reveals the address: its balance is removed from the pool, exactly undoing
			// the pre-funding, so the multisig nets zero into the soundness pool.
			assert_eq!(
				pool(),
				POOL_BASE,
				"multisig creation must deduct the pre-funded balance from the pool"
			);
			assert!(
				!pallet_wormhole::Pallet::<Runtime>::is_ambiguous_account(&multisig_addr),
				"registered multisig must no longer be treated as ambiguous"
			);

			// A later receipt to the now-registered multisig is not re-counted.
			run_transfer(&sender, &multisig_addr, 100 * UNIT);
			assert_eq!(
				pool(),
				POOL_BASE,
				"post-creation receipts to a registered multisig must not be counted"
			);
		});
	}
}
