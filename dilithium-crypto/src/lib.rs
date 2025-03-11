#![no_std]

pub mod crypto;
pub mod types;
pub mod traits;
pub mod pair;

pub use types::{ResonancePublic, ResonanceSignature, ResonancePair, ResonanceSignatureScheme, ResonanceSigner, WrappedPublicBytes, WrappedSignatureBytes};
pub use crypto::{PUB_KEY_BYTES, SECRET_KEY_BYTES, SIGNATURE_BYTES};
pub use pair::{crystal_alice, dilithium_bob, crystal_charlie};