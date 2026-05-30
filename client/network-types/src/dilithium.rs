// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// Copyright (C) Quantus Network Developers
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Dilithium ML-DSA-87 keys for post-quantum cryptography.
//!
//! This module provides type conversions between:
//! - Substrate's Dilithium types (this module)
//! - litep2p's Dilithium types

use crate::PeerId;
use core::{cmp, fmt, hash};
use litep2p::crypto::dilithium as litep2p_dilithium;
use qp_rusty_crystals_dilithium::{ml_dsa_87, SensitiveBytes32};
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
	pub fn try_from_bytes(kp: &mut [u8]) -> Result<Keypair, DecodingError> {
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

			let public = ml_dsa_87::PublicKey::from_bytes(&kp[SEED_BYTES..])
				.map_err(|e| DecodingError::KeypairParseError(format!("{e:?}").into()))?;

			kp.zeroize();

			Ok(Keypair { seed, public })
		} else {
			Err(DecodingError::KeypairParseError(Box::new(std::io::Error::new(
				std::io::ErrorKind::InvalidData,
				format!(
					"Invalid Dilithium keypair length: expected {} or {} bytes, got {}",
					SEED_BYTES,
					SEED_BYTES + PUBLIC_KEY_BYTES,
					kp.len()
				),
			))))
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

impl From<litep2p_dilithium::Keypair> for Keypair {
	fn from(kp: litep2p_dilithium::Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("litep2p Dilithium keypair to use the same format")
	}
}

impl From<Keypair> for litep2p_dilithium::Keypair {
	fn from(kp: Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("Substrate Dilithium keypair to use the same format")
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

impl cmp::PartialEq for PublicKey {
	fn eq(&self, other: &Self) -> bool {
		self.0.bytes.eq(&other.0.bytes)
	}
}

impl hash::Hash for PublicKey {
	fn hash<H: hash::Hasher>(&self, state: &mut H) {
		self.0.bytes.hash(state);
	}
}

impl cmp::PartialOrd for PublicKey {
	fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl cmp::Ord for PublicKey {
	fn cmp(&self, other: &Self) -> cmp::Ordering {
		self.0.bytes.cmp(&other.0.bytes)
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

	/// Try to parse a public key from a byte slice.
	pub fn try_from_bytes(k: &[u8]) -> Result<PublicKey, DecodingError> {
		ml_dsa_87::PublicKey::from_bytes(k)
			.map(PublicKey)
			.map_err(|e| DecodingError::PublicKeyParseError(format!("{e:?}").into()))
	}

	/// Convert public key to `PeerId`.
	pub fn to_peer_id(&self) -> PeerId {
		let litep2p_pk: litep2p_dilithium::PublicKey = self.clone().into();
		let public_key = litep2p::crypto::PublicKey::from(litep2p_pk);
		litep2p::PeerId::from_public_key(&public_key).into()
	}
}

impl From<litep2p_dilithium::PublicKey> for PublicKey {
	fn from(k: litep2p_dilithium::PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes()).expect("litep2p Dilithium public key to parse")
	}
}

impl From<PublicKey> for litep2p_dilithium::PublicKey {
	fn from(k: PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes()).expect("Substrate Dilithium public key to parse")
	}
}

/// A Dilithium secret key (stored as 32-byte seed).
#[derive(Clone)]
pub struct SecretKey([u8; SEED_BYTES]);

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
	pub fn try_from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
		let sk_bytes = sk_bytes.as_mut();
		let secret = <[u8; SEED_BYTES]>::try_from(&*sk_bytes)
			.map_err(|e| DecodingError::SecretKeyParseError(Box::new(e)))?;
		sk_bytes.zeroize();
		Ok(SecretKey(secret))
	}

	/// Convert this secret key to a byte array.
	pub fn to_bytes(&self) -> [u8; SEED_BYTES] {
		self.0
	}
}

impl Drop for SecretKey {
	fn drop(&mut self) {
		self.0.zeroize();
	}
}

impl From<litep2p_dilithium::SecretKey> for SecretKey {
	fn from(sk: litep2p_dilithium::SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.to_bytes()).expect("Dilithium seed to be 32 bytes")
	}
}

impl From<SecretKey> for litep2p_dilithium::SecretKey {
	fn from(sk: SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.to_bytes())
			.expect("litep2p `SecretKey` to accept 32 bytes as Dilithium seed")
	}
}

/// Error when decoding Dilithium-related types.
#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
	#[error("failed to parse Dilithium keypair: {0}")]
	KeypairParseError(Box<dyn std::error::Error + Send + Sync>),
	#[error("failed to parse Dilithium secret key: {0}")]
	SecretKeyParseError(Box<dyn std::error::Error + Send + Sync>),
	#[error("failed to parse Dilithium public key: {0}")]
	PublicKeyParseError(Box<dyn std::error::Error + Send + Sync>),
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
		let mut seed = kp1.secret().to_bytes().to_vec();
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

		let mut invalid_sig = sig.clone();
		invalid_sig[3..6].copy_from_slice(&[10, 23, 42]);
		assert!(!pk.verify(msg, &invalid_sig));

		let invalid_msg = "h3ll0 w0rld".as_bytes();
		assert!(!pk.verify(invalid_msg, &sig));
	}

	#[test]
	fn substrate_kp_to_litep2p() {
		let kp = Keypair::generate();
		let kp_bytes = kp.to_bytes();
		let kp1: litep2p_dilithium::Keypair = kp.clone().into();

		assert_eq!(kp_bytes, kp1.to_bytes());

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);
		let sig1 = kp1.sign(msg);

		// Note: Dilithium signatures include randomness, so we verify instead of comparing
		let pk = kp.public();
		let pk1 = kp1.public();

		assert!(pk.verify(msg, &sig));
		assert!(pk.verify(msg, &sig1));
		assert!(pk1.verify(msg, &sig));
		assert!(pk1.verify(msg, &sig1));
	}

	#[test]
	fn litep2p_kp_to_substrate_kp() {
		let kp = litep2p_dilithium::Keypair::generate();
		let kp1: Keypair = kp.clone().into();
		let kp2 = Keypair::try_from_bytes(&mut kp.to_bytes()).unwrap();

		assert_eq!(kp.to_bytes(), kp1.to_bytes());

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);

		let pk1 = kp1.public();
		let pk2 = kp2.public();

		assert!(pk1.verify(msg, &sig));
		assert!(pk2.verify(msg, &sig));
	}

	#[test]
	fn substrate_pk_to_litep2p() {
		let kp = Keypair::generate();
		let pk = kp.public();
		let pk_bytes = pk.to_bytes();
		let pk1: litep2p_dilithium::PublicKey = pk.clone().into();

		assert_eq!(pk_bytes, pk1.to_bytes());

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);

		assert!(pk.verify(msg, &sig));
		assert!(pk1.verify(msg, &sig));
	}

	#[test]
	fn litep2p_pk_to_substrate_pk() {
		let kp = litep2p_dilithium::Keypair::generate();
		let pk = kp.public();
		let pk_bytes = pk.clone().to_bytes();
		let pk1: PublicKey = pk.clone().into();
		let pk2 = PublicKey::try_from_bytes(&pk_bytes).unwrap();

		assert_eq!(pk_bytes, pk1.to_bytes());

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);

		assert!(pk.verify(msg, &sig));
		assert!(pk1.verify(msg, &sig));
		assert!(pk2.verify(msg, &sig));
	}

	#[test]
	fn substrate_sk_to_litep2p() {
		let sk = SecretKey::generate();
		let sk1: litep2p_dilithium::SecretKey = sk.clone().into();

		let kp: Keypair = sk.into();
		let kp1: litep2p_dilithium::Keypair = sk1.into();

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);

		// Verify with both keypairs' public keys
		assert!(kp.public().verify(msg, &sig));
		assert!(kp1.public().verify(msg, &sig));
	}

	#[test]
	fn litep2p_sk_to_substrate_sk() {
		let sk = litep2p_dilithium::SecretKey::generate();
		let sk1: SecretKey = sk.clone().into();
		let sk2 = SecretKey::try_from_bytes(&mut sk.to_bytes()).unwrap();

		let kp: litep2p_dilithium::Keypair = sk.into();
		let kp1: Keypair = sk1.into();
		let kp2: Keypair = sk2.into();

		let msg = "hello world".as_bytes();
		let sig = kp.sign(msg);

		assert!(kp1.public().verify(msg, &sig));
		assert!(kp2.public().verify(msg, &sig));
	}
}
