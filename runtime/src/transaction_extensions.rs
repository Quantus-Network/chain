//! Custom signed extensions for the runtime.
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
	traits::{DispatchInfoOf, PostDispatchInfoOf, StaticLookup, TransactionExtension},
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

/// Details of a transfer to be recorded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferDetails {
	from: AccountId,
	to: AccountId,
	amount: Balance,
	asset_id: AssetId,
}

/// Transaction extension that records transfer proofs in the wormhole pallet
///
/// This extension:
/// - Extracts transfer details from balance/asset transfer calls
/// - Records proofs in wormhole storage after successful execution
/// - Increments transfer count
/// - Emits events
/// - Fails the transaction if proof recording fails
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct WormholeProofRecorderExtension<T: pallet_wormhole::Config + Send + Sync>(PhantomData<T>);

impl<T: pallet_wormhole::Config + Send + Sync> WormholeProofRecorderExtension<T> {
	/// Creates new extension
	pub fn new() -> Self {
		Self(PhantomData)
	}

	/// Helper to convert lookup errors to transaction validity errors
	fn lookup(address: &Address) -> Result<AccountId, TransactionValidityError> {
		<Runtime as frame_system::Config>::Lookup::lookup(address.clone())
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::BadSigner))
	}

	/// Extract transfer details from a runtime call
	fn extract_transfer_details(
		origin: &RuntimeOrigin,
		call: &RuntimeCall,
	) -> Result<Option<TransferDetails>, TransactionValidityError> {
		// Only process signed transactions
		let who = match ensure_signed(origin.clone()) {
			Ok(signer) => signer,
			Err(_) => return Ok(None),
		};

		let details = match call {
			// Native balance transfers
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, value }) => {
				let to = Self::lookup(dest)?;
				Some(TransferDetails { from: who, to, amount: *value, asset_id: 0 })
			},
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest, value }) => {
				let to = Self::lookup(dest)?;
				Some(TransferDetails { from: who, to, amount: *value, asset_id: 0 })
			},
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. }) => None,

			// Asset transfers
			RuntimeCall::Assets(pallet_assets::Call::transfer { id, target, amount }) => {
				let to = Self::lookup(target)?;
				Some(TransferDetails { asset_id: id.0, from: who, to, amount: *amount })
			},
			RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive {
				id,
				target,
				amount,
			}) => {
				let to = Self::lookup(target)?;
				Some(TransferDetails { asset_id: id.0, from: who, to, amount: *amount })
			},

			_ => None,
		};

		Ok(details)
	}

	/// Record the transfer proof using the TransferProofRecorder trait
	fn record_proof(details: TransferDetails) -> Result<(), TransactionValidityError> {
		let asset_id = if details.asset_id == 0 { None } else { Some(details.asset_id) };

		<Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
			asset_id,
			details.from,
			details.to,
			details.amount,
		)
		.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Custom(100)))
	}
}

impl<T: pallet_wormhole::Config + Send + Sync + alloc::fmt::Debug> TransactionExtension<RuntimeCall>
	for WormholeProofRecorderExtension<T>
{
	type Pre = Option<TransferDetails>;
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "WormholeProofRecorderExtension";

	fn weight(&self, call: &RuntimeCall) -> Weight {
		// Account for proof recording in post_dispatch
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. })
			| RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. })
			| RuntimeCall::Assets(pallet_assets::Call::transfer { .. })
			| RuntimeCall::Assets(pallet_assets::Call::transfer_keep_alive { .. }) => {
				// 2 writes: TransferProof insert + TransferCount update
				// 1 read: TransferCount get
				T::DbWeight::get().reads_writes(1, 2)
			},
			_ => Weight::zero(),
		}
	}

	fn prepare(
		self,
		_val: Self::Val,
		origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// Extract transfer details to pass to post_dispatch
		Self::extract_transfer_details(origin, call)
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
		post_info: &mut PostDispatchInfoOf<RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<(), TransactionValidityError> {
		// Only record proof if the transaction succeeded (no error in post_info)
		if post_info.actual_weight.is_some() || _result.is_ok() {
			if let Some(details) = pre {
				// Record the proof - if this fails, fail the whole transaction
				Self::record_proof(details)?;
			}
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

	#[test]
	fn wormhole_proof_recorder_native_transfer() {
		new_test_ext().execute_with(|| {
			let alice_origin = RuntimeOrigin::signed(alice());
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});

			let details = WormholeProofRecorderExtension::<Runtime>::extract_transfer_details(
				&alice_origin,
				&call,
			)
			.unwrap();

			assert!(details.is_some());
			let details = details.unwrap();
			assert_eq!(details.from, alice());
			assert_eq!(details.to, bob());
			assert_eq!(details.amount, 100 * UNIT);
			assert_eq!(details.asset_id, 0);
		});
	}

	#[test]
	fn wormhole_proof_recorder_asset_transfer() {
		new_test_ext().execute_with(|| {
			let alice_origin = RuntimeOrigin::signed(alice());
			let asset_id = 42u32;
			let call = RuntimeCall::Assets(pallet_assets::Call::transfer {
				id: codec::Compact(asset_id),
				target: MultiAddress::Id(bob()),
				amount: 500,
			});

			let details = WormholeProofRecorderExtension::<Runtime>::extract_transfer_details(
				&alice_origin,
				&call,
			)
			.unwrap();

			assert!(details.is_some());
			let details = details.unwrap();
			assert_eq!(details.from, alice());
			assert_eq!(details.to, bob());
			assert_eq!(details.amount, 500);
			assert_eq!(details.asset_id, asset_id);
		});
	}

	#[test]
	fn wormhole_proof_recorder_ignores_non_transfer() {
		new_test_ext().execute_with(|| {
			let alice_origin = RuntimeOrigin::signed(alice());
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

			let details = WormholeProofRecorderExtension::<Runtime>::extract_transfer_details(
				&alice_origin,
				&call,
			)
			.unwrap();

			assert!(details.is_none());
		});
	}
}
