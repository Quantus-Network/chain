
use codec::{Decode, Encode, MaxEncodedLen};
use scale_info::{prelude::string::String, TypeInfo};
use sp_core::{crypto::{DeriveJunction, PublicBytes, SignatureBytes}, RuntimeDebug};
use sp_std::vec::Vec;
use sp_core::{ecdsa, ed25519, sr25519};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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
pub enum ResonancePair {
    Generated,
    GeneratedWithPhrase,
    GeneratedFromPhrase { phrase: String, password: Option<String> },
    Standard { phrase: String, password: Option<String>, path: Vec<DeriveJunction> },
    Seed(Vec<u8>),
}

impl Default for ResonancePair {
    fn default() -> Self {
        ResonancePair::Generated
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
    Resonance(ResonanceSignature, [u8; super::crypto::PUB_KEY_BYTES]), // Signature and public key bytes
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
