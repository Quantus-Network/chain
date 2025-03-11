
use codec::{Decode, Encode, MaxEncodedLen};
use rusty_crystals_dilithium::ml_dsa_87::{PUBLICKEYBYTES, SECRETKEYBYTES};
use scale_info::TypeInfo;
use sp_core::{crypto::{PublicBytes, SignatureBytes}, RuntimeDebug};
use sp_core::{ecdsa, ed25519, sr25519};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use thiserror::Error;

///
/// Resonance Crypto Types
/// 
/// Currently implementing the Dilithum cryprographic scheme for post quantum security
/// 
/// It is modeled after the Substrate MultiSignature and Signature types such as sr25519.
/// 
/// For traits implemented see traits.rs
///

#[derive(Clone, Eq, PartialEq, Debug, Hash, Encode, Decode, TypeInfo, Ord, PartialOrd)]
pub struct ResonanceCryptoTag;

// TODO: Review if we even need Pair - we need some sort of pair trait in order to satisfy crypto bytes
// which is one of the wrapped public key types. But I am not sure we need that either. 
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ResonancePair {
    pub secret: [u8; SECRETKEYBYTES],
    pub public: [u8; PUBLICKEYBYTES]

}

impl Default for ResonancePair {
    fn default() -> Self {
        let seed = sp_std::vec![0u8; 32];
        return ResonancePair::from_seed(&seed).expect("Failed to generate keypair");
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct WrappedPublicBytes<const N: usize, SubTag>(pub PublicBytes<N, SubTag>);

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct WrappedSignatureBytes<const N: usize, SubTag>(pub SignatureBytes<N, SubTag>);

pub type ResonancePublic = WrappedPublicBytes<{super::crypto::PUB_KEY_BYTES}, ResonanceCryptoTag>;
pub type ResonanceSignature = WrappedSignatureBytes<{super::crypto::SIGNATURE_BYTES}, ResonanceCryptoTag>;

// ResonanceSignatureScheme drop-in replacement for MultiSignature
#[derive(Eq, PartialEq, Clone, Encode, Decode, MaxEncodedLen, RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ResonanceSignatureScheme {
    Ed25519(ed25519::Signature),
    Sr25519(sr25519::Signature),
    Ecdsa(ecdsa::Signature),
    // Resonance(ResonanceSignature, [u8; super::crypto::PUB_KEY_BYTES]), // Signature and public key bytes
    Resonance(ResonanceSignatureWithPublic)
}

// Replacement for MultiSigner
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ResonanceSigner {
    Ed25519(ed25519::Public),
    Sr25519(sr25519::Public),
    Ecdsa(ecdsa::Public),
    Resonance(ResonancePublic),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to generate keypair")]
    KeyGenerationFailed,
}

#[derive(Eq, PartialEq, Clone, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct ResonanceSignatureWithPublic {
    pub signature: ResonanceSignature,
    pub public: ResonancePublic,
}
