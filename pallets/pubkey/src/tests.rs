use crate::{
	mock::{new_test_ext, Test},
	CachedSignature, Pallet, Pubkeys,
};
use qp_dilithium_crypto::{
	sig_only_signing_payload, Dilithium65Pair, Dilithium65SignatureWithPublic, Dilithium87Pair,
	DilithiumSignatureScheme,
};
use sp_core::Pair;
use sp_runtime::traits::{IdentifyAccount, Verify};

type Sig = CachedSignature<Test>;

const MSG: &[u8] = b"payload bytes";

/// Sign `msg` in the `SigOnly` form: over the domain-separated payload, so the
/// signature commits to the sig-only encoding (see `SIG_ONLY_SIGNING_PREFIX`).
fn sign_sig_only_65(pair: &Dilithium65Pair, msg: &[u8]) -> Sig {
	let sig = pair.sign(&sig_only_signing_payload(msg)).signature();
	Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(sig))
}

fn sign_sig_only_87(pair: &Dilithium87Pair, msg: &[u8]) -> Sig {
	let sig = pair.sign(&sig_only_signing_payload(msg)).signature();
	Sig::from(DilithiumSignatureScheme::Dilithium87SigOnly(sig))
}

#[test]
fn full_signature_verifies_and_caches_pubkey() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();
		assert!(Pallet::<Test>::pubkey_of(&account).is_none());

		let sig = Sig::from(pair.sign(MSG));
		assert!(sig.verify(MSG, &account));

		let cached = Pallet::<Test>::pubkey_of(&account).expect("pubkey must be cached");
		assert_eq!(cached.into_account(), account);
	});
}

#[test]
fn failed_full_signature_caches_nothing() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		let sig = Sig::from(pair.sign(MSG));
		assert!(!sig.verify(&b"different payload"[..], &account));
		assert!(Pallet::<Test>::pubkey_of(&account).is_none());
	});
}

#[test]
fn sig_only_verifies_against_cached_pubkey() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		// Register through a full signature first.
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		let sig_only = sign_sig_only_65(&pair, MSG);
		assert!(sig_only.verify(MSG, &account));
		assert!(!sig_only.verify(&b"different payload"[..], &account));
	});
}

#[test]
fn sig_only_87_verifies_against_cached_pubkey() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium87Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		// Register through a full ML-DSA-87 signature first.
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		let sig_only = sign_sig_only_87(&pair, MSG);
		assert!(sig_only.verify(MSG, &account));
		assert!(!sig_only.verify(&b"different payload"[..], &account));
	});
}

#[test]
fn sig_only_fails_without_cached_pubkey() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		let sig_only = sign_sig_only_65(&pair, MSG);
		assert!(!sig_only.verify(MSG, &account));
	});
}

#[test]
fn sig_only_fails_on_parameter_set_mismatch() {
	new_test_ext().execute_with(|| {
		// Cache an ML-DSA-65 key, then claim an ML-DSA-87 sig-only signature
		// for the same account.
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		let pair87 = Dilithium87Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let sig_only = sign_sig_only_87(&pair87, MSG);
		assert!(!sig_only.verify(MSG, &account));
	});
}

#[test]
fn sig_only_fails_for_wrong_account() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		// Another account with a cached key must not validate signatures it
		// did not produce.
		let other_pair = Dilithium65Pair::from_seed_slice(&[2u8; 32]).unwrap();
		let other_account = other_pair.public().into_account();
		assert!(Sig::from(other_pair.sign(MSG)).verify(MSG, &other_account));

		let sig_only = sign_sig_only_65(&pair, MSG);
		assert!(!sig_only.verify(MSG, &other_account));
	});
}

/// The two encodings of a signature must not be interchangeable: a full
/// signature (over the raw payload) re-wrapped as `SigOnly` must fail, because
/// `SigOnly` verification runs over the domain-separated payload.
#[test]
fn full_signature_rewrapped_as_sig_only_is_rejected() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		// Register, then strip the pubkey off a *full* signature and resubmit
		// the bare signature as SigOnly — exactly what a third party can do
		// with any full extrinsic it observes.
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		let stripped =
			Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(pair.sign(MSG).signature()));
		assert!(!stripped.verify(MSG, &account));
	});
}

/// The reverse direction: a `SigOnly` signature re-wrapped as a full
/// `SignatureWithPublic` (with the world-readable cached key attached) must
/// fail, because a full signature is verified over the raw payload while this
/// one signed the domain-separated payload.
#[test]
fn sig_only_signature_rewrapped_as_full_is_rejected() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();
		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));

		// A valid sig-only signature…
		let sig_only = pair.sign(&sig_only_signing_payload(MSG)).signature();
		assert!(Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(sig_only.clone()))
			.verify(MSG, &account));

		// …inflated back to the full form with the (public) cached key.
		let inflated = Sig::from(DilithiumSignatureScheme::Dilithium65(
			Dilithium65SignatureWithPublic::new(sig_only, pair.public()),
		));
		assert!(!inflated.verify(MSG, &account));
	});
}

#[test]
fn wire_format_matches_inner_enum() {
	use codec::Encode;

	let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
	let inner = DilithiumSignatureScheme::Dilithium65(pair.sign(MSG));
	assert_eq!(Sig::from(inner.clone()).encode(), inner.encode());
}

#[test]
fn full_signature_does_not_overwrite_existing_entry() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));
		let first = Pubkeys::<Test>::get(&account).unwrap();

		assert!(Sig::from(pair.sign(b"second payload")).verify(&b"second payload"[..], &account));
		assert_eq!(Pubkeys::<Test>::get(&account).unwrap(), first);
	});
}

#[test]
fn killed_account_clears_cached_pubkey() {
	use frame_support::traits::OnKilledAccount;

	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		assert!(Sig::from(pair.sign(MSG)).verify(MSG, &account));
		assert!(Pallet::<Test>::pubkey_of(&account).is_some());

		Pallet::<Test>::on_killed_account(&account);
		assert!(Pallet::<Test>::pubkey_of(&account).is_none());
	});
}

/// A reap outside extrinsic application (e.g. a scheduler-enacted transfer in
/// `on_initialize`) has no fee payer, so `on_killed_account` must register its
/// own `Pubkeys::remove` — one DB write, booked as `Mandatory` like other
/// block-lifecycle work — and must not touch the extrinsic-reap counter.
#[test]
fn hook_context_reap_registers_cleanup_weight() {
	use frame_support::{
		dispatch::DispatchClass,
		traits::{Get, OnKilledAccount},
	};
	use sp_runtime::traits::Zero;

	new_test_ext().execute_with(|| {
		let account = sp_runtime::AccountId32::new([1u8; 32]);

		// No `ExecutionPhase` set: outside extrinsic application, like the mock's
		// bare externalities and like `on_initialize`/`on_finalize` contexts.
		assert!(frame_system::Pallet::<Test>::execution_phase().is_none());

		let before = *frame_system::Pallet::<Test>::block_weight().get(DispatchClass::Mandatory);
		Pallet::<Test>::on_killed_account(&account);
		let after = *frame_system::Pallet::<Test>::block_weight().get(DispatchClass::Mandatory);

		let expected = <<Test as frame_system::Config>::DbWeight as Get<
			frame_support::weights::RuntimeDbWeight,
		>>::get()
		.writes(1);
		assert!(
			!expected.is_zero(),
			"mock DbWeight must be nonzero for this test to be meaningful"
		);
		assert_eq!(after.saturating_sub(before), expected);
		assert_eq!(Pallet::<Test>::extrinsic_reaps(), 0, "hook-context reaps are not counted");
	});
}

/// A reap during extrinsic application must increment [`crate::ExtrinsicReaps`]
/// — the signal the runtime's `ChargePubkeyCacheVerify` extension reconciles its
/// statically pre-charged reap-cleanup weight against — and must NOT register
/// block weight itself: the extension already put the cleanup into the
/// extrinsic's declared weight (admission) and actual weight (fee), so a second
/// registration here would double-count it.
#[test]
fn extrinsic_reap_is_counted_not_registered() {
	use frame_support::traits::OnKilledAccount;

	new_test_ext().execute_with(|| {
		let account = sp_runtime::AccountId32::new([1u8; 32]);

		// Enter the extrinsic-application span, exactly as block execution does.
		frame_system::Pallet::<Test>::note_finished_initialize();
		assert!(matches!(
			frame_system::Pallet::<Test>::execution_phase(),
			Some(frame_system::Phase::ApplyExtrinsic(_))
		));

		let weight_before = frame_system::Pallet::<Test>::block_weight().total();
		Pallet::<Test>::on_killed_account(&account);
		Pallet::<Test>::on_killed_account(&account);

		assert_eq!(Pallet::<Test>::extrinsic_reaps(), 2, "each reap must be counted");
		assert_eq!(
			frame_system::Pallet::<Test>::block_weight().total(),
			weight_before,
			"in-extrinsic reaps are charged via the extension, not registered by the hook"
		);
	});
}
