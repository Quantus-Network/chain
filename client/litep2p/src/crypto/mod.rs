// Copyright 2023 Protocol Labs.
// Copyright 2023 litep2p developers
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

//! Crypto-related code.
//!
//! This module provides post-quantum cryptography using Dilithium ML-DSA-87.

use crate::{error::ParseError, peer_id::*};

pub mod dilithium;

pub(crate) mod noise;
#[cfg(feature = "quic")]
pub(crate) mod tls;
pub(crate) mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

// Re-export Keypair for convenience
pub use dilithium::Keypair;

/// The public key of a node's identity keypair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey(pub(crate) dilithium::PublicKey);

impl PublicKey {
    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        use prost::Message;

        let public_key = keys_proto::PublicKey::from(self);

        let mut buf = Vec::with_capacity(public_key.encoded_len());
        public_key.encode(&mut buf).expect("Vec<u8> provides capacity as needed");
        buf
    }

    /// Convert the `PublicKey` into the corresponding `PeerId`.
    pub fn to_peer_id(&self) -> PeerId {
        self.into()
    }

    /// Verify a signature for a message using this public key.
    #[must_use]
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        self.0.verify(msg, sig)
    }

    /// Convert the public key to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes()
    }

    /// Get the public key as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl From<&PublicKey> for keys_proto::PublicKey {
    fn from(key: &PublicKey) -> Self {
        keys_proto::PublicKey {
            r#type: keys_proto::KeyType::Dilithium as i32,
            data: key.0.to_bytes(),
        }
    }
}

impl TryFrom<keys_proto::PublicKey> for PublicKey {
    type Error = ParseError;

    fn try_from(pubkey: keys_proto::PublicKey) -> Result<Self, Self::Error> {
        let key_type = keys_proto::KeyType::try_from(pubkey.r#type)
            .map_err(|_| ParseError::UnknownKeyType(pubkey.r#type))?;

        if key_type != keys_proto::KeyType::Dilithium {
            return Err(ParseError::UnknownKeyType(key_type as i32));
        }

        dilithium::PublicKey::try_from_bytes(&pubkey.data).map(PublicKey)
    }
}

impl From<dilithium::PublicKey> for PublicKey {
    fn from(public_key: dilithium::PublicKey) -> Self {
        PublicKey(public_key)
    }
}

/// The public key of a remote node's identity keypair.
///
/// This is used when verifying signatures from remote peers.
pub type RemotePublicKey = PublicKey;

impl RemotePublicKey {
    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<RemotePublicKey, ParseError> {
        use prost::Message;

        let pubkey = keys_proto::PublicKey::decode(bytes)?;

        pubkey.try_into()
    }
}
