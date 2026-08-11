//! End-to-end tests for the on-chain pubkey cache (`pallet-pubkey`): an
//! account's first full `SignatureWithPublic` transaction registers its
//! Dilithium public key, after which `SigOnly` transactions — which omit the
//! ~2 KB public key — pass the full `Executive::apply_extrinsic` pipeline.

use codec::Encode;
use frame_support::{assert_ok, traits::Currency};
use qp_dilithium_crypto::{Dilithium65Pair, Dilithium65Public, DilithiumSignatureScheme};
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

/// Build a signed extrinsic for `call`. With `sig_only` the public key is
/// omitted from the signature, relying on the on-chain cache.
fn signed_call(
	pair: &Dilithium65Pair,
	sender: AccountId32,
	call: RuntimeCall,
	nonce: u32,
	sig_only: bool,
) -> UncheckedExtrinsic {
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
		call.clone(),
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
	let sig_with_public = raw_payload.using_encoded(|e| pair.sign(e));
	let signature: Signature = if sig_only {
		DilithiumSignatureScheme::Dilithium65SigOnly(sig_with_public.signature()).into()
	} else {
		sig_with_public.into()
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

/// A full-signed `transfer_all(..., keep_alive = false)` verifies (and would
/// cache the pubkey) before dispatch reaps the sender. The cache entry must
/// not survive the reap, or fund → register → reap could leave unbounded
/// orphan `Pubkeys` state.
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

		assert!(
			!frame_system::Account::<Runtime>::contains_key(&account),
			"sender must be reaped"
		);
		assert!(
			pallet_pubkey::Pallet::<Runtime>::pubkey_of(&account).is_none(),
			"reaped account must not leave an orphan pubkey cache entry"
		);
	});
}
