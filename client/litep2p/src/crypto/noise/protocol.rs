// Copyright 2019 Parity Technologies (UK) Ltd.
// Copyright 2023 litep2p developers
// Copyright 2025 Quantus Network developers
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

//! Noise protocol implementation using Clatter with pqXX pattern and ML-KEM 768.
//!
//! This implementation uses the NIST-standardized ML-KEM 768 (FIPS 203) for
//! post-quantum key encapsulation, providing ~192-bit security against quantum attacks.

use clatter::{
	bytearray::ByteArray,
	crypto::{cipher::ChaChaPoly, hash::Sha256, kem::rust_crypto_ml_kem::MlKem768},
	handshakepattern::noise_pqxx,
	traits::{Handshaker, Kem},
	transportstate::TransportState,
	PqHandshake,
};
use rand::SeedableRng;
use zeroize::Zeroize;

use crate::error::NegotiationError;

/// Clatter session that manages the pqXX handshake state with ML-KEM 768.
///
/// This struct owns both the RNG and handshake with proper lifetime management.
/// The RNG is boxed for a stable address, allowing the handshake to borrow it.
///
/// # Safety
/// This uses a self-referential pattern: the handshake borrows from the boxed RNG.
/// This is sound because:
/// 1. `_rng` is boxed, giving it a stable heap address
/// 2. We never move or drop `_rng` while `handshake` exists
/// 3. `ClatterSession` is not Clone/Copy, preventing aliasing
/// 4. Struct fields drop in declaration order: `handshake` drops before `_rng`
pub struct ClatterSession {
	/// The RNG - must be boxed for stable address. Kept alive for the handshake's lifetime.
	_rng: Box<rand::rngs::StdRng>,
	/// The handshake state. The 'static lifetime is a lie - it actually borrows from `_rng`.
	handshake: PqHandshake<'static, MlKem768, MlKem768, ChaChaPoly, Sha256, rand::rngs::StdRng>,
}

impl std::fmt::Debug for ClatterSession {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ClatterSession")
			.field("is_initiator", &self.handshake.is_initiator())
			.field("is_finished", &self.handshake.is_finished())
			.finish()
	}
}

impl ClatterSession {
	/// Create a new Clatter session for the pqXX handshake pattern.
	///
	/// # Arguments
	/// * `prologue` - Optional prologue data to bind to the handshake
	/// * `is_initiator` - Whether this is the initiator (dialer) or responder (listener)
	/// * `static_keypair` - The static ML-KEM 768 keypair for authentication
	pub fn new(
		prologue: &[u8],
		is_initiator: bool,
		static_keypair: &Keypair,
	) -> Result<Self, NegotiationError> {
		let kem_secret = <MlKem768 as Kem>::SecretKey::from_slice(static_keypair.secret.as_ref());
		let kem_public = <MlKem768 as Kem>::PubKey::from_slice(static_keypair.public.as_ref());
		let clatter_keypair = clatter::KeyPair { public: kem_public, secret: kem_secret };

		let mut rng = Box::new(rand::rngs::StdRng::from_entropy());

		// Get a raw pointer to the RNG
		let rng_ptr = rng.as_mut() as *mut rand::rngs::StdRng;

		// SAFETY: We're creating a reference that borrows from the boxed RNG.
		// This is sound because:
		// 1. The Box gives a stable heap address that won't move
		// 2. We store the Box in the same struct, so it lives as long as the handshake
		// 3. Struct fields drop in declaration order: handshake drops before _rng
		// 4. We never expose &mut _rng or allow moving it
		let rng_ref: &'static mut rand::rngs::StdRng = unsafe { &mut *rng_ptr };

		let handshake = PqHandshake::<MlKem768, MlKem768, ChaChaPoly, Sha256, _>::new(
			noise_pqxx(),
			prologue,
			is_initiator,
			Some(clatter_keypair),
			None, // No pre-shared ephemeral key
			None, // No remote static key (XX pattern)
			None, // No remote ephemeral key
			rng_ref,
		)
		.map_err(|e| {
			NegotiationError::Clatter(format!("Failed to create pqXX handshake: {:?}", e))
		})?;

		Ok(Self { _rng: rng, handshake })
	}

	/// Write a handshake message.
	pub fn write_message(
		&mut self,
		payload: &[u8],
		message: &mut [u8],
	) -> Result<usize, NegotiationError> {
		self.handshake
			.write_message(payload, message)
			.map_err(|e| NegotiationError::Clatter(format!("pqXX write failed: {:?}", e)))
	}

	/// Read a handshake message.
	pub fn read_message(
		&mut self,
		message: &[u8],
		payload: &mut [u8],
	) -> Result<usize, NegotiationError> {
		self.handshake
			.read_message(message, payload)
			.map_err(|e| NegotiationError::Clatter(format!("pqXX read failed: {:?}", e)))
	}

	/// Get the remote's static public key.
	pub fn get_remote_static(&self) -> Option<Vec<u8>> {
		self.handshake.get_remote_static().map(|k| k.as_slice().to_vec())
	}

	/// Convert to transport state after handshake completion.
	pub fn into_transport_mode(self) -> Result<ClatterTransport, NegotiationError> {
		let transport = self.handshake.finalize().map_err(|e| {
			NegotiationError::Clatter(format!("Failed to finalize pqXX handshake: {:?}", e))
		})?;

		Ok(ClatterTransport(Box::new(transport)))
	}
}

/// Transport state after handshake completion.
pub struct ClatterTransport(Box<TransportState<ChaChaPoly, Sha256>>);

impl std::fmt::Debug for ClatterTransport {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ClatterTransport").finish()
	}
}

impl ClatterTransport {
	/// Write a transport message (encrypt).
	pub fn write_message(
		&mut self,
		plaintext: &[u8],
		ciphertext: &mut [u8],
	) -> Result<usize, NegotiationError> {
		self.0
			.send(plaintext, ciphertext)
			.map_err(|e| NegotiationError::Clatter(format!("Transport write failed: {:?}", e)))
	}

	/// Read a transport message (decrypt).
	pub fn read_message(
		&mut self,
		ciphertext: &[u8],
		plaintext: &mut [u8],
	) -> Result<usize, NegotiationError> {
		self.0
			.receive(ciphertext, plaintext)
			.map_err(|e| NegotiationError::Clatter(format!("Transport read failed: {:?}", e)))
	}
}

/// ML-KEM 768 keypair for Noise static keys.
#[derive(Clone)]
pub struct Keypair {
	pub secret: SecretKey,
	pub public: PublicKey,
}

impl Keypair {
	/// Generate a new ML-KEM 768 keypair.
	pub fn new() -> Self {
		let mut rng = rand::thread_rng();
		let keypair = MlKem768::genkey(&mut rng).expect("ML-KEM key generation should not fail");

		let secret = SecretKey(keypair.secret.as_slice().to_vec());
		let public = PublicKey(keypair.public.as_slice().to_vec());

		Keypair { secret, public }
	}

	/// Get the public key.
	pub fn public(&self) -> &PublicKey {
		&self.public
	}
}

impl Default for Keypair {
	fn default() -> Self {
		Self::new()
	}
}

/// ML-KEM 768 secret key.
#[derive(Clone)]
pub struct SecretKey(Vec<u8>);

impl Drop for SecretKey {
	fn drop(&mut self) {
		self.0.zeroize()
	}
}

impl AsRef<[u8]> for SecretKey {
	fn as_ref(&self) -> &[u8] {
		&self.0
	}
}

/// ML-KEM 768 public key.
#[derive(Clone, PartialEq)]
pub struct PublicKey(Vec<u8>);

impl AsRef<[u8]> for PublicKey {
	fn as_ref(&self) -> &[u8] {
		&self.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// ML-KEM 768 public key size (FIPS 203)
	const ML_KEM_768_PUBLIC_KEY_SIZE: usize = 1184;

	/// ML-KEM 768 secret key size (FIPS 203)
	const ML_KEM_768_SECRET_KEY_SIZE: usize = 2400;

	/// Test helpers for ClatterSession
	impl ClatterSession {
		fn is_initiator(&self) -> bool {
			self.handshake.is_initiator()
		}

		fn is_finished(&self) -> bool {
			self.handshake.is_finished()
		}
	}

	#[test]
	fn keypair_generation_works() {
		let keypair = Keypair::new();
		assert_eq!(keypair.secret.as_ref().len(), ML_KEM_768_SECRET_KEY_SIZE);
		assert_eq!(keypair.public.as_ref().len(), ML_KEM_768_PUBLIC_KEY_SIZE);
	}

	#[test]
	fn session_creation_works() {
		let keypair = Keypair::new();

		let alice = ClatterSession::new(b"prologue", true, &keypair).unwrap();
		let bob = ClatterSession::new(b"prologue", false, &keypair).unwrap();

		assert!(alice.is_initiator());
		assert!(!bob.is_initiator());
	}

	#[test]
	fn full_handshake_works() {
		let alice_keypair = Keypair::new();
		let bob_keypair = Keypair::new();

		let mut alice = ClatterSession::new(b"prologue", true, &alice_keypair).unwrap();
		let mut bob = ClatterSession::new(b"prologue", false, &bob_keypair).unwrap();

		// pqXX pattern: 4 messages
		// Message 1: -> e
		let mut msg1 = vec![0u8; 4096];
		let len1 = alice.write_message(&[], &mut msg1).unwrap();
		msg1.truncate(len1);

		let mut payload1 = vec![0u8; 4096];
		let _plen1 = bob.read_message(&msg1, &mut payload1).unwrap();

		// Message 2: <- ekem, e, es
		let mut msg2 = vec![0u8; 4096];
		let len2 = bob.write_message(&[], &mut msg2).unwrap();
		msg2.truncate(len2);

		let mut payload2 = vec![0u8; 4096];
		let _plen2 = alice.read_message(&msg2, &mut payload2).unwrap();

		// Message 3: -> skem, s, se (with payload)
		let mut msg3 = vec![0u8; 8192];
		let test_payload = b"hello from alice";
		let len3 = alice.write_message(test_payload, &mut msg3).unwrap();
		msg3.truncate(len3);

		let mut payload3 = vec![0u8; 4096];
		let plen3 = bob.read_message(&msg3, &mut payload3).unwrap();
		payload3.truncate(plen3);
		assert_eq!(&payload3, test_payload);

		// Message 4: <- sks (final KEM, empty payload)
		let mut msg4 = vec![0u8; 4096];
		let len4 = bob.write_message(&[], &mut msg4).unwrap();
		msg4.truncate(len4);

		let mut payload4 = vec![0u8; 4096];
		let plen4 = alice.read_message(&msg4, &mut payload4).unwrap();
		assert_eq!(plen4, 0); // Empty payload

		// Both should be finished
		assert!(alice.is_finished());
		assert!(bob.is_finished());

		// Convert to transport mode
		let mut alice_transport = alice.into_transport_mode().unwrap();
		let mut bob_transport = bob.into_transport_mode().unwrap();

		// Test transport
		let plaintext = b"post-quantum secure message";
		let mut ciphertext = vec![0u8; plaintext.len() + 16]; // +16 for auth tag
		let clen = alice_transport.write_message(plaintext, &mut ciphertext).unwrap();
		ciphertext.truncate(clen);

		let mut decrypted = vec![0u8; plaintext.len()];
		let dlen = bob_transport.read_message(&ciphertext, &mut decrypted).unwrap();
		decrypted.truncate(dlen);

		assert_eq!(&decrypted, plaintext);
	}
}
