#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod pair;
pub mod traits;
pub mod types;

use qp_rusty_crystals_dilithium::ml_dsa_87;

pub const PUB_KEY_BYTES: usize = ml_dsa_87::PUBLICKEYBYTES;
pub const SECRET_KEY_BYTES: usize = ml_dsa_87::SECRETKEYBYTES;
pub const SIGNATURE_BYTES: usize = ml_dsa_87::SIGNBYTES;

pub use pair::{
	create_keypair, crystal_alice, crystal_charlie, crystal_dave, crystal_eve, dilithium_bob,
	generate,
};
pub use traits::verify;
pub use types::{
	DilithiumPair, DilithiumPublic, DilithiumSignature, DilithiumSignatureScheme,
	DilithiumSignatureWithPublic, DilithiumSigner, WrappedPublicBytes, WrappedSignatureBytes,
};
