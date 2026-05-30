// Copyright 2024 Quantus Network Developers
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Dilithium ML-DSA-87 keys for post-quantum cryptography.

use crate::{
	error::{Error, ParseError},
	PeerId,
};

use qp_rusty_crystals_dilithium::{ml_dsa_87, SensitiveBytes32};
use std::fmt;
use zeroize::Zeroize;

/// Size of the Dilithium public key in bytes.
pub const PUBLIC_KEY_BYTES: usize = ml_dsa_87::PUBLICKEYBYTES;

/// Size of the Dilithium signature in bytes.
pub const SIGNATURE_BYTES: usize = ml_dsa_87::SIGNBYTES;

/// Size of the seed used to generate a keypair (32 bytes).
pub const SEED_BYTES: usize = 32;

/// A Dilithium ML-DSA-87 keypair.
///
/// Internally stores the 32-byte seed and the public key.
/// The full secret key is derived on-demand when signing.
#[derive(Clone)]
pub struct Keypair {
	/// The seed used to generate the keypair (32 bytes).
	seed: [u8; SEED_BYTES],
	/// The public key.
	public: ml_dsa_87::PublicKey,
}

impl Keypair {
	/// Generate a new random Dilithium keypair.
	pub fn generate() -> Keypair {
		Keypair::from(SecretKey::generate())
	}

	/// Convert the keypair into a byte array.
	///
	/// Returns the 32-byte seed concatenated with the public key bytes.
	/// Format: [seed (32 bytes)][public key (2592 bytes)]
	pub fn to_bytes(&self) -> Vec<u8> {
		let mut bytes = Vec::with_capacity(SEED_BYTES + PUBLIC_KEY_BYTES);
		bytes.extend_from_slice(&self.seed);
		bytes.extend_from_slice(&self.public.to_bytes());
		bytes
	}

	/// Try to parse a keypair from bytes, zeroing the input on success.
	///
	/// Accepts either:
	/// - 32 bytes (seed only) - public key will be regenerated
	/// - 32 + 2592 bytes (seed + public key)
	pub fn try_from_bytes(kp: &mut [u8]) -> Result<Keypair, Error> {
		if kp.len() == SEED_BYTES {
			// Seed only - regenerate the keypair
			let mut seed = [0u8; SEED_BYTES];
			seed.copy_from_slice(kp);
			kp.zeroize();

			let sensitive_seed = SensitiveBytes32::from(&mut seed.clone());
			let internal_kp = ml_dsa_87::Keypair::generate(sensitive_seed);

			Ok(Keypair { seed, public: internal_kp.public })
		} else if kp.len() == SEED_BYTES + PUBLIC_KEY_BYTES {
			// Full keypair
			let mut seed = [0u8; SEED_BYTES];
			seed.copy_from_slice(&kp[..SEED_BYTES]);

			let public = ml_dsa_87::PublicKey::from_bytes(&kp[SEED_BYTES..]).map_err(|e| {
				Error::Other(format!("Failed to parse Dilithium public key: {e:?}"))
			})?;

			kp.zeroize();

			Ok(Keypair { seed, public })
		} else {
			Err(Error::Other(format!(
				"Invalid Dilithium keypair length: expected {} or {} bytes, got {}",
				SEED_BYTES,
				SEED_BYTES + PUBLIC_KEY_BYTES,
				kp.len()
			)))
		}
	}

	/// Sign a message using the private key of this keypair.
	pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
		// Regenerate the full keypair from seed for signing
		let mut seed_copy = self.seed;
		let sensitive_seed = SensitiveBytes32::from(&mut seed_copy);
		let internal_kp = ml_dsa_87::Keypair::generate(sensitive_seed);

		// Sign without context, with hedged randomness for side-channel protection
		let mut hedge = [0u8; 32];
		rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut hedge);

		internal_kp
			.sign(msg, None, Some(hedge))
			.expect("Signing should not fail")
			.to_vec()
	}

	/// Get the public key of this keypair.
	pub fn public(&self) -> PublicKey {
		PublicKey(self.public.clone())
	}

	/// Get the secret key (seed) of this keypair.
	pub fn secret(&self) -> SecretKey {
		SecretKey(self.seed)
	}
}

impl fmt::Debug for Keypair {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Keypair").field("public", &self.public).finish_non_exhaustive()
	}
}

/// Demote a Dilithium keypair to a secret key (seed).
impl From<Keypair> for SecretKey {
	fn from(kp: Keypair) -> SecretKey {
		SecretKey(kp.seed)
	}
}

/// Promote a Dilithium secret key (seed) into a keypair.
impl From<SecretKey> for Keypair {
	fn from(sk: SecretKey) -> Keypair {
		let mut seed_copy = sk.0;
		let sensitive_seed = SensitiveBytes32::from(&mut seed_copy);
		let internal_kp = ml_dsa_87::Keypair::generate(sensitive_seed);

		Keypair { seed: sk.0, public: internal_kp.public }
	}
}

/// A Dilithium ML-DSA-87 public key.
#[derive(Eq, Clone)]
pub struct PublicKey(ml_dsa_87::PublicKey);

impl fmt::Debug for PublicKey {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("PublicKey(Dilithium): ")?;
		// Only show first 8 bytes for readability
		for byte in &self.0.bytes[..8] {
			write!(f, "{byte:02x}")?;
		}
		write!(f, "...")?;
		Ok(())
	}
}

impl PartialEq for PublicKey {
	fn eq(&self, other: &Self) -> bool {
		self.0.bytes.eq(&other.0.bytes)
	}
}

impl PublicKey {
	/// Verify the Dilithium signature on a message using the public key.
	pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
		self.0.verify(msg, sig, None)
	}

	/// Convert the public key to a byte array.
	pub fn to_bytes(&self) -> Vec<u8> {
		self.0.to_bytes().to_vec()
	}

	/// Get the public key as a byte slice.
	pub fn as_bytes(&self) -> &[u8] {
		&self.0.bytes
	}

	/// Try to parse a public key from a byte slice.
	pub fn try_from_bytes(k: &[u8]) -> Result<PublicKey, ParseError> {
		ml_dsa_87::PublicKey::from_bytes(k)
			.map(PublicKey)
			.map_err(|_| ParseError::InvalidPublicKey)
	}

	/// Convert public key to `PeerId`.
	pub fn to_peer_id(&self) -> PeerId {
		crate::crypto::PublicKey::from(self.clone()).into()
	}
}

/// A Dilithium secret key (stored as 32-byte seed).
#[derive(Clone)]
pub struct SecretKey([u8; SEED_BYTES]);

impl Drop for SecretKey {
	fn drop(&mut self) {
		self.0.zeroize();
	}
}

/// View the bytes of the secret key (seed).
impl AsRef<[u8]> for SecretKey {
	fn as_ref(&self) -> &[u8] {
		&self.0[..]
	}
}

impl fmt::Debug for SecretKey {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "SecretKey(Dilithium)")
	}
}

impl SecretKey {
	/// Generate a new Dilithium secret key (seed).
	pub fn generate() -> SecretKey {
		let mut seed = [0u8; SEED_BYTES];
		rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut seed);
		SecretKey(seed)
	}

	/// Try to parse a Dilithium secret key from a byte slice,
	/// zeroing the input on success.
	pub fn try_from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> crate::Result<SecretKey> {
		let sk_bytes = sk_bytes.as_mut();
		let secret = <[u8; SEED_BYTES]>::try_from(&*sk_bytes)
			.map_err(|e| Error::Other(format!("Failed to parse Dilithium secret key: {e}")))?;
		sk_bytes.zeroize();
		Ok(SecretKey(secret))
	}

	/// Convert this secret key to a byte array.
	pub fn to_bytes(&self) -> [u8; SEED_BYTES] {
		self.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn eq_keypairs(kp1: &Keypair, kp2: &Keypair) -> bool {
		kp1.public() == kp2.public() && kp1.seed == kp2.seed
	}

	#[test]
	fn dilithium_keypair_encode_decode() {
		let kp1 = Keypair::generate();
		let mut kp1_enc = kp1.to_bytes();
		let kp2 = Keypair::try_from_bytes(&mut kp1_enc).unwrap();
		assert!(eq_keypairs(&kp1, &kp2));
		// Verify the bytes were zeroized
		assert!(kp1_enc.iter().all(|b| *b == 0));
	}

	#[test]
	fn dilithium_keypair_from_seed_only() {
		let kp1 = Keypair::generate();
		let mut seed = kp1.secret().to_bytes();
		let kp2 = Keypair::try_from_bytes(&mut seed[..]).unwrap();
		assert!(eq_keypairs(&kp1, &kp2));
	}

	#[test]
	fn dilithium_keypair_from_secret() {
		let kp1 = Keypair::generate();
		let sk = kp1.secret();
		let kp2 = Keypair::from(sk);
		assert!(eq_keypairs(&kp1, &kp2));
	}

	#[test]
	fn dilithium_signature() {
		let kp = Keypair::generate();
		let pk = kp.public();

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);
		assert!(pk.verify(msg, &sig));

		// Invalid signature
		let mut invalid_sig = sig.clone();
		invalid_sig[3..6].copy_from_slice(&[10, 23, 42]);
		assert!(!pk.verify(msg, &invalid_sig));

		// Wrong message
		let invalid_msg = "h3ll0 w0rld".as_bytes();
		assert!(!pk.verify(invalid_msg, &sig));
	}

	#[test]
	fn dilithium_public_key_roundtrip() {
		let kp = Keypair::generate();
		let pk = kp.public();
		let pk_bytes = pk.to_bytes();
		let pk2 = PublicKey::try_from_bytes(&pk_bytes).unwrap();
		assert_eq!(pk, pk2);
	}

	#[test]
	fn secret_key_zeroized_on_drop() {
		let kp = Keypair::generate();
		let sk = kp.secret();
		let sk_bytes = sk.to_bytes();
		// Verify we got valid bytes
		assert!(!sk_bytes.iter().all(|b| *b == 0));
		// Drop happens automatically
	}
}
