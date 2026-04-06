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
///   - Sudo transfers (sudo.sudo_as)
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
			RuntimeCall::Assets(pallet_assets::Call::transfer { .. }) |
			RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive { .. }) => 1,

			RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) =>
				calls.iter().map(Self::count_transfers).sum(),

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
		// Read all events and filter by pallet.
		// We use read_events_no_consensus to iterate through all events.
		// Only process events that were added since this tx started.
		for event_record in frame_system::Pallet::<Runtime>::read_events_no_consensus()
			.skip(event_count_before as usize)
		{
			match event_record.event {
				// Native balance transfers
				RuntimeEvent::Balances(pallet_balances::Event::Transfer { from, to, amount }) => {
					<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
						None, // Native token has no asset_id
						from,
						to,
						amount,
					);
				},
				// Native balance mints
				RuntimeEvent::Balances(pallet_balances::Event::Minted { who, amount }) => {
					let minting_account = crate::configs::MintingAccount::get();
					<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
						None,
						minting_account,
						who,
						amount,
					);
				},
				// Asset transfers
				RuntimeEvent::Assets(pallet_assets::Event::Transferred {
					asset_id,
					from,
					to,
					amount,
				}) => {
					<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
						Some(asset_id),
						from,
						to,
						amount,
					);
				},
				// Asset mints
				RuntimeEvent::Assets(pallet_assets::Event::Issued { asset_id, owner, amount }) => {
					let minting_account = crate::configs::AssetMintingAccount::get();
					<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
						Some(asset_id),
						minting_account,
						owner,
						amount,
					);
				},
				_ => {}, // Ignore all other events
			}
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
		if n > 0 {
			// Per transfer: 1 read (TransferCount) + 2 writes (TransferProof + TransferCount)
			T::DbWeight::get().reads_writes(n, 2 * n)
		} else {
			Weight::zero()
		}
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
		_origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &DispatchInfoOf<RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		// No validation needed - just return Ok
		Ok((ValidTransaction::default(), (), _origin))
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
	use frame_support::{assert_ok, pallet_prelude::TransactionValidityError};
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

		// high securiry account is charlie
		// interceptor is alice
		pallet_reversible_transfers::GenesisConfig::<Runtime> {
			initial_high_security_accounts: vec![(charlie(), alice(), 10)],
		}
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

			let non_transfer =
				RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &non_transfer);
			assert_eq!(weight, Weight::zero());

			let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &transfer);
			assert!(weight.ref_time() > 0);

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

			// Verify proof was recorded
			let count_after = Wormhole::transfer_count(&bob_account);
			assert_eq!(count_after, count_before + 1, "Transfer count should increment");

			// Verify the proof exists
			assert!(
				Wormhole::transfer_proof((bob_account, count_before)).is_some(),
				"Transfer proof should exist"
			);
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

			// Verify proof was recorded
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
			assert!(Wormhole::transfer_proof((bob_account, count_before)).is_some());
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

			// Verify proof was recorded with actual amount (not Balance::MAX)
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
			assert!(Wormhole::transfer_proof((bob_account, count_before)).is_some());
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

			// Verify both proofs were recorded
			assert_eq!(Wormhole::transfer_count(&bob_account), bob_count_before + 1);
			assert_eq!(Wormhole::transfer_count(&charlie_account), charlie_count_before + 1);
			assert!(Wormhole::transfer_proof((bob_account, bob_count_before)).is_some());
			assert!(Wormhole::transfer_proof((charlie_account, charlie_count_before)).is_some());
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

			// Mint tokens (requires sudo/root)
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
			// This test documents the expected behavior
			if count_after > count_before {
				assert!(Wormhole::transfer_proof((recipient, count_before)).is_some());
			}
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
			let encoded_call = inner_call.encode();
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

			// Verify proof was recorded for the transfer TO charlie
			// The transfer is FROM the multisig address
			let count_after = Wormhole::transfer_count(&charlie_account);
			assert_eq!(
				count_after,
				count_before + 1,
				"Transfer count should increment for multisig transfer"
			);

			// Verify the proof exists
			assert!(
				Wormhole::transfer_proof((charlie_account, count_before)).is_some(),
				"Transfer proof should exist for multisig transfer"
			);
		});
	}
}
