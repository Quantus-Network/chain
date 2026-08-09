//! End-to-end tests: extrinsics signed with ML-DSA-65 through the full runtime
//! transaction pipeline (`Executive::apply_extrinsic` with all `TxExtension`s).

use codec::Encode;
use frame_support::{assert_ok, traits::Currency};
use qp_dilithium_crypto::Dilithium65Pair;
use quantus_runtime::{
	transaction_extensions::{ReversibleTransactionExtension, WormholeProofRecorderExtension},
	Balances, BalancesCall, Executive, Runtime, RuntimeCall, Signature, SignedPayload, System,
	TxExtension, UncheckedExtrinsic, UNIT, VERSION,
};
use sp_core::Pair;
use sp_runtime::{generic::Era, traits::IdentifyAccount, AccountId32, MultiAddress};

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

/// Build a `transfer_keep_alive` extrinsic signed by `pair`, claiming `sender`
/// as the transaction origin.
fn signed_transfer(
	pair: &Dilithium65Pair,
	sender: AccountId32,
	dest: AccountId32,
	value: u128,
	nonce: u32,
) -> UncheckedExtrinsic {
	let genesis_hash = System::block_hash(0);
	let call: RuntimeCall =
		BalancesCall::transfer_keep_alive { dest: MultiAddress::Id(dest), value }.into();

	let tx_ext: TxExtension = (
		frame_system::CheckNonZeroSender::<Runtime>::new(),
		frame_system::CheckSpecVersion::<Runtime>::new(),
		frame_system::CheckTxVersion::<Runtime>::new(),
		frame_system::CheckGenesis::<Runtime>::new(),
		frame_system::CheckEra::<Runtime>::from(Era::immortal()),
		frame_system::CheckNonce::<Runtime>::from(nonce),
		frame_system::CheckWeight::<Runtime>::new(),
		pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(0),
		frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
		ReversibleTransactionExtension::<Runtime>::new(),
		WormholeProofRecorderExtension::<Runtime>::new(),
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
			None,
			(),
			(),
			(),
		),
	);
	let signature = raw_payload.using_encoded(|e| pair.sign(e));

	UncheckedExtrinsic::new_signed(
		call,
		MultiAddress::Id(sender),
		Signature::Dilithium65(signature),
		tx_ext,
	)
}

#[test]
fn test_ml_dsa_65_extrinsic_accepted_by_runtime() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		let dest = AccountId32::new([9u8; 32]);
		let xt = signed_transfer(&pair, account.clone(), dest.clone(), 10 * UNIT, 0);

		let outcome = Executive::apply_extrinsic(xt)
			.expect("ML-DSA-65 signed extrinsic should pass validation");
		assert_ok!(outcome);
		assert_eq!(Balances::free_balance(&dest), 10 * UNIT);
	});
}

#[test]
fn test_ml_dsa_65_extrinsic_wrong_signer_rejected() {
	let pair = Dilithium65Pair::from_seed_slice(&[42u8; 32]).expect("valid seed");
	let account = pair.public().into_account();
	let mut ext = test_ext(&account);

	ext.execute_with(|| {
		// The payload is signed by `pair`, but the extrinsic claims a different sender.
		let impostor = AccountId32::new([7u8; 32]);
		let dest = AccountId32::new([9u8; 32]);
		let xt = signed_transfer(&pair, impostor, dest, 10 * UNIT, 0);

		assert!(
			Executive::apply_extrinsic(xt).is_err(),
			"extrinsic with mismatched signer must be rejected"
		);
	});
}
