#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod pair;
mod scheme_macro;
pub mod signing_context;
pub mod traits;
pub mod types;

use qp_rusty_crystals_dilithium::ml_dsa_87;

pub const PUB_KEY_BYTES: usize = ml_dsa_87::PUBLICKEYBYTES;
pub const SECRET_KEY_BYTES: usize = ml_dsa_87::SECRETKEYBYTES;
pub const SIGNATURE_BYTES: usize = ml_dsa_87::SIGNBYTES;

pub use pair::{create_keypair, crystal_alice, crystal_charlie, dilithium_bob, generate};
pub use types::{
	verify_ml_dsa_65, verify_ml_dsa_65_with_context, verify_ml_dsa_87,
	verify_ml_dsa_87_with_context, Dilithium65CryptoTag, Dilithium65Pair, Dilithium65Public,
	Dilithium65Signature, Dilithium65SignatureWithPublic, Dilithium87CryptoTag, Dilithium87Pair,
	Dilithium87Public, Dilithium87Signature, Dilithium87SignatureWithPublic,
	DilithiumSignatureScheme, DilithiumSigner, WrappedPublicBytes, WrappedSignatureBytes,
};
