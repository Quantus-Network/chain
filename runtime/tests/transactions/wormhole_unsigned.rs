//! The wormhole settlement calls (`verify_private_batch` / `verify_public_batch`) do
//! not run the ZK verifier in their body. The verifier runs in
//! `ValidateUnsigned::pre_dispatch`, which the executive invokes only for a bare
//! unsigned extrinsic and which leaves a `VerifiedProofHash` marker the dispatchable
//! must consume. These tests pin both layers against the production runtime:
//!
//! 1. through `Executive::apply_extrinsic`, a genuine proof settles and a proof that parses but
//!    does not verify is rejected before the dispatchable runs;
//! 2. the dispatchable itself refuses any proof that did not pass `pre_dispatch` in the same
//!    extrinsic, so a `None`-origin dispatch from anywhere else mints nothing;
//! 3. as defence in depth, no other way of dispatching a `RuntimeCall` in this runtime can hand
//!    those calls a `None` origin — `batch_all` refuses `None`, governance enacts with `Root`, the
//!    scheduler's calls are disabled, and `dispatch_as` is not compiled in.
//!
//! The fixture is the pallet's real private-batch proof; "tampered" below means the
//! proof bytes are corrupted while the public inputs (serialized after the proof)
//! stay intact, so every cheap structural check passes and only the verifier fails.

use crate::common::TestCommons;
use codec::Encode;
use frame_support::{
	assert_noop, assert_ok,
	traits::{schedule::DispatchTime, Bounded},
};
use frame_system::RawOrigin;
use pallet_referenda::TracksInfo;
use qp_wormhole_verifier::{parse_private_batch_public_inputs, ProofWithPublicInputs, C, F};
use quantus_runtime::{
	configs::TechReferendaInstance, governance::definitions::TechCollectiveTracksInfo, Balances,
	Executive, OriginCaller, Runtime, RuntimeCall, RuntimeOrigin, System, TechCollective,
	TechReferenda, UncheckedExtrinsic, Utility,
};
use sp_core::H256;
use sp_runtime::{
	traits::{BadOrigin, Dispatchable},
	transaction_validity::{InvalidTransaction, TransactionValidityError},
	AccountId32, MultiAddress,
};

const D: usize = 2;

const PRIVATE_BATCH_PROOF_HEX: &str =
	include_str!("../../../pallets/wormhole/test-data/private_batch.hex");

struct Fixture {
	bytes: Vec<u8>,
	block_number: u32,
	block_hash: [u8; 32],
	exit_account: AccountId32,
	nullifiers: Vec<[u8; 32]>,
}

impl Fixture {
	fn load() -> Self {
		let bytes = hex::decode(PRIVATE_BATCH_PROOF_HEX.trim()).expect("fixture is valid hex");
		let verifier = pallet_wormhole::get_private_batch_verifier().expect("verifier available");
		let proof = ProofWithPublicInputs::<F, C, D>::from_bytes(
			bytes.clone(),
			&verifier.circuit_data.common,
		)
		.expect("fixture deserializes");
		let inputs = parse_private_batch_public_inputs(&proof).expect("fixture PIs parse");

		let exit_account = inputs
			.account_data
			.iter()
			.find(|a| a.summed_output_amount > 0 && a.exit_account.as_ref() != [0u8; 32])
			.map(|a| {
				let raw: [u8; 32] = a.exit_account.as_ref().try_into().unwrap();
				AccountId32::new(raw)
			})
			.expect("fixture carries a mintable exit");
		let nullifiers = inputs
			.nullifiers
			.iter()
			.map(|n| n.as_ref().try_into().unwrap())
			.filter(|n: &[u8; 32]| *n != [0u8; 32])
			.collect();

		Self {
			bytes,
			block_number: inputs.block_data.block_number,
			block_hash: inputs.block_data.block_hash.as_ref().try_into().unwrap(),
			exit_account,
			nullifiers,
		}
	}

	/// Pin the proof's block binding so the cheap checks pass and only the
	/// verifier decides.
	fn bind_block(&self) {
		frame_system::BlockHash::<Runtime>::insert(self.block_number, H256(self.block_hash));
		System::set_block_number(self.block_number + 10);
	}

	/// The proof with its first 64 bytes flipped: still parses canonically with
	/// intact public inputs, but fails `verifier.verify`.
	fn tampered_bytes(&self) -> Vec<u8> {
		let mut bytes = self.bytes.clone();
		for byte in bytes.iter_mut().take(64) {
			*byte ^= 0xFF;
		}
		bytes
	}

	fn call(proof_bytes: Vec<u8>) -> RuntimeCall {
		RuntimeCall::Wormhole(pallet_wormhole::Call::verify_private_batch { proof_bytes })
	}

	fn assert_nothing_minted(&self) {
		assert_eq!(Balances::free_balance(&self.exit_account), 0);
		assert!(self
			.nullifiers
			.iter()
			.all(|n| !pallet_wormhole::Pallet::<Runtime>::used_nullifiers(n)));
	}
}

/// Control: the genuine proof, as a bare unsigned extrinsic through the executive,
/// passes `pre_dispatch` and settles. Without this the rejection tests below could
/// pass for reasons unrelated to the verifier.
#[test]
fn bare_unsigned_valid_proof_settles_through_the_executive() {
	TestCommons::new_test_ext().execute_with(|| {
		let fx = Fixture::load();
		fx.bind_block();
		fx.assert_nothing_minted();

		let xt = UncheckedExtrinsic::new_bare(Fixture::call(fx.bytes.clone()));
		let outcome = Executive::apply_extrinsic(xt).expect("genuine proof passes pre_dispatch");
		assert_ok!(outcome);

		assert!(Balances::free_balance(&fx.exit_account) > 0);
		assert!(fx.nullifiers.iter().all(pallet_wormhole::Pallet::<Runtime>::used_nullifiers));
	});
}

/// The fake-proof shape (parses, round-trips, intact public inputs, zero real
/// cryptography) is rejected by the executive before the dispatchable runs: no
/// mint, no nullifier writes, no issuance change.
#[test]
fn bare_unsigned_tampered_proof_is_rejected_before_dispatch_and_mints_nothing() {
	TestCommons::new_test_ext().execute_with(|| {
		let fx = Fixture::load();
		fx.bind_block();
		let issuance_before = Balances::total_issuance();

		let xt = UncheckedExtrinsic::new_bare(Fixture::call(fx.tampered_bytes()));
		assert_eq!(
			Executive::apply_extrinsic(xt).unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Call)
		);

		fx.assert_nothing_minted();
		assert_eq!(Balances::total_issuance(), issuance_before);
	});
}

/// Nobody can mint without going through the verifier: a `None`-origin dispatch that
/// bypasses `pre_dispatch` (what a `dispatch_as`-style wrapper, a scheduled call, or
/// a hook would do) is refused by the dispatchable itself with `ProofNotVerified`,
/// for genuine and tampered bytes alike, and mints nothing.
#[test]
fn none_origin_dispatch_without_pre_dispatch_is_refused() {
	TestCommons::new_test_ext().execute_with(|| {
		let fx = Fixture::load();
		fx.bind_block();

		for bytes in [fx.bytes.clone(), fx.tampered_bytes()] {
			assert_eq!(
				Fixture::call(bytes).dispatch(RuntimeOrigin::none()).unwrap_err().error,
				pallet_wormhole::Error::<Runtime>::ProofNotVerified.into()
			);
		}
		fx.assert_nothing_minted();
	});
}

/// Governance enactment, the scheduler, and multisig all dispatch with `Root` or
/// `Signed`; neither reaches the settlement body.
#[test]
fn verify_batch_rejects_root_and_signed_origins() {
	TestCommons::new_test_ext().execute_with(|| {
		let fx = Fixture::load();
		fx.bind_block();

		for origin in [RuntimeOrigin::root(), RuntimeOrigin::signed(TestCommons::account_id(1))] {
			assert_noop!(Fixture::call(fx.tampered_bytes()).dispatch(origin), BadOrigin);
		}
		fx.assert_nothing_minted();
	});
}

/// `batch_all` is the only utility combinator compiled in. It refuses a `None`
/// outer origin, and a `Root` batch re-dispatches its children as `Root`, so it
/// cannot launder a `None` origin into the wormhole call.
#[test]
fn batch_all_cannot_deliver_a_none_origin_to_verify_batch() {
	TestCommons::new_test_ext().execute_with(|| {
		let fx = Fixture::load();
		fx.bind_block();

		assert_eq!(
			Utility::batch_all(RuntimeOrigin::none(), vec![Fixture::call(fx.tampered_bytes())])
				.unwrap_err()
				.error,
			BadOrigin.into()
		);
		assert_eq!(
			Utility::batch_all(RuntimeOrigin::root(), vec![Fixture::call(fx.tampered_bytes())])
				.unwrap_err()
				.error,
			BadOrigin.into()
		);
		fx.assert_nothing_minted();
	});
}

/// A referendum dispatches its stored `proposal_origin` verbatim at enactment, so
/// the lane must not accept `None` as a proposal origin.
#[test]
fn tech_referenda_accept_no_none_proposal_origin() {
	TestCommons::new_test_ext().execute_with(|| {
		let none = OriginCaller::system(RawOrigin::None);
		assert!(TechCollectiveTracksInfo::track_for(&none).is_err());

		let member = TestCommons::account_id(1);
		assert_ok!(TechCollective::add_member(
			RuntimeOrigin::root(),
			MultiAddress::from(member.clone())
		));
		let proposal = Bounded::Inline(
			RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
				.encode()
				.try_into()
				.unwrap(),
		);
		assert_noop!(
			TechReferenda::submit(
				RuntimeOrigin::signed(member),
				Box::new(none),
				proposal,
				DispatchTime::After(0),
			),
			pallet_referenda::Error::<Runtime, TechReferendaInstance>::NoTrack
		);
	});
}

/// Compile-time guard on the utility call surface. Re-adding `dispatch_as` (or any
/// origin-substituting combinator) would reopen a `None`-origin route into the
/// settlement calls; this match stops compiling when a variant appears so the
/// change gets reviewed against the module docs above.
#[allow(dead_code)]
fn utility_call_surface_is_batch_all_only(call: pallet_utility::Call<Runtime>) {
	match call {
		pallet_utility::Call::batch_all { .. } => {},
		pallet_utility::Call::__Ignore(_, _) => {},
	}
}
