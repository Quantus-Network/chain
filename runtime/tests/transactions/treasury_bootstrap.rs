//! Pins that `treasury_signer_seed` funds a Planck treasury signer through the
//! full bootstrap — create the multisig, then make the first proposal — via the
//! production signed pipeline (`Executive::apply_extrinsic`), so the transient
//! `MaxInnerCallWeight` inclusion-fee prepay, base/length fees, and the burned
//! multisig fees are all charged for real. The seed and every fee derive from
//! the `FEE_SCALE` dial, so this holds at whatever scale the runtime is
//! compiled with — turning the dial cannot silently strand treasury bootstrap.

use crate::common::TestCommons;
use codec::Encode;
use frame_support::{assert_ok, traits::Currency};
use qp_dilithium_crypto::Dilithium65Pair;
use quantus_runtime::{
	genesis_config_presets::treasury_signer_seed, Balances, Executive, Multisig, RuntimeCall,
	System,
};
use sp_core::Pair;
use sp_runtime::traits::IdentifyAccount;

#[test]
fn treasury_signer_seed_covers_create_and_first_proposal() {
	let proposer_pair = Dilithium65Pair::from_seed_slice(&[61u8; 32]).expect("valid seed");
	let proposer = proposer_pair.public().into_account();
	let signers = vec![proposer.clone(), TestCommons::account_id(2), TestCommons::account_id(3)];

	let mut ext = TestCommons::new_test_ext();
	ext.execute_with(|| {
		System::set_block_number(1);
		Balances::make_free_balance_be(&proposer, treasury_signer_seed(signers.len() as u32));

		let create = RuntimeCall::Multisig(pallet_multisig::Call::create_multisig {
			signers: signers.clone(),
			threshold: 2,
			nonce: 0,
		});
		let outcome = Executive::apply_extrinsic(TestCommons::signed_extrinsic(
			&proposer_pair,
			proposer.clone(),
			create,
			0,
			0,
		))
		.expect("create_multisig must pass validation on the seeded balance");
		assert_ok!(outcome);

		let inner = RuntimeCall::System(frame_system::Call::remark { remark: vec![] }).encode();
		let propose = RuntimeCall::Multisig(pallet_multisig::Call::propose {
			multisig_address: Multisig::derive_multisig_address(&signers, 2, 0),
			call: inner.try_into().expect("within MaxCallSize"),
			expiry: System::block_number() + 100,
		});
		let outcome = Executive::apply_extrinsic(TestCommons::signed_extrinsic(
			&proposer_pair,
			proposer.clone(),
			propose,
			1,
			0,
		))
		.expect("propose must pass validation on the seeded balance");
		assert_ok!(outcome);
	});
}
