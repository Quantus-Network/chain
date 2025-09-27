// This file is part of Substrate.
//
// Copyright (C) Parity Technologies (UK) Ltd.
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

//! Signature-related code

pub use libp2p_identity::SigningError;

/// Public key.
pub struct PublicKey(libp2p_identity::PublicKey);

impl PublicKey {
	/// Create new [`PublicKey`].
	pub fn new(public_key: libp2p_identity::PublicKey) -> Self {
		Self(public_key)
	}

	/// Protobuf-encode [`PublicKey`].
	pub fn encode_protobuf(&self) -> Vec<u8> {
		self.0.encode_protobuf()
	}

	/// Get `PeerId` of the [`PublicKey`].
	pub fn to_peer_id(&self) -> sc_network_types::PeerId {
		self.0.to_peer_id().into()
	}
}

/// Keypair.
pub struct Keypair(libp2p_identity::Keypair);

impl Keypair {
	/// Create new [`Keypair`].
	pub fn new(keypair: libp2p_identity::Keypair) -> Self {
		Self(keypair)
	}

	/// Generate Dilithium (Post-Quantum) keypair.
	pub fn generate_dilithium() -> Self {
		Keypair(libp2p_identity::Keypair::generate_dilithium())
	}

	/// Get [`Keypair`]'s public key.
	pub fn public(&self) -> PublicKey {
		PublicKey::new(self.0.public())
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
		let public_key = keypair.0.public();
		let bytes = keypair.0.sign(message.as_ref())?;

		Ok(Signature { public_key: PublicKey::new(public_key), bytes })
	}
}
