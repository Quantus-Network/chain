#![no_std]

pub mod crypto;
pub mod pair;
pub mod traits;
pub mod types;

pub use crypto::{PUB_KEY_BYTES, SECRET_KEY_BYTES, SIGNATURE_BYTES};
pub use pair::{create_keypair, crystal_alice, crystal_charlie, dilithium_bob, generate};
pub use traits::verify;
pub use types::{
    ResonancePair, ResonancePublic, ResonanceSignature, ResonanceSignatureScheme,
    ResonanceSignatureWithPublic, ResonanceSigner, WrappedPublicBytes, WrappedSignatureBytes,
};
