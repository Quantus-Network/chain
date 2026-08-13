//! End-to-end tests for the on-chain pubkey cache (`pallet-pubkey`): an
//! account's first full `SignatureWithPublic` transaction registers its
//! Dilithium public key, after which `SigOnly` transactions — which omit the
//! ~2 KB public key and sign the domain-separated payload — pass the full
//! `Executive::apply_extrinsic` pipeline.

use codec::Encode;
use frame_support::{assert_ok, traits::Currency};
use qp_dilithium_crypto::{
	sig_only_signing_payload, Dilithium65Pair, Dilithium65Public, Dilithium65SignatureWithPublic,
	DilithiumSignatureScheme, DilithiumSigner,
};
use quantus_runtime::{
	transaction_extensions::{
		ChargePubkeyCacheVerify, ReversibleTransactionExtension, WormholeProofRecorderExtension,
	},
	Balances, BalancesCall, Executive, Runtime, RuntimeCall, RuntimeOrigin, Signature,
	SignedPayload, System, TxExtension, UncheckedExtrinsic, UNIT, VERSION,
};
use sp_core::{ByteArray, Pair};
use sp_runtime::{
	generic::Era,
	traits::IdentifyAccount,
	transaction_validity::{InvalidTransaction, TransactionValidityError},
	AccountId32, MultiAddress,
};

fn test_ext(account: &AccountId32) -> sp_io::TestExternalities {
	use quantus_runtime::BuildStorage;

	let t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| {
		Balances::make_free_balance_be(account, 1000 * UNIT);
		System::set_block_number(1);
		// Enter the extrinsic-application phase, as `Executive::initialize_block`
		// does: `pallet_pubkey`'s `OnKilledAccount` hook attributes reap cleanup
		// by execution phase (counted for the fee inside extrinsics, registered
		// as block weight outside them).
		System::note_finished_initialize();
	});
	ext
}

/// The fee `who` paid for the extrinsic just applied, from `TransactionFeePaid`.
fn fee_paid(who: &AccountId32) -> u128 {
	System::events()
		.into_iter()
		.find_map(|record| match record.event {
			quantus_runtime::RuntimeEvent::TransactionPayment(
				pallet_transaction_payment::Event::TransactionFeePaid {
					who: payer,
					actual_fee,
					..
				},
			) if payer == *who => Some(actual_fee),
			_ => None,
		})
		.expect("the extrinsic must have paid a fee")
}

/// The signing payload and transaction extensions for `call` at `nonce`.
fn payload_and_ext(call: RuntimeCall, nonce: u32) -> (SignedPayload, TxExtension) {
	let genesis_hash = System::block_hash(0);

	let tx_ext: TxExtension = (
		frame_system::CheckNonZeroSender::<Runtime>::new(),
		frame_system::CheckSpecVersion::<Runtime>::new(),
		frame_system::CheckTxVersion::<Runtime>::new(),
		frame_system::CheckGenesis::<Runtime>::new(),
		frame_system::CheckEra::<Runtime>::from(Era::immortal()),
		frame_system::CheckNonce::<Runtime>::from(nonce),
		(ChargePubkeyCacheVerify::new(), frame_system::CheckWeight::<Runtime>::new()),
		ReversibleTransactionExtension::<Runtime>::new(),
		WormholeProofRecorderExtension::<Runtime>::new(),
		pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(0),
		frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
		frame_system::WeightReclaim::<Runtime>::new(),
	);

	let raw_payload = SignedPayload::from_raw(
		call,
		tx_ext.clone(),
		(
			(),
			VERSION.spec_version,
			VERSION.transaction_version,
			genesis_hash,
			genesis_hash,
			(),
			((), ()),
			(),
			(),
			(),
			None,
			(),
		),
	);
	(raw_payload, tx_ext)
}

/// Build a signed extrinsic for `call`. With `sig_only` the public key is
/// omitted from the signature, relying on the on-chain cache, and the
/// signature is made over the domain-separated sig-only payload.
fn signed_call(
	pair: &Dilithium65Pair,
	sender: AccountId32,
	call: RuntimeCall,
	nonce: u32,
	sig_only: bool,
) -> UncheckedExtrinsic {
	let (raw_payload, tx_ext) = payload_and_ext(call.clone(), nonce);

	let signature: Signature = if sig_only {
		let sig = raw_payload
			.using_encoded(|e| pair.sign(&sig_only_signing_payload(e)))
			.signature();
		DilithiumSignatureScheme::Dilithium65SigOnly(sig).into()
	} else {
		raw_payload.using_encoded(|e| pair.sign(e)).into()
	};

	UncheckedExtrinsic::new_signed(call, MultiAddress::Id(sender), signature, tx_ext)
}

/// Build a `transfer_keep_alive` extrinsic signed by `pair`.
fn signed_transfer(
	pair: &Dilithium65Pair,
	sender: AccountId32,
	dest: AccountId32,
	value: u128,
	nonce: u32,
	sig_only: bool,
) -> UncheckedExtrinsic {
	signed_call(
		pair,
		sender,
		BalancesCall::transfer_keep_alive { dest: MultiAddress::Id(dest), value }.into(),
		nonce,
		sig_only,
	)
}

#[test]
fn first_full_transaction_caches_pubkey() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		assert!(pallet_pubkey::Pallet::<Runtime>::pubkey_of(&account).is_none());

		let dest = AccountId32::new([9u8; 32]);
		let xt = signed_transfer(&pair, account.clone(), dest, 10 * UNIT, 0, false);
		assert_ok!(Executive::apply_extrinsic(xt).expect("full-signature extrinsic is valid"));

		let cached = pallet_pubkey::Pallet::<Runtime>::pubkey_of(&account)
			.expect("pubkey must be cached after the first full transaction");
		assert_eq!(cached.into_account(), account);
	});
}

#[test]
fn sig_only_transaction_applies_after_registration() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);

		// Register the pubkey with a full transaction, then transfer sig-only.
		let full = signed_transfer(&pair, account.clone(), dest.clone(), 10 * UNIT, 0, false);
		let full_len = full.encode().len();
		assert_ok!(Executive::apply_extrinsic(full).expect("full-signature extrinsic is valid"));

		let sig_only = signed_transfer(&pair, account.clone(), dest.clone(), 5 * UNIT, 1, true);
		let sig_only_len = sig_only.encode().len();
		assert_ok!(Executive::apply_extrinsic(sig_only).expect("sig-only extrinsic is valid"));
		assert_eq!(Balances::free_balance(&dest), 15 * UNIT);

		// The saving is exactly the omitted ML-DSA-65 public key.
		assert_eq!(full_len - sig_only_len, <Dilithium65Public as ByteArray>::LEN);
	});
}

#[test]
fn sig_only_transaction_rejected_without_registration() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);
		let xt = signed_transfer(&pair, account.clone(), dest, 10 * UNIT, 0, true);

		assert_eq!(
			Executive::apply_extrinsic(xt),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)),
			"sig-only extrinsic must be rejected while no pubkey is cached"
		);
	});
}

#[test]
fn sig_only_transaction_from_wrong_key_rejected() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);

		let full = signed_transfer(&pair, account.clone(), dest.clone(), 10 * UNIT, 0, false);
		assert_ok!(Executive::apply_extrinsic(full).expect("full-signature extrinsic is valid"));

		// An impostor signs sig-only, claiming `account` as origin: their
		// signature cannot verify against the cached key.
		let impostor = Dilithium65Pair::from_seed_slice(&[7u8; 32]).expect("valid seed");
		let xt = signed_transfer(&impostor, account.clone(), dest, 5 * UNIT, 1, true);

		assert_eq!(
			Executive::apply_extrinsic(xt),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)),
			"sig-only extrinsic signed by a different key must be rejected"
		);
	});
}

/// A third party must not be able to re-encode a sig-only extrinsic into the
/// ~2 KB larger full form: the cached pubkey is world-readable, so only the
/// domain separation of the signed message stops the substitution. Without
/// it, a fee-collecting block author could inflate every sig-only extrinsic
/// it includes — the sender pays the extra length fee, and the extrinsic hash
/// the sender is watching never appears on chain.
#[test]
fn sig_only_extrinsic_reencoded_as_full_is_rejected() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);

		// Register the key.
		let full = signed_transfer(&pair, account.clone(), dest.clone(), 10 * UNIT, 0, false);
		assert_ok!(Executive::apply_extrinsic(full).expect("full-signature extrinsic is valid"));

		// The victim's sig-only transaction for nonce 1.
		let call: RuntimeCall =
			BalancesCall::transfer_keep_alive { dest: MultiAddress::Id(dest), value: 5 * UNIT }
				.into();
		let (raw_payload, tx_ext) = payload_and_ext(call.clone(), 1);
		let sig = raw_payload
			.using_encoded(|e| pair.sign(&sig_only_signing_payload(e)))
			.signature();

		// An attacker pulls the raw signature out of the preamble and
		// re-wraps it as a full SignatureWithPublic, with the public key read
		// from on-chain storage.
		let cached_public = match pallet_pubkey::Pallet::<Runtime>::pubkey_of(&account) {
			Some(DilithiumSigner::Dilithium65(public)) => public,
			other => panic!("expected a cached ML-DSA-65 key, got {other:?}"),
		};
		let inflated_sig: Signature = DilithiumSignatureScheme::Dilithium65(
			Dilithium65SignatureWithPublic::new(sig.clone(), cached_public),
		)
		.into();
		let inflated = UncheckedExtrinsic::new_signed(
			call.clone(),
			MultiAddress::Id(account.clone()),
			inflated_sig,
			tx_ext.clone(),
		);
		assert_eq!(
			Executive::apply_extrinsic(inflated),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)),
			"a sig-only signature re-wrapped as a full signature must be rejected"
		);

		// The genuine sig-only encoding of the very same signature applies
		// fine: only the re-encoding was rejected, not the signature.
		let sig_only: Signature = DilithiumSignatureScheme::Dilithium65SigOnly(sig).into();
		let genuine = UncheckedExtrinsic::new_signed(
			call,
			MultiAddress::Id(account.clone()),
			sig_only,
			tx_ext,
		);
		assert_ok!(Executive::apply_extrinsic(genuine).expect("genuine sig-only form is valid"));
	});
}

/// The reverse direction: stripping the public key off an observed full
/// extrinsic and resubmitting it in the smaller sig-only form must fail,
/// because the full signature was made over the raw payload, not the
/// domain-separated one.
#[test]
fn full_extrinsic_reencoded_as_sig_only_is_rejected() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);

		let full = signed_transfer(&pair, account.clone(), dest.clone(), 10 * UNIT, 0, false);
		assert_ok!(Executive::apply_extrinsic(full).expect("full-signature extrinsic is valid"));

		// A full-signature transaction for nonce 1, as an attacker would see
		// it in the pool…
		let call: RuntimeCall =
			BalancesCall::transfer_keep_alive { dest: MultiAddress::Id(dest), value: 5 * UNIT }
				.into();
		let (raw_payload, tx_ext) = payload_and_ext(call.clone(), 1);
		let full_sig = raw_payload.using_encoded(|e| pair.sign(e));

		// …stripped down to its bare signature.
		let stripped: Signature =
			DilithiumSignatureScheme::Dilithium65SigOnly(full_sig.signature()).into();
		let xt = UncheckedExtrinsic::new_signed(
			call,
			MultiAddress::Id(account.clone()),
			stripped,
			tx_ext,
		);

		assert_eq!(
			Executive::apply_extrinsic(xt),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)),
			"a full signature re-wrapped as sig-only must be rejected"
		);
	});
}

/// Verify-path DB work (one `Pubkeys` read plus the first-registration insert)
/// is charged by `ChargePubkeyCacheVerify` as signed-only `extension_weight`;
/// the `Pubkeys::remove` on account reap is pre-charged by the same extension
/// on kill-capable calls and kept when the reap happens. Together they cover
/// the worst case — a first full-signature `transfer_all(..., keep_alive =
/// false)` that registers and reaps in one extrinsic.
#[test]
fn first_registration_plus_reap_weight_covers_combined_db_ops() {
	use frame_support::{
		dispatch::DispatchClass,
		weights::constants::{ExtrinsicBaseWeight, RocksDbWeight},
	};
	use quantus_runtime::configs::{PubkeyCacheVerifyWeight, RuntimeBlockWeights};
	use sp_runtime::traits::TransactionExtension;

	let verify = RocksDbWeight::get().reads_writes(1, 1);
	let cleanup = pallet_pubkey::Pallet::<Runtime>::reap_cleanup_weight();

	// Signed-only extension weight covers the verify-path work; class-wide
	// `base_extrinsic` must not, or bare unsigned Normal (wormhole exits) would
	// pay for a Verify they never run.
	assert_eq!(PubkeyCacheVerifyWeight::get(), verify);
	let call: RuntimeCall = BalancesCall::transfer_keep_alive {
		dest: MultiAddress::Id(AccountId32::new([9u8; 32])),
		value: UNIT,
	}
	.into();
	assert!(
		ChargePubkeyCacheVerify::new().weight(&call).all_gte(verify),
		"ChargePubkeyCacheVerify must cover verify {verify:?}"
	);
	let weights = RuntimeBlockWeights::get();
	for class in [DispatchClass::Normal, DispatchClass::Operational] {
		assert_eq!(
			weights.get(class).base_extrinsic,
			ExtrinsicBaseWeight::get(),
			"{class:?} base_extrinsic must not include PubkeyCacheVerifyWeight"
		);
	}

	// The cleanup is pre-charged per kill-capable call and kept when the reap
	// happens, so a realized reap's block-weight consumption carries it.
	// Differential check: two otherwise identical `transfer_all` extrinsics,
	// one of which reaps its sender — the reaping one must consume at least
	// the reap-cleanup weight.
	//
	// Each extrinsic runs as the sole extrinsic of its own externalities so
	// `WormholeProofRecorderExtension`'s event-scan weight (charged on the
	// cumulative block event count) cannot confound the differential with a
	// positional re-scan of a prior extrinsic's events. Even then the reaping
	// path emits extra events (e.g. `KilledAccount`), so the scan cost still
	// differs — the exact fee-side accounting is pinned by
	// `reaping_transfer_all_pays_the_cleanup_fee`; here we only require
	// block-weight coverage.
	let reaper_pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let reaper = reaper_pair.public().into_account();
	let keeper_pair = Dilithium65Pair::from_seed_slice(&[43u8; 32]).expect("valid seed");
	let keeper = keeper_pair.public().into_account();
	let dest = AccountId32::new([9u8; 32]);

	let consumed_by = |pair: &Dilithium65Pair, sender: &AccountId32, keep_alive: bool| {
		let mut ext = test_ext(sender);
		ext.execute_with(|| {
			Balances::make_free_balance_be(&dest, UNIT);
			let xt = signed_call(
				pair,
				sender.clone(),
				BalancesCall::transfer_all { dest: MultiAddress::Id(dest.clone()), keep_alive }
					.into(),
				0,
				false,
			);
			let before = *System::block_weight().get(DispatchClass::Normal);
			assert_ok!(Executive::apply_extrinsic(xt).expect("extrinsic is valid"));
			let after = *System::block_weight().get(DispatchClass::Normal);
			if !keep_alive {
				assert!(
					!frame_system::Account::<Runtime>::contains_key(sender),
					"sender must be reaped"
				);
			}
			after.saturating_sub(before)
		})
	};

	let keep = consumed_by(&keeper_pair, &keeper, true);
	let reap = consumed_by(&reaper_pair, &reaper, false);

	assert!(
		reap.saturating_sub(keep).all_gte(cleanup),
		"a reaping transfer_all must cover at least the Pubkeys::remove write \
		 on top of an otherwise identical non-reaping one; \
		 keep={keep:?} reap={reap:?} cleanup={cleanup:?}"
	);
}

/// A full-signed `transfer_all(..., keep_alive = false)` verifies (and would
/// cache the pubkey) before dispatch reaps the sender. The cache entry must
/// not survive the reap, or fund → register → reap could leave unbounded
/// orphan `Pubkeys` state. Weight coverage for this combined path lives in
/// [`first_registration_plus_reap_weight_covers_combined_db_ops`].
#[test]
fn full_signed_reap_clears_cached_pubkey() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);
		Balances::make_free_balance_be(&dest, UNIT);

		let xt = signed_call(
			&pair,
			account.clone(),
			BalancesCall::transfer_all { dest: MultiAddress::Id(dest), keep_alive: false }.into(),
			0,
			false,
		);
		assert_ok!(Executive::apply_extrinsic(xt).expect("full-signed transfer_all is valid"));

		assert!(!frame_system::Account::<Runtime>::contains_key(&account), "sender must be reaped");
		assert!(
			pallet_pubkey::Pallet::<Runtime>::pubkey_of(&account).is_none(),
			"reaped account must not leave an orphan pubkey cache entry"
		);
	});
}

/// Regression (security review 2026-08): the `Pubkeys::remove` a reap performs
/// must be billed to the submitter through the corrected post-dispatch weight,
/// not merely registered against the block after the fact. Two full-pipeline
/// extrinsics from the same signer, identical in length and call weight and
/// differing only in `keep_alive`: the reaping one's `TransactionFeePaid`
/// amount must exceed the non-reaping one's by exactly the reap-cleanup weight
/// converted to fee.
#[test]
fn reaping_transfer_all_pays_the_cleanup_fee() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let dest = AccountId32::new([9u8; 32]);

	let fee_with = |keep_alive: bool| {
		let mut ext = test_ext(&account);
		ext.execute_with(|| {
			Balances::make_free_balance_be(&dest, UNIT);
			let xt = signed_call(
				&pair,
				account.clone(),
				BalancesCall::transfer_all { dest: MultiAddress::Id(dest.clone()), keep_alive }
					.into(),
				0,
				false,
			);
			assert_ok!(Executive::apply_extrinsic(xt).expect("extrinsic is valid"));
			assert_eq!(
				frame_system::Account::<Runtime>::contains_key(&account),
				keep_alive,
				"keep_alive={keep_alive}: reap expectation"
			);
			fee_paid(&account)
		})
	};

	let keep_fee = fee_with(true);
	let reap_fee = fee_with(false);

	// The runtime's fee multiplier is constant 1, so the adjusted weight fee
	// difference is exactly the unadjusted conversion of the cleanup weight.
	let cleanup_fee =
		pallet_transaction_payment::Pallet::<Runtime>::weight_to_fee(pallet_pubkey::Pallet::<
			Runtime,
		>::reap_cleanup_weight());
	assert!(cleanup_fee > 0, "the cleanup must convert to a nonzero fee");
	assert_eq!(
		reap_fee - keep_fee,
		cleanup_fee,
		"a reaping transfer_all must pay for its pubkey-cache cleanup"
	);
}

/// Regression (security review 2026-08): under call composition a single
/// extrinsic can reap several accounts — here a `batch_all` of three
/// `as_derivative`-wrapped `transfer_all(keep_alive = false)` sweeps, each
/// killing one funded derivative account. Every reap must be (a) admitted
/// pre-dispatch — the declared weight `CheckWeight` sees carries one cleanup
/// reservation per sweep — and (b) billed to the payer: the fee exceeds the
/// non-reaping control's by exactly three cleanups.
#[test]
fn batched_derivative_reaps_are_admitted_and_paid_per_reap() {
	use frame_support::dispatch::GetDispatchInfo;

	const REAPS: u64 = 3;

	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let dest = AccountId32::new([9u8; 32]);

	let batch_call = |keep_alive: bool| -> RuntimeCall {
		let calls = (0..REAPS as u16)
			.map(|index| {
				RuntimeCall::Utility(pallet_utility::Call::as_derivative {
					index,
					call: Box::new(
						BalancesCall::transfer_all {
							dest: MultiAddress::Id(dest.clone()),
							keep_alive,
						}
						.into(),
					),
				})
			})
			.collect();
		RuntimeCall::Utility(pallet_utility::Call::batch_all { calls })
	};

	let run = |keep_alive: bool| {
		let mut ext = test_ext(&account);
		ext.execute_with(|| {
			Balances::make_free_balance_be(&dest, UNIT);
			let derivatives: Vec<AccountId32> = (0..REAPS as u16)
				.map(|index| pallet_utility::derivative_account_id(account.clone(), index))
				.collect();
			for derivative in &derivatives {
				Balances::make_free_balance_be(derivative, UNIT);
			}

			let xt = signed_call(&pair, account.clone(), batch_call(keep_alive), 0, false);
			let declared = xt.get_dispatch_info().total_weight();
			assert_ok!(Executive::apply_extrinsic(xt).expect("extrinsic is valid"));

			for derivative in &derivatives {
				assert_eq!(
					frame_system::Account::<Runtime>::contains_key(derivative),
					keep_alive,
					"keep_alive={keep_alive}: derivative reap expectation"
				);
			}
			(declared, fee_paid(&account))
		})
	};

	let (keep_declared, keep_fee) = run(true);
	let (reap_declared, reap_fee) = run(false);

	let cleanup = pallet_pubkey::Pallet::<Runtime>::reap_cleanup_weight();

	// (a) Pre-dispatch bound: the reaping batch is admitted with one cleanup
	// reservation per potential reap — this is what `CheckWeight` checks
	// against block limits BEFORE dispatch, closing the boundary-overshoot gap.
	assert_eq!(
		reap_declared.saturating_sub(keep_declared),
		cleanup.saturating_mul(REAPS),
		"declared weight must carry one cleanup reservation per batched reap"
	);

	// (b) Fee: all three realized reaps stay in the corrected weight the payer
	// is billed for.
	let cleanup_fee =
		pallet_transaction_payment::Pallet::<Runtime>::weight_to_fee(cleanup.saturating_mul(REAPS));
	assert!(cleanup_fee > 0, "the cleanups must convert to a nonzero fee");
	assert_eq!(
		reap_fee - keep_fee,
		cleanup_fee,
		"a multi-reap wrapper must pay for every pubkey-cache cleanup it caused"
	);
}

/// Regression (security review 2026-08): `Multisig::execute` must not dispatch
/// work invisible to pre-dispatch admission and fees. Since `execute` carries
/// the proposal's call in the submitted extrinsic (verified byte-equal to the
/// stored payload), `count_reaps` recurses into it like any other wrapper.
/// Full pipeline: a threshold-1 multisig proposal wrapping a `batch_all` of
/// three `as_derivative` `transfer_all(keep_alive = false)` sweeps, executed
/// through `Executive::apply_extrinsic` with a real signed `execute`. Asserts
/// (a) admission — the declared weight `CheckWeight` sees carries exactly the
/// three counted cleanups on top of the non-reaping control's — and
/// (b) fees — the payer's `TransactionFeePaid` exceeds the control's by
/// exactly the three realized cleanups.
#[test]
fn multisig_stored_call_reaps_are_admitted_and_paid_per_reap() {
	use frame_support::dispatch::GetDispatchInfo;
	use sp_runtime::traits::TransactionExtension;

	const REAPS: u64 = 3;

	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let cosigner = AccountId32::new([8u8; 32]);
	let dest = AccountId32::new([9u8; 32]);

	let stored_call = |keep_alive: bool| -> RuntimeCall {
		let calls = (0..REAPS as u16)
			.map(|index| {
				RuntimeCall::Utility(pallet_utility::Call::as_derivative {
					index,
					call: Box::new(
						BalancesCall::transfer_all {
							dest: MultiAddress::Id(dest.clone()),
							keep_alive,
						}
						.into(),
					),
				})
			})
			.collect();
		RuntimeCall::Utility(pallet_utility::Call::batch_all { calls })
	};

	let run = |keep_alive: bool| {
		let mut ext = test_ext(&account);
		ext.execute_with(|| {
			Balances::make_free_balance_be(&dest, UNIT);
			Balances::make_free_balance_be(&cosigner, UNIT);

			// Threshold-1 multisig (at least two signers required): `propose`
			// marks the proposal Approved immediately, but `execute` is still
			// its own dispatch — sent below as a real signed extrinsic.
			let signers = vec![account.clone(), cosigner.clone()];
			assert_ok!(pallet_multisig::Pallet::<Runtime>::create_multisig(
				RuntimeOrigin::signed(account.clone()),
				signers.clone(),
				1,
				0,
			));
			let multisig_address =
				pallet_multisig::Pallet::<Runtime>::derive_multisig_address(&signers, 1, 0);
			Balances::make_free_balance_be(&multisig_address, 10 * UNIT);

			// The sweeps run as derivatives *of the multisig*: it is the
			// origin the stored call is dispatched with.
			let derivatives: Vec<AccountId32> = (0..REAPS as u16)
				.map(|index| pallet_utility::derivative_account_id(multisig_address.clone(), index))
				.collect();
			for derivative in &derivatives {
				Balances::make_free_balance_be(derivative, UNIT);
			}

			let encoded: pallet_multisig::BoundedCallOf<Runtime> =
				stored_call(keep_alive).encode().try_into().expect("call fits MaxCallSize");
			assert_ok!(pallet_multisig::Pallet::<Runtime>::propose(
				RuntimeOrigin::signed(account.clone()),
				multisig_address.clone(),
				encoded,
				System::block_number() + 100,
			));

			let dest_before = Balances::free_balance(&dest);
			// The executor resubmits the stored call byte-for-byte — this is
			// what makes the extrinsic self-describing for admission and fees.
			let execute_call: RuntimeCall = pallet_multisig::Call::execute {
				multisig_address: multisig_address.clone(),
				proposal_id: 0,
				call: Box::new(stored_call(keep_alive)),
			}
			.into();
			let xt = signed_call(&pair, account.clone(), execute_call, 0, false);
			let declared = xt.get_dispatch_info().total_weight();
			assert_ok!(Executive::apply_extrinsic(xt).expect("extrinsic is valid"));

			// The stored call really ran: the sweeps reached `dest`, and the
			// derivative accounts died exactly when keep_alive was off.
			assert!(Balances::free_balance(&dest) > dest_before, "sweeps must have executed");
			for derivative in &derivatives {
				assert_eq!(
					frame_system::Account::<Runtime>::contains_key(derivative),
					keep_alive,
					"keep_alive={keep_alive}: derivative reap expectation"
				);
			}
			(declared, fee_paid(&account))
		})
	};

	let (keep_declared, keep_fee) = run(true);
	let (reap_declared, reap_fee) = run(false);

	// (a) Admission: the resubmitted call is inspected pre-dispatch, so the
	// reaping variant's declared weight — what `CheckWeight` admits against
	// block limits — exceeds the non-reaping control's by exactly the three
	// counted cleanups. (The two variants differ only in the `keep_alive`
	// bool: same encoded size, same inner call weight, same transfer count.)
	let cleanup = pallet_pubkey::Pallet::<Runtime>::reap_cleanup_weight();
	assert_eq!(
		reap_declared,
		keep_declared.saturating_add(cleanup.saturating_mul(REAPS)),
		"declared admission weight must count each cleanup in the resubmitted call"
	);
	let execute_shape: RuntimeCall = pallet_multisig::Call::execute {
		multisig_address: dest.clone(),
		proposal_id: 0,
		call: Box::new(stored_call(false)),
	}
	.into();
	assert_eq!(
		ChargePubkeyCacheVerify::new().weight(&execute_shape),
		quantus_runtime::configs::PubkeyCacheVerifyWeight::get()
			.saturating_add(cleanup.saturating_mul(REAPS)),
		"execute's extension weight must price the cleanups its inner call can cause"
	);

	// (b) Fees: all three charged cleanups were realized, so nothing is
	// refunded and the payer's delta over the non-reaping control is exactly
	// three cleanups.
	let cleanup_fee =
		pallet_transaction_payment::Pallet::<Runtime>::weight_to_fee(cleanup.saturating_mul(REAPS));
	assert!(cleanup_fee > 0, "the cleanups must convert to a nonzero fee");
	assert_eq!(
		reap_fee - keep_fee,
		cleanup_fee,
		"a multisig-executed call must pay for every pubkey-cache cleanup it caused"
	);
}
