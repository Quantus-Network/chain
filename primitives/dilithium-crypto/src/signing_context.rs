//! FIPS 204 ML-DSA context strings for domain separation.
//!
//! `sign` / `verify` hash the context into the signature, so a signature
//! produced under one of these strings will not verify under another — even
//! over the same message and key. Wallets and other signers must use the same
//! context the verifier expects.
//!
//! Each string is at most 255 bytes, as required by FIPS 204.
//!
//! litep2p node-identity signatures stay on the empty context so mixed-version
//! Noise handshakes keep working. Node keys are not account keys; an empty-
//! context p2p signature still will not verify as an extrinsic.

/// On-chain extrinsic signatures (`Pair::sign`, `Verify::verify`).
pub const EXTRINSIC: &[u8] = b"QUANTUS_EXTRINSIC";

const _: () = assert!(!EXTRINSIC.is_empty() && EXTRINSIC.len() <= 255);
