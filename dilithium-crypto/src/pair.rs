use crate::{DilithiumSignatureScheme, DilithiumSignatureWithPublic, DilithiumSigner};

use super::types::{DilithiumPair, DilithiumPublic};
use alloc::vec::Vec;
use rusty_crystals_dilithium::{
	ml_dsa_87::{Keypair, PublicKey, SecretKey},
	params::SEEDBYTES,
};
use sp_core::{
	crypto::{DeriveError, DeriveJunction, SecretStringError},
	ByteArray, Pair,
};
use sp_runtime::{
	traits::{IdentifyAccount, Verify},
	AccountId32,
};

pub fn crystal_alice() -> DilithiumPair {
	let seed = [0u8; 32];
	DilithiumPair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn dilithium_bob() -> DilithiumPair {
	let seed = [1u8; 32];
	DilithiumPair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn crystal_charlie() -> DilithiumPair {
	let seed = [2u8; 32];
	DilithiumPair::from_seed_slice(&seed).expect("Always succeeds")
}

impl IdentifyAccount for DilithiumPair {
	type AccountId = AccountId32;
	fn into_account(self) -> AccountId32 {
		self.public().into_account()
	}
}

impl Pair for DilithiumPair {
	type Public = DilithiumPublic;
	type Seed = Vec<u8>;
	type Signature = DilithiumSignatureWithPublic;

	fn derive<Iter: Iterator<Item = DeriveJunction>>(
		&self,
		_path_iter: Iter,
		_seed: Option<<DilithiumPair as Pair>::Seed>,
	) -> Result<(Self, Option<<DilithiumPair as Pair>::Seed>), DeriveError> {
		// Dilithium doesn't support hierarchical derivation like BIP32
		// This is a fundamental limitation of the post-quantum signature scheme
		Err(DeriveError::SoftKeyInPath)
	}

	fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
		DilithiumPair::from_seed(seed).map_err(|_| SecretStringError::InvalidSeed)
	}

	#[cfg(any(feature = "default", feature = "full_crypto"))]
	fn sign(&self, message: &[u8]) -> DilithiumSignatureWithPublic {
		// Create keypair struct

		use crate::types::DilithiumSignature;
		let keypair = create_keypair(&self.public, &self.secret).expect("Failed to create keypair");

		// Sign the message
		let signature = keypair.sign(message, None, false);

		let signature =
			DilithiumSignature::try_from(signature.as_ref()).expect("Wrap doesn't fail");

		DilithiumSignatureWithPublic::new(signature, self.public())
	}

	fn verify<M: AsRef<[u8]>>(
		sig: &DilithiumSignatureWithPublic,
		message: M,
		pubkey: &DilithiumPublic,
	) -> bool {
		let sig_scheme = DilithiumSignatureScheme::Dilithium(sig.clone());
		let signer = DilithiumSigner::Dilithium(pubkey.clone());
		sig_scheme.verify(message.as_ref(), &signer.into_account())
	}

	fn public(&self) -> Self::Public {
		DilithiumPublic::from_slice(&self.public).expect("Valid public key bytes")
	}

	fn to_raw_vec(&self) -> Vec<u8> {
		// this is modeled after sr25519 which returns the private key for this method
		self.secret.to_vec()
	}

	#[cfg(feature = "std")]
	fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError> {
		Self::from_string_with_seed(s, password_override).map(|x| x.0)
	}
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
pub fn generate(entropy: Option<&[u8]>) -> Result<Keypair, crate::types::Error> {
	if let Some(entropy_bytes) = entropy {
		if entropy_bytes.len() < SEEDBYTES {
			return Err(crate::types::Error::InsufficientEntropy {
				required: SEEDBYTES,
				actual: entropy_bytes.len(),
			});
		}
	}
	Ok(Keypair::generate(entropy))
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

	let keypair = Keypair { secret, public };
	Ok(keypair)
}

#[cfg(test)]
mod tests {
	use alloc::vec::Vec;

	use super::*;
	use alloc::vec::Vec;

	fn setup() {
		// Initialize the logger once per test run
		// Using try_init to avoid panics if called multiple times
		let _ = env_logger::try_init();
	}

	#[test]
	fn test_sign_and_verify() {
		setup();

		let seed = vec![0u8; 32];

		let pair = DilithiumPair::from_seed_slice(&seed).expect("Failed to create pair");
		let message: Vec<u8> = b"Hello, world!".to_vec();

		log::info!("Signing message: {:?}", &message[..10]);

		let signature = pair.sign(&message);

		log::info!("Signature: {:?}", &message[..10]);

		let public = pair.public();

		let result = DilithiumPair::verify(&signature, message, &public);

		assert!(result, "Signature should verify");
	}

	#[test]
	fn test_sign_different_message_fails() {
		let seed = [0u8; 32];
		let pair = DilithiumPair::from_seed(&seed).expect("Failed to create pair");
		let message = b"Hello, world!";
		let wrong_message = b"Goodbye, world!";

		let signature = pair.sign(message);
		let public = pair.public();

		assert!(
			!DilithiumPair::verify(&signature, wrong_message, &public),
			"Signature should not verify with wrong message"
		);
	}

	#[test]
	fn test_wrong_signature_fails() {
		let seed = [0u8; 32];
		let pair = DilithiumPair::from_seed(&seed).expect("Failed to create pair");
		let message = b"Hello, world!";

		let mut signature = pair.sign(message);
		let signature_bytes = signature.as_mut();
		// Corrupt the signature by flipping a bit
		if let Some(byte) = signature_bytes.get_mut(0) {
			*byte ^= 1;
		}
		let false_signature = DilithiumSignatureWithPublic::from_slice(signature_bytes)
			.expect("Failed to create signature");
		let public = pair.public();

		assert!(
			!DilithiumPair::verify(&false_signature, message, &public),
			"Corrupted signature should not verify"
		);
	}

	#[test]
	fn test_different_seed_different_public() {
		let seed1 = vec![0u8; 32];
		let seed2 = vec![1u8; 32];
		let pair1 = DilithiumPair::from_seed(&seed1).expect("Failed to create pair");
		let pair2 = DilithiumPair::from_seed(&seed2).expect("Failed to create pair");

		let pub1 = pair1.public();
		let pub2 = pair2.public();

		assert_ne!(
			pub1.as_ref(),
			pub2.as_ref(),
			"Different seeds should produce different public keys"
		);
	}
}
