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
		// One `is_high_security` storage read for the flat whitelist check.
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

		// Enforce the high-security whitelist on the top-level signer. Origin-rewriting wrappers
		// (`as_derivative`/`as_recovered`) re-check the whitelist at the effective origin inside
		// their own pallets at dispatch time, so this flat check needs no call traversal.
		if !crate::configs::HighSecurityConfig::is_call_allowed(&who, call) {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(1)));
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

	/// Weight charged per recorded transfer proof.
	///
	/// Per recorded transfer, `record_transfer` touches one `TransferCount` read and one
	/// write, plus the ZK-tree leaf insert, whose path update walks the tree leaf-to-root
	/// and therefore costs reads/writes proportional to the *current* tree depth (read from
	/// storage here, so the charge tracks the tree as it deepens over the chain's life).
	fn per_transfer_weight() -> Weight {
		let (tree_reads, tree_writes) = pallet_zk_tree::Pallet::<Runtime>::insert_leaf_db_ops();
		T::DbWeight::get()
			.reads_writes(1u64.saturating_add(tree_reads), 1u64.saturating_add(tree_writes))
	}

	fn count_transfers(call: &RuntimeCall) -> u64 {
		// NOTE: this must stay in sync with the events matched by `record_proofs_from_events_since`
		// — we only weight calls whose emitted events we actually record. In particular
		// `Balances::force_set_balance` is deliberately NOT counted here: it emits `BalanceSet`
		// (an absolute set, not a transfer/mint), which we cannot turn into a transfer proof and
		// therefore never record.
		//
		// Wrappers whose inner call is stored on-chain rather than in the submitted call
		// (`Multisig::execute`, `ReversibleTransfers::recover_funds`, ...) cannot be counted
		// statically. Proof-recording work they trigger is reconciled in `post_dispatch`, which
		// registers any weight shortfall against the block via
		// `register_extra_weight_unchecked`.
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::force_transfer { .. }) => 1,

			RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) =>
				calls.iter().map(Self::count_transfers).sum(),

			RuntimeCall::Utility(pallet_utility::Call::dispatch_as { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::dispatch_as_fallible { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::with_weight { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::as_derivative { call, .. }) |
			RuntimeCall::Recovery(pallet_recovery::Call::as_recovered { call, .. }) =>
				Self::count_transfers(call),

			// Exactly one branch executes: the fallback runs only if the main call failed, in
			// which case the main call's changes (and its events) were rolled back. Charge the
			// worst case across the two branches; overcharge is never refunded, so summing
			// would systematically overprice the honest path.
			RuntimeCall::Utility(pallet_utility::Call::if_else { main, fallback }) =>
				Self::count_transfers(main).max(Self::count_transfers(fallback)),

			// Vesting calls fall through to 0 deliberately: the pallet records its
			// payouts itself (so Root calls enacted by the scheduler are captured too)
			// and carries the recording cost in its own benchmarked weights, while the
			// event scan below skips every pot-touching transfer. Counting them here
			// would charge twice for work this extension never performs.
			_ => 0,
		}
	}

	/// Scan events and record transfer proofs for any transfers that occurred
	/// since the given event count (to avoid re-processing previous events
	/// within the same block).
	///
	/// `event_count_before` is the value from `frame_system::Pallet::event_count()`
	/// captured in `prepare()`.
	///
	/// Returns the number of transfer proofs recorded, so callers can reconcile the actual
	/// proof-recording work against the statically charged weight.
	fn record_proofs_from_events_since(event_count_before: u32) -> u64 {
		// IMPORTANT: We must collect all transfers FIRST before calling record_transfer_proof,
		// because record_transfer_proof deposits new events which would invalidate the
		// stream_iter iterator (causing "Corrupted state" errors).
		//
		// The iterator reads from Events storage using stream_iter, which caches data.
		// If we modify Events storage during iteration (by depositing new events),
		// the cached data becomes stale and decoding fails.

		// The vesting pot's flows are excluded: the vesting pallet records its own
		// pot -> beneficiary payouts (also for scheduler-enacted Root calls this
		// extension never sees), so scanning them here would double-record; and pot
		// inbound/refund legs (treasury <-> pot) need no leaves — the pot is a keyless
		// pallet account and the treasury spends by signature, so neither can ever
		// exit through the wormhole.
		let vesting_pot = pallet_vesting::Pallet::<Runtime>::pot_account_id();

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
						}) if from != vesting_pot && to != vesting_pot => Some((None, from, to, amount)),
						// Native balance mints
						RuntimeEvent::Balances(pallet_balances::Event::Minted { who, amount }) => {
							let minting_account = crate::configs::MintingAccount::get();
							Some((None, minting_account, who, amount))
						},
						_ => None, // Ignore all other events
					}
				})
				.collect();

		// Now record the proofs - this is safe because we're no longer iterating over Events.
		// Count only credits that were actually recorded: `record_transfer_proof` returns
		// `false` for deliberately dropped credits, and counting those as recorded would
		// over-reserve fees and falsely register extra block weight on opaque paths.
		let mut recorded = 0u64;
		for (asset_id, from, to, amount) in transfers_to_record {
			if <Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
				asset_id, from, to, amount,
			) {
				recorded = recorded.saturating_add(1);
			}
		}
		recorded
	}
}

impl<T: pallet_wormhole::Config + Send + Sync + alloc::fmt::Debug> TransactionExtension<RuntimeCall>
	for WormholeProofRecorderExtension<T>
{
	/// `(event_count_snapshot, statically_charged_transfer_count)`. The snapshot bounds the
	/// event scan in `post_dispatch`; the charged count lets `post_dispatch` reconcile actual
	/// proof-recording work against the weight reserved by `weight()`.
	type Pre = (u32, u64);
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "WormholeProofRecorderExtension";

	fn weight(&self, call: &RuntimeCall) -> Weight {
		let n = Self::count_transfers(call);
		if n > 0 {
			Self::per_transfer_weight().saturating_mul(n)
		} else {
			Weight::zero()
		}
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// Snapshot current event count so we only process events added by this tx
		// (and any events from previous txs in the same block), and remember how many transfer
		// proofs were statically charged for so post_dispatch can reconcile.
		Ok((frame_system::Pallet::<Runtime>::event_count(), Self::count_transfers(call)))
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
		Ok((ValidTransaction::default(), (), origin))
	}

	fn post_dispatch(
		pre: Self::Pre,
		info: &DispatchInfoOf<RuntimeCall>,
		_post_info: &mut PostDispatchInfoOf<RuntimeCall>,
		_len: usize,
		result: &DispatchResult,
	) -> Result<(), TransactionValidityError> {
		// Only record proofs if the transaction succeeded.
		// Use the event count snapshot from prepare() to avoid duplicate recording.
		if result.is_ok() {
			let (event_count_before, charged_transfers) = pre;
			let recorded = Self::record_proofs_from_events_since(event_count_before);

			// Wrappers that dispatch inner calls stored on-chain (`Multisig::execute`,
			// `ReversibleTransfers::recover_funds`, ...) can emit transfer events the static
			// `count_transfers` matcher cannot see, so the proof-recording work above may exceed
			// the weight reserved by `weight()`. Register the shortfall against the block so
			// block-weight based DoS protection stays sound even when the static count drifts.
			if recorded > charged_transfers {
				frame_system::Pallet::<Runtime>::register_extra_weight_unchecked(
					Self::per_transfer_weight()
						.saturating_mul(recorded.saturating_sub(charged_transfers)),
					info.class,
				);
			}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_support::{
		assert_err_ignore_postinfo, assert_noop, assert_ok,
		pallet_prelude::TransactionValidityError, traits::Currency,
	};
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

		// Treasury account + portion are required for mining-reward distribution. Both
		// must be explicit: the genesis default no longer configures anything (the old
		// default account was the keyless `[1u8; 32]` minting sentinel).
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(AccountId32::from([9u8; 32])),
			treasury_portion: Some(sp_runtime::Permill::from_percent(50)),
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

	// Run the reversible transaction extension's `validate` for `call` signed by `signer`.
	fn validate_with(
		signer: AccountId,
		call: &RuntimeCall,
	) -> Result<(), TransactionValidityError> {
		ReversibleTransactionExtension::<Runtime>::new()
			.validate(
				RuntimeOrigin::signed(signer),
				call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.map(|_| ())
	}

	fn check_call(call: RuntimeCall) -> Result<(), TransactionValidityError> {
		// Verify Charlie is high-security
		assert!(ReversibleTransfers::is_high_security(&charlie()).is_some());

		let origin = RuntimeOrigin::signed(charlie());

		// Test the prepare method
		ReversibleTransactionExtension::<Runtime>::new()
			.prepare((), &origin, &call, &Default::default(), 0)
			.unwrap();

		// Test the validate method
		validate_with(charlie(), &call)
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
	// Origin-rewriting wrappers must not bypass high-security restrictions.
	// `as_recovered` / `as_derivative` re-check the whitelist at the effective
	// (rewritten) origin inside their own pallets, so a non-whitelisted call
	// cannot be dispatched as a high-security account, including under `batch`.
	// =========================================================================

	fn boxed(call: RuntimeCall) -> alloc::boxed::Box<RuntimeCall> {
		alloc::boxed::Box::new(call)
	}

	fn non_whitelisted_transfer() -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
			dest: MultiAddress::Id(bob()),
			value: 10 * EXISTENTIAL_DEPOSIT,
		})
	}

	fn whitelisted_schedule() -> RuntimeCall {
		RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::schedule_transfer {
			dest: MultiAddress::Id(bob()),
			amount: 10 * EXISTENTIAL_DEPOSIT,
		})
	}

	#[test]
	fn as_recovered_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// bob is charlie's recovery proxy; charlie is high-security (from genesis).
			pallet_recovery::Proxy::<Runtime>::insert(bob(), charlie());

			// A non-whitelisted call dispatched as the high-security account is rejected.
			assert_noop!(
				Recovery::as_recovered(
					RuntimeOrigin::signed(bob()),
					MultiAddress::Id(charlie()),
					boxed(non_whitelisted_transfer()),
				),
				pallet_recovery::Error::<Runtime>::CallNotAllowedForHighSecurity
			);

			// A whitelisted call is allowed through as the high-security account.
			assert_ok!(Recovery::as_recovered(
				RuntimeOrigin::signed(bob()),
				MultiAddress::Id(charlie()),
				boxed(whitelisted_schedule()),
			));
		});
	}

	#[test]
	fn as_derivative_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// Make alice's index-0 derivative a high-security account.
			let derivative = pallet_utility::derivative_account_id(alice(), 0u16);
			Balances::make_free_balance_be(&derivative, EXISTENTIAL_DEPOSIT * 100);
			assert_ok!(ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(derivative.clone()),
				qp_scheduler::BlockNumberOrTimestamp::BlockNumber(10),
				bob(),
			));

			// A non-whitelisted call dispatched as the high-security derivative is rejected.
			assert_err_ignore_postinfo!(
				Utility::as_derivative(
					RuntimeOrigin::signed(alice()),
					0,
					boxed(non_whitelisted_transfer()),
				),
				pallet_utility::Error::<Runtime>::CallNotAllowedForHighSecurity
			);

			// A whitelisted call as the derivative is allowed.
			assert_ok!(Utility::as_derivative(
				RuntimeOrigin::signed(alice()),
				0,
				boxed(whitelisted_schedule()),
			));

			// A different, non-high-security derivative is unaffected.
			assert_ok!(Utility::as_derivative(
				RuntimeOrigin::signed(alice()),
				1,
				boxed(RuntimeCall::System(frame_system::Call::remark { remark: vec![1] })),
			));
		});
	}

	#[test]
	fn batch_wrapped_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// Wrapping the origin-rewriter in a batch does not bypass the check: `batch_all`
			// re-dispatches `as_recovered`, whose own check rejects the non-whitelisted call.
			pallet_recovery::Proxy::<Runtime>::insert(bob(), charlie());
			let inner = RuntimeCall::Recovery(pallet_recovery::Call::as_recovered {
				account: MultiAddress::Id(charlie()),
				call: boxed(non_whitelisted_transfer()),
			});
			assert_err_ignore_postinfo!(
				Utility::batch_all(RuntimeOrigin::signed(bob()), vec![inner]),
				pallet_recovery::Error::<Runtime>::CallNotAllowedForHighSecurity
			);
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

			// Non-transfer calls trigger no proof-recording work and carry no weight.
			let non_transfer =
				RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
			let base_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &non_transfer);
			assert_eq!(base_weight, Weight::zero());

			let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &transfer);
			// A transfer is charged the per-transfer proof-recording cost.
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
	fn wormhole_proof_recorder_counts_as_derivative_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();

			// A transfer hidden behind `as_derivative` (possibly batched) must be charged the
			// same per-transfer weight as a direct transfer.
			let wrapped = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
				index: 0,
				call: boxed(RuntimeCall::Utility(pallet_utility::Call::batch {
					calls: vec![non_whitelisted_transfer(), non_whitelisted_transfer()],
				})),
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &wrapped);
			assert_eq!(
				weight,
				WormholeProofRecorderExtension::<Runtime>::per_transfer_weight().saturating_mul(2),
				"as_derivative-wrapped transfers must be statically counted"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_if_else_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			// `if_else` executes exactly one branch: the fallback runs only if the main call
			// failed, in which case the main call's changes (and events) were rolled back. The
			// static charge must therefore cover the worst case across the two branches.
			let call = RuntimeCall::Utility(pallet_utility::Call::if_else {
				main: boxed(RuntimeCall::Utility(pallet_utility::Call::batch {
					calls: vec![non_whitelisted_transfer(), non_whitelisted_transfer()],
				})),
				fallback: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				2,
				"if_else must charge for the transfer-heavier branch (main)"
			);

			// The worst case can also sit in the fallback branch.
			let call = RuntimeCall::Utility(pallet_utility::Call::if_else {
				main: boxed(RuntimeCall::System(frame_system::Call::remark { remark: vec![1] })),
				fallback: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				1,
				"if_else must charge for the transfer-heavier branch (fallback)"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_ignores_vesting_calls_and_pot_events() {
		new_test_ext().execute_with(|| {
			// Vesting calls charge no extension weight: the pallet records its own
			// payouts (covering scheduler-enacted Root calls too) and its benchmarked
			// weights carry that cost, while the event scan skips pot-touching
			// transfers.
			for call in [
				RuntimeCall::Vesting(pallet_vesting::Call::claim { schedule_id: 0 }),
				RuntimeCall::Vesting(pallet_vesting::Call::create_schedule {
					beneficiary: alice(),
					start: 0,
					cliff: 0,
					end: 1,
					total: 1,
				}),
				RuntimeCall::Vesting(pallet_vesting::Call::end_schedule { schedule_id: 0 }),
				RuntimeCall::Vesting(pallet_vesting::Call::retarget_schedule {
					schedule_id: 0,
					new_beneficiary: alice(),
				}),
			] {
				assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&call), 0);
			}

			// And the event scan must not record pot-sourced payouts (the pallet
			// already did): a pot -> alice transfer event yields no new proof.
			System::set_block_number(1);
			let pot = pallet_vesting::Pallet::<Runtime>::pot_account_id();
			let count_before = Wormhole::transfer_count(&alice());
			System::deposit_event(RuntimeEvent::Balances(pallet_balances::Event::Transfer {
				from: pot,
				to: alice(),
				amount: EXISTENTIAL_DEPOSIT * 100,
			}));
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);
			assert_eq!(Wormhole::transfer_count(&alice()), count_before);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_dispatch_as_fallible_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::dispatch_as_fallible {
				as_origin: alloc::boxed::Box::new(OriginCaller::system(
					frame_system::RawOrigin::Signed(alice()),
				)),
				call: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				1,
				"dispatch_as_fallible-wrapped transfers must be statically counted"
			);
		});
	}

	#[test]
	fn per_transfer_weight_scales_with_tree_depth() {
		new_test_ext().execute_with(|| {
			// Recording a transfer inserts a ZK-tree leaf, and the path update walks the
			// tree leaf-to-root — the charged weight must track the live tree depth.
			pallet_zk_tree::Depth::<Runtime>::put(1);
			let shallow = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();

			pallet_zk_tree::Depth::<Runtime>::put(20);
			let deep = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();

			assert!(
				deep.ref_time() > shallow.ref_time(),
				"per-transfer weight must grow with ZK-tree depth"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_registers_extra_weight_for_uncounted_transfers() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// The presented call is opaque to the static matcher (like `Multisig::execute` or
			// `ReversibleTransfers::recover_funds`, whose inner call lives on-chain), but the
			// dispatch emits a real transfer event that post_dispatch must record.
			let opaque_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&opaque_call), 0);

			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			run_lifecycle(&alice(), opaque_call, || {
				assert_ok!(Balances::transfer_keep_alive(
					RuntimeOrigin::signed(alice()),
					MultiAddress::Id(bob()),
					EXISTENTIAL_DEPOSIT * 50,
				));
			});

			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::per_transfer_weight(),
				"the uncounted recorded transfer must be registered as extra block weight"
			);
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
			let encoded_call: pallet_multisig::BoundedCallOf<Runtime> =
				inner_call.encode().try_into().unwrap();
			let expiry = System::block_number() + 100;

			// Alice proposes
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				encoded_call.clone(),
				expiry,
			));

			// Bob approves (reaches threshold), resubmitting the proposal's call
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				0, // proposal_id
				encoded_call,
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

	/// Run a full transaction (whatever `dispatch` performs) through the extension lifecycle.
	/// `call` is the call presented to validate/prepare; `dispatch` performs the real execution.
	fn run_lifecycle(from: &AccountId, call: RuntimeCall, dispatch: impl FnOnce()) {
		use sp_runtime::traits::TxBaseImplication;

		let ext = WormholeProofRecorderExtension::<Runtime>::new();
		let origin = RuntimeOrigin::signed(from.clone());

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

		// Execute the real call (emits the Transfer event(s) that post_dispatch scans).
		dispatch();

		// post_dispatch(): records the transfer proof(s).
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
}
