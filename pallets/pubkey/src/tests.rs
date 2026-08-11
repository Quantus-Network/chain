use crate::{
	mock::{new_test_ext, Test},
	CachedSignature, Pallet, Pubkeys,
};
use qp_dilithium_crypto::{Dilithium65Pair, Dilithium87Pair, DilithiumSignatureScheme};
use sp_core::Pair;
use sp_runtime::traits::{IdentifyAccount, Verify};

type Sig = CachedSignature<Test>;

const MSG: &[u8] = b"payload bytes";

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

		let sig_only = Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(
			pair.sign(MSG).signature(),
		));
		assert!(sig_only.verify(MSG, &account));
		assert!(!sig_only.verify(&b"different payload"[..], &account));
	});
}

#[test]
fn sig_only_fails_without_cached_pubkey() {
	new_test_ext().execute_with(|| {
		let pair = Dilithium65Pair::from_seed_slice(&[1u8; 32]).unwrap();
		let account = pair.public().into_account();

		let sig_only = Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(
			pair.sign(MSG).signature(),
		));
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
		let sig_only = Sig::from(DilithiumSignatureScheme::Dilithium87SigOnly(
			pair87.sign(MSG).signature(),
		));
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

		let sig_only = Sig::from(DilithiumSignatureScheme::Dilithium65SigOnly(
			pair.sign(MSG).signature(),
		));
		assert!(!sig_only.verify(MSG, &other_account));
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
