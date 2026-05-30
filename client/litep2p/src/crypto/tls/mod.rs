// Copyright 2021 Parity Technologies (UK) Ltd.
// Copyright 2022 Protocol Labs.
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

//! TLS configuration based on libp2p TLS specs.
//!
//! See <https://github.com/libp2p/specs/blob/master/tls/tls.md>.
//!
//! This implementation uses post-quantum key exchange via ML-KEM (Kyber) hybrid mode
//! when available, providing quantum-resistant forward secrecy.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use crate::{crypto::dilithium::Keypair, PeerId};

use rustls::pki_types::PrivateKeyDer;
use std::sync::Arc;

pub mod certificate;
mod verifier;

const P2P_ALPN: [u8; 6] = *b"libp2p";

/// Create a TLS server configuration for litep2p with post-quantum key exchange.
pub fn make_server_config(
    keypair: &Keypair,
) -> Result<rustls::ServerConfig, certificate::GenError> {
    let (certificate, private_key) = certificate::generate(keypair)?;

    // Use post-quantum provider with ML-KEM hybrid key exchange
    let provider = rustls_post_quantum::provider();

    let mut crypto = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
        .expect("Protocol versions are valid; qed")
        .with_client_cert_verifier(Arc::new(verifier::Libp2pCertificateVerifier::new()))
        .with_single_cert(vec![certificate], PrivateKeyDer::Pkcs8(private_key))
        .expect("Server cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}

/// Create a TLS client configuration for libp2p with post-quantum key exchange.
pub fn make_client_config(
    keypair: &Keypair,
    remote_peer_id: Option<PeerId>,
) -> Result<rustls::ClientConfig, certificate::GenError> {
    let (certificate, private_key) = certificate::generate(keypair)?;

    // Use post-quantum provider with ML-KEM hybrid key exchange
    let provider = rustls_post_quantum::provider();

    let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
        .expect("Protocol versions are valid; qed")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(
            verifier::Libp2pCertificateVerifier::with_remote_peer_id(remote_peer_id),
        ))
        .with_client_auth_cert(vec![certificate], PrivateKeyDer::Pkcs8(private_key))
        .expect("Client cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}
