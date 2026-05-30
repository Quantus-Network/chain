// This file is part of Substrate.
//
// Copyright (C) Parity Technologies (UK) Ltd.
// Copyright (C) Quantus Network Developers
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
//
// If you read this, you are very thorough, congratulations.

//! Signature-related code for litep2p network backend.

use litep2p::crypto::{
	PublicKey as Litep2pPublicKey,
	dilithium::Keypair as DilithiumKeypair,
};

/// Error during signing of a message.
#[derive(Debug, thiserror::Error)]
pub enum SigningError {
	/// Signing operation failed.
	#[error("Signing failed")]
	SigningFailed,
}

/// Error during decoding of key material.
#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
	/// Invalid key data.
	#[error("Invalid key data")]
	InvalidKey,
	/// Unknown key type.
	#[error("Unknown key type")]
	UnknownKeyType,
}

/// Public key (litep2p, supports Dilithium).
pub struct PublicKey(Litep2pPublicKey);

impl PublicKey {
	/// Protobuf-encode [`PublicKey`].
	pub fn encode_protobuf(&self) -> Vec<u8> {
		self.0.to_protobuf_encoding()
	}

	/// Get `PeerId` of the [`PublicKey`].
	pub fn to_peer_id(&self) -> sc_network_types::PeerId {
		let litep2p_peer_id: litep2p::PeerId = self.0.to_peer_id();
		litep2p_peer_id.into()
	}

	/// Try to decode public key from protobuf.
	pub fn try_decode_protobuf(bytes: &[u8]) -> Result<Self, DecodingError> {
		Litep2pPublicKey::from_protobuf_encoding(bytes)
			.map(PublicKey)
			.map_err(|_| DecodingError::InvalidKey)
	}

	/// Verify a signature.
	pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
		self.0.verify(msg, sig)
	}
}

impl From<Litep2pPublicKey> for PublicKey {
	fn from(key: Litep2pPublicKey) -> Self {
		PublicKey(key)
	}
}

/// Keypair (litep2p, supports Dilithium).
pub enum Keypair {
	/// Dilithium keypair (post-quantum).
	Dilithium(DilithiumKeypair),
}

impl Keypair {
	/// Generate ed25519 keypair (stub for API compatibility, but we use Dilithium).
	#[deprecated(note = "This network uses Dilithium. Use generate_dilithium() instead.")]
	pub fn generate_ed25519() -> Self {
		// For API compatibility, generate a Dilithium keypair instead
		Self::generate_dilithium()
	}

	/// Generate Dilithium keypair (post-quantum).
	pub fn generate_dilithium() -> Self {
		Keypair::Dilithium(DilithiumKeypair::generate())
	}

	/// Get [`Keypair`]'s public key.
	pub fn public(&self) -> PublicKey {
		match self {
			Keypair::Dilithium(kp) => PublicKey(Litep2pPublicKey::from(kp.public().clone())),
		}
	}

	/// Sign a message.
	pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, SigningError> {
		match self {
			Keypair::Dilithium(kp) => Ok(kp.sign(msg)),
		}
	}

	/// Get the secret key bytes.
	pub fn secret(&self) -> Option<Vec<u8>> {
		match self {
			Keypair::Dilithium(kp) => Some(kp.to_bytes()),
		}
	}

	/// Get the Dilithium secret bytes (for serialization).
	pub fn dilithium_to_bytes(&self) -> Vec<u8> {
		match self {
			Keypair::Dilithium(kp) => kp.to_bytes(),
		}
	}

	/// Create a Dilithium keypair from bytes.
	pub fn dilithium_from_bytes(bytes: &[u8]) -> Result<Self, DecodingError> {
		let mut bytes_mut = bytes.to_vec();
		DilithiumKeypair::try_from_bytes(&mut bytes_mut)
			.map(Keypair::Dilithium)
			.map_err(|_| DecodingError::InvalidKey)
	}

	/// Convert to litep2p keypair for the network backend.
	pub fn to_litep2p_keypair(&self) -> litep2p::crypto::Keypair {
		match self {
			Keypair::Dilithium(kp) => litep2p::crypto::Keypair::from(kp.clone()),
		}
	}
}

/// A result of signing a message with a network identity. Since `PeerId` is potentially a hash of a
/// `PublicKey`, you need to reveal the `PublicKey` next to the signature, so the verifier can check
/// if the signature was made by the entity that controls a given `PeerId`.
pub struct Signature {
	/// The public key derived from the network identity that signed the message.
	pub public_key: PublicKey,

	/// The actual signature made for the message signed.
	pub bytes: Vec<u8>,
}

impl Signature {
	/// Create new [`Signature`].
	pub fn new(public_key: PublicKey, bytes: Vec<u8>) -> Self {
		Self { public_key, bytes }
	}

	/// Create a signature for a message with a given network identity.
	pub fn sign_message(
		message: impl AsRef<[u8]>,
		keypair: &Keypair,
	) -> Result<Self, SigningError> {
		let public_key = keypair.public();
		let bytes = keypair.sign(message.as_ref())?;
		Ok(Signature { public_key, bytes })
	}
}
