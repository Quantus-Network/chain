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
	transaction_extensions::{ReversibleTransactionExtension, WormholeProofRecorderExtension},
	Balances, BalancesCall, Executive, Runtime, RuntimeCall, Signature, SignedPayload, System,
	TxExtension, UncheckedExtrinsic, UNIT, VERSION,
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
	});
	ext
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
		frame_system::CheckWeight::<Runtime>::new(),
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
			(),
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
/// is charged in the signed `base_extrinsic` surcharge; the `Pubkeys::remove`
/// on account reap is registered as block weight by `pallet_pubkey`'s
/// `OnKilledAccount` hook itself. Together they cover the worst case — a first
/// full-signature `transfer_all(..., keep_alive = false)` that registers and
/// reaps in one extrinsic (one read, two writes).
#[test]
fn first_registration_plus_reap_weight_covers_combined_db_ops() {
	use frame_support::{
		dispatch::DispatchClass,
		weights::constants::{ExtrinsicBaseWeight, RocksDbWeight},
	};
	use quantus_runtime::configs::{PubkeyCacheVerifyWeight, RuntimeBlockWeights};

	let verify = RocksDbWeight::get().reads_writes(1, 1);
	let cleanup = RocksDbWeight::get().writes(1);

	// The base-weight surcharge on signed classes covers the verify-path work.
	assert_eq!(PubkeyCacheVerifyWeight::get(), verify);
	let weights = RuntimeBlockWeights::get();
	for class in [DispatchClass::Normal, DispatchClass::Operational] {
		let base_surcharge =
			weights.get(class).base_extrinsic.saturating_sub(ExtrinsicBaseWeight::get());
		assert!(
			base_surcharge.all_gte(verify),
			"{class:?} base_extrinsic surcharge {base_surcharge:?} must cover verify {verify:?}"
		);
	}

	// The cleanup write is registered where it happens, so it is covered on
	// every reap path by construction. Differential check: two otherwise
	// identical `transfer_all` extrinsics, one of which reaps its sender —
	// the reaping one must consume at least the cleanup DB write.
	//
	// Each extrinsic runs as the sole extrinsic of its own externalities so
	// `WormholeProofRecorderExtension`'s event-scan weight (charged on the
	// cumulative block event count) cannot confound the differential with a
	// positional re-scan of a prior extrinsic's events. Even then the reaping
	// path emits extra events (e.g. `KilledAccount`), so the scan cost still
	// differs — exact registration of the single write is proven by
	// `killed_account_registers_cleanup_weight`; here we only require coverage.
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
				BalancesCall::transfer_all {
					dest: MultiAddress::Id(dest.clone()),
					keep_alive,
				}
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
