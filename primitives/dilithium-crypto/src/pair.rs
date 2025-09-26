use crate::{DilithiumSignatureScheme, DilithiumSignatureWithPublic, DilithiumSigner};

use super::types::{DilithiumPair, DilithiumPublic};
use alloc::vec::Vec;
use qp_rusty_crystals_dilithium::{
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
	type Seed = [u8; 32];
	type Signature = DilithiumSignatureWithPublic;

	fn derive<Iter: Iterator<Item = DeriveJunction>>(
		&self,
		_path_iter: Iter,
		seed: Option<<DilithiumPair as Pair>::Seed>,
	) -> Result<(Self, Option<<DilithiumPair as Pair>::Seed>), DeriveError> {
		// TODO: derive child keys from path
		Ok((self.clone(), seed))
	}

	fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
		DilithiumPair::from_seed(seed).map_err(|_| SecretStringError::InvalidSeed)
	}

	#[cfg(feature = "full_crypto")]
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

	// NOTE: This method does not parse all secret uris correctly, like
	// "mnemonic///password///account" This was supported in standard substrate, if there is
	// demand, we can support it in the future
	fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError> {
		let res = Self::from_phrase(s, password_override)
			.map_err(|_| SecretStringError::InvalidPhrase)?;
		Ok(res.0)
	}

	#[cfg(feature = "std")]
	fn from_phrase(
		phrase: &str,
		password: Option<&str>,
	) -> Result<(Self, Self::Seed), SecretStringError> {
		use qp_rusty_crystals_hdwallet::HDLattice;
		let hd = HDLattice::from_mnemonic(phrase, password)
			.map_err(|_| SecretStringError::InvalidPhrase)?;
		let keypair = hd.generate_keys();
		let pair = DilithiumPair { secret: keypair.secret.bytes, public: keypair.public.bytes };
		let mut seed = [0u8; 32];
		seed.copy_from_slice(&hd.seed[..32]);
		Ok((pair, seed))
	}

	#[cfg(feature = "std")]
	fn from_string_with_seed(
		s: &str,
		password: Option<&str>,
	) -> Result<(Self, Option<Self::Seed>), SecretStringError> {
		use qp_rusty_crystals_hdwallet::HDLattice;
		// For Dilithium, we use the string directly as entropy for key generation
		// We combine the string with the password if provided
		let hd =
			HDLattice::from_mnemonic(s, password).map_err(|_| SecretStringError::InvalidPhrase)?;
		let keypair = hd.generate_keys();
		let pair = DilithiumPair { secret: keypair.secret.bytes, public: keypair.public.bytes };

		// Return the pair with no seed since Dilithium doesn't use traditional seed-based
		// generation
		Ok((pair, None))
	}
}

#[cfg(feature = "std")]
impl DilithiumPublic {
	/// Attempt to parse a Dilithium public key from a string and return it with the
	/// associated SS58 address format (version).
	///
	/// This inherent method is provided to avoid relying solely on the generic Ss58Codec
	/// behavior which expects SS58-encoded keys of the same length as the public key.
	/// For Dilithium, we primarily support hex-encoded public keys (0x-prefixed) here.
	///
	/// Note: SS58 AccountId32 addresses are not convertible back into Dilithium public
	/// keys. The CLI already includes a fallback to parse AccountId32 addresses when
	/// this function returns an error.
	pub fn from_string_with_version(
		s: &str,
	) -> Result<(Self, sp_core::crypto::Ss58AddressFormat), sp_core::crypto::PublicError> {
		use sp_core::crypto::{default_ss58_version, PublicError};
		// Accept 0x-prefixed hex of the raw Dilithium public key bytes.
		let maybe_hex = s.strip_prefix("0x").unwrap_or(s);
		// Expect exact hex length for a Dilithium public key
		let expected_hex_len = <Self as ByteArray>::LEN * 2;
		if maybe_hex.len() == expected_hex_len && maybe_hex.chars().all(|c| c.is_ascii_hexdigit()) {
			let mut bytes = vec![0u8; <Self as ByteArray>::LEN];
			for (i, chunk) in maybe_hex.as_bytes().chunks(2).enumerate() {
				let h = (chunk[0] as char).to_digit(16).ok_or(PublicError::InvalidFormat)? as u8;
				let l = (chunk[1] as char).to_digit(16).ok_or(PublicError::InvalidFormat)? as u8;
				bytes[i] = (h << 4) | l;
			}
			let pk = <Self as ByteArray>::from_slice(&bytes).map_err(|_| PublicError::BadLength)?;
			return Ok((pk, default_ss58_version()));
		}
		// Not a supported Dilithium public key representation here.
		Err(PublicError::InvalidFormat)
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
	use super::*;

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
		let message = b"Something";
		let signature = pair.sign(message);

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
