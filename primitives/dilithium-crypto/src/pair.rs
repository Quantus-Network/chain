use super::types::Dilithium87Pair;
use qp_rusty_crystals_dilithium::{
	ml_dsa_87::{Keypair, PublicKey, SecretKey},
	params::SEEDBYTES,
	SensitiveBytes32,
};
use sp_core::Pair;

pub fn crystal_alice() -> Dilithium87Pair {
	let seed = [0u8; 32];
	Dilithium87Pair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn dilithium_bob() -> Dilithium87Pair {
	let seed = [1u8; 32];
	Dilithium87Pair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn crystal_charlie() -> Dilithium87Pair {
	let seed = [2u8; 32];
	Dilithium87Pair::from_seed_slice(&seed).expect("Always succeeds")
}

/// Generates a new Dilithium ML-DSA-87 keypair
///
/// # Arguments
/// * `entropy` - Optional entropy bytes for key generation. Must be at least SEEDBYTES long if
///   provided.
///
/// # Returns
/// `Ok(Keypair)` on success, `Err(Error)` on failure
///
/// # Errors
/// Returns an error if the provided entropy is shorter than SEEDBYTES
pub fn generate(entropy: &[u8]) -> Result<Keypair, crate::types::Error> {
	if entropy.len() < SEEDBYTES {
		return Err(crate::types::Error::InsufficientEntropy {
			required: SEEDBYTES,
			actual: entropy.len(),
		});
	}
	let mut entropy_array = [0u8; 32];
	entropy_array.copy_from_slice(&entropy[..32]);
	let mut sensitive_entropy = SensitiveBytes32::from(&mut entropy_array);
	Ok(Keypair::generate(&mut sensitive_entropy))
}

/// Creates a keypair from existing public and secret key bytes
///
/// # Arguments
/// * `public_key` - The public key bytes
/// * `secret_key` - The secret key bytes
///
/// # Returns
/// `Ok(Keypair)` on success, `Err(Error)` on failure
///
/// # Errors
/// Returns an error if either key fails to parse
pub fn create_keypair(
	public_key: &[u8],
	secret_key: &[u8],
) -> Result<Keypair, crate::types::Error> {
	let secret =
		SecretKey::from_bytes(secret_key).map_err(|_| crate::types::Error::InvalidSecretKey)?;
	let public =
		PublicKey::from_bytes(public_key).map_err(|_| crate::types::Error::InvalidPublicKey)?;

	// from_parts also validates that the public key corresponds to the secret.
	let keypair =
		Keypair::from_parts(secret, public).map_err(|_| crate::types::Error::InvalidPublicKey)?;
	Ok(keypair)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Dilithium65Pair, Dilithium87SignatureWithPublic};
	use sp_core::ByteArray;

	fn setup() {
		// Initialize the logger once per test run
		// Using try_init to avoid panics if called multiple times
		let _ = env_logger::try_init();
	}

	#[test]
	fn test_sign_and_verify() {
		setup();

		let seed = vec![0u8; 32];

		let pair = Dilithium87Pair::from_seed_slice(&seed).expect("Failed to create pair");
		let message = b"Something";
		let signature = pair.sign(message);

		let public = pair.public();

		let result = Dilithium87Pair::verify(&signature, message, &public);

		assert!(result, "Signature should verify");
	}

	#[test]
	fn test_sign_different_message_fails() {
		let seed = [0u8; 32];
		let pair = Dilithium87Pair::from_seed(&seed).expect("Failed to create pair");
		let message = b"Hello, world!";
		let wrong_message = b"Goodbye, world!";

		let signature = pair.sign(message);
		let public = pair.public();

		assert!(
			!Dilithium87Pair::verify(&signature, wrong_message, &public),
			"Signature should not verify with wrong message"
		);
	}

	#[test]
	fn test_wrong_signature_fails() {
		let seed = [0u8; 32];
		let pair = Dilithium87Pair::from_seed(&seed).expect("Failed to create pair");
		let message = b"Hello, world!";

		let mut signature = pair.sign(message);
		let signature_bytes = signature.as_mut();
		// Corrupt the signature by flipping a bit
		if let Some(byte) = signature_bytes.get_mut(0) {
			*byte ^= 1;
		}
		let false_signature = Dilithium87SignatureWithPublic::from_slice(signature_bytes)
			.expect("Failed to create signature");
		let public = pair.public();

		assert!(
			!Dilithium87Pair::verify(&false_signature, message, &public),
			"Corrupted signature should not verify"
		);
	}

	#[test]
	fn test_different_seed_different_public() {
		let seed1 = vec![0u8; 32];
		let seed2 = vec![1u8; 32];
		let pair1 = Dilithium87Pair::from_seed(&seed1).expect("Failed to create pair");
		let pair2 = Dilithium87Pair::from_seed(&seed2).expect("Failed to create pair");

		let pub1 = pair1.public();
		let pub2 = pair2.public();

		assert_ne!(
			pub1.as_ref(),
			pub2.as_ref(),
			"Different seeds should produce different public keys"
		);
	}

	const TEST_PHRASE: &str =
		"sample split bamboo west visual approve brain fox arch impact relief smile";

	/// The seed returned by `from_phrase` must control the very same account as the
	/// returned pair: users back up the displayed "secret seed" and expect to recover
	/// their account from it with `from_seed` if the mnemonic is lost.
	#[test]
	fn test_from_phrase_returned_seed_reconstructs_same_pair() {
		let (pair, seed) = DilithiumPair::from_phrase(TEST_PHRASE, None).expect("valid phrase");
		let restored = DilithiumPair::from_seed(&seed).expect("valid seed");
		assert_eq!(
			pair.public_bytes(),
			restored.public_bytes(),
			"seed returned by from_phrase must reconstruct the same account"
		);
		assert_eq!(pair.secret_bytes(), restored.secret_bytes());
	}

	/// Same as above, with a password.
	#[test]
	fn test_from_phrase_returned_seed_reconstructs_same_pair_with_password() {
		let password = Some("hunter2");
		let (pair, seed) = DilithiumPair::from_phrase(TEST_PHRASE, password).expect("valid phrase");
		let restored = DilithiumPair::from_seed(&seed).expect("valid seed");
		assert_eq!(
			pair.public_bytes(),
			restored.public_bytes(),
			"seed returned by from_phrase must reconstruct the same account"
		);
	}

	/// `from_string_with_seed` shall expose the same faithful seed for mnemonic inputs.
	#[test]
	fn test_from_string_with_seed_returns_matching_seed() {
		let (pair, seed) =
			DilithiumPair::from_string_with_seed(TEST_PHRASE, None).expect("valid phrase");
		let seed = seed.expect("a faithful seed is available for mnemonic inputs");
		let restored = DilithiumPair::from_seed(&seed).expect("valid seed");
		assert_eq!(pair.public_bytes(), restored.public_bytes());
	}

	/// Guard: `from_phrase` must keep deriving the keypair through the default HD path,
	/// otherwise existing accounts created from mnemonics would change address.
	#[test]
	fn test_from_phrase_matches_hd_derivation() {
		let keypair = qp_rusty_crystals_hdwallet::derive_key_from_mnemonic(
			TEST_PHRASE,
			None,
			"m/44'/189189'/0'/0'/0'",
		)
		.expect("valid phrase");
		let expected = DilithiumPair::from_keypair(keypair);
		let (pair, _) = DilithiumPair::from_phrase(TEST_PHRASE, None).expect("valid phrase");
		assert_eq!(
			pair.public_bytes(),
			expected.public_bytes(),
			"from_phrase must not change the account derived from a mnemonic"
		);
	}

	/// `zeroize()` must scrub the secret key material.
	#[test]
	fn test_zeroize_clears_secret() {
		use zeroize::Zeroize;
		let mut pair = DilithiumPair::from_seed(&[7u8; 32]).expect("valid seed");
		assert!(pair.secret_bytes().iter().any(|b| *b != 0));
		pair.zeroize();
		assert!(pair.secret_bytes().iter().all(|b| *b == 0));
	}

	/// Compile-time guarantee that dropped pairs (including clones) wipe their secret.
	#[test]
	fn test_pair_zeroizes_on_drop() {
		fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
		assert_zeroize_on_drop::<DilithiumPair>();
	}

	#[test]
	fn test_from_raw_matching_keys_succeeds() {
		let seed = [0u8; 32];
		let pair = Dilithium87Pair::from_seed(&seed).expect("Failed to create pair");
		let public = pair.public().as_ref().to_vec();
		let secret = pair.secret_bytes().to_vec();
		let restored =
			Dilithium87Pair::from_raw(&public, &secret).expect("Matching keys should succeed");
		assert_eq!(restored.public().as_ref(), pair.public().as_ref());
	}

	#[test]
	fn test_from_raw_mismatched_keys_fails() {
		let seed1 = [0u8; 32];
		let seed2 = [1u8; 32];
		let pair1 = Dilithium87Pair::from_seed(&seed1).expect("Failed to create pair1");
		let pair2 = Dilithium87Pair::from_seed(&seed2).expect("Failed to create pair2");
		// Swap: pair1's secret with pair2's public - should fail validation
		let result = Dilithium87Pair::from_raw(pair2.public().as_ref(), pair1.secret_bytes());
		assert!(result.is_err(), "Mismatched public/secret should be rejected");
	}

	#[test]
	fn test_ml_dsa_65_sign_and_verify() {
		let seed = [0u8; 32];
		let pair = Dilithium65Pair::from_seed_slice(&seed).expect("Failed to create pair");
		let message = b"Something";
		let signature = pair.sign(message);
		let public = pair.public();

		assert!(Dilithium65Pair::verify(&signature, message, &public));
	}

	#[test]
	fn test_ml_dsa_65_wrong_message_fails() {
		let seed = [0u8; 32];
		let pair = Dilithium65Pair::from_seed(&seed).expect("Failed to create pair");
		let signature = pair.sign(b"Hello, world!");
		let public = pair.public();

		assert!(
			!Dilithium65Pair::verify(&signature, b"Goodbye, world!", &public),
			"Signature should not verify with wrong message"
		);
	}

	#[test]
	fn test_ml_dsa_65_wrong_signature_fails() {
		let seed = [0u8; 32];
		let pair = Dilithium65Pair::from_seed(&seed).expect("Failed to create pair");
		let message = b"Hello, world!";

		let mut signature = pair.sign(message);
		let signature_bytes = signature.as_mut();
		if let Some(byte) = signature_bytes.get_mut(0) {
			*byte ^= 1;
		}
		let false_signature = crate::Dilithium65SignatureWithPublic::from_slice(signature_bytes)
			.expect("Failed to create signature");
		let public = pair.public();

		assert!(
			!Dilithium65Pair::verify(&false_signature, message, &public),
			"Corrupted signature should not verify"
		);
	}

	#[test]
	fn test_ml_dsa_65_from_raw_mismatched_keys_fails() {
		let pair1 = Dilithium65Pair::from_seed(&[0u8; 32]).expect("Failed to create pair1");
		let pair2 = Dilithium65Pair::from_seed(&[1u8; 32]).expect("Failed to create pair2");
		let result = Dilithium65Pair::from_raw(pair2.public().as_ref(), pair1.secret_bytes());
		assert!(result.is_err(), "Mismatched public/secret should be rejected");
	}

	#[test]
	fn test_schemes_produce_different_accounts() {
		use sp_runtime::traits::IdentifyAccount;

		let seed = [0u8; 32];
		let pair87 = Dilithium87Pair::from_seed(&seed).expect("Failed to create 87 pair");
		let pair65 = Dilithium65Pair::from_seed(&seed).expect("Failed to create 65 pair");

		// The security property: the same seed must not yield the same on-chain
		// account under the two schemes.
		assert_ne!(
			pair87.public().into_account(),
			pair65.public().into_account(),
			"ML-DSA-87 and ML-DSA-65 keys must derive different AccountIds"
		);
	}

	#[test]
	fn test_from_phrase_works_for_both_schemes() {
		// Well-known test mnemonic; proves HD derivation is wired up for both parameter sets
		// (this is the path the CLI key commands use).
		let phrase = "legal winner thank year wave sausage worth useful legal winner thank yellow";

		let (pair87, _) =
			Dilithium87Pair::from_phrase(phrase, None).expect("87 from_phrase failed");
		let (pair87_again, _) =
			Dilithium87Pair::from_phrase(phrase, None).expect("87 from_phrase failed");
		assert_eq!(pair87.public(), pair87_again.public(), "87 derivation must be deterministic");

		let (pair65, _) =
			Dilithium65Pair::from_phrase(phrase, None).expect("65 from_phrase failed");
		let (pair65_again, _) =
			Dilithium65Pair::from_phrase(phrase, None).expect("65 from_phrase failed");
		assert_eq!(pair65.public(), pair65_again.public(), "65 derivation must be deterministic");

		// Same mnemonic, different parameter sets -> different key material.
		assert_ne!(pair87.public().as_ref(), pair65.public().as_ref());

		// The derived 65 pair must produce valid signatures.
		let message = b"mnemonic-derived key";
		let signature = pair65.sign(message);
		assert!(Dilithium65Pair::verify(&signature, message, &pair65.public()));
	}
}
