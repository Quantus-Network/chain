
use codec::{Decode, Encode, MaxEncodedLen};
use rusty_crystals_dilithium::ml_dsa_87::{PUBLICKEYBYTES, SECRETKEYBYTES};
use scale_info::TypeInfo;
use sp_core::{crypto::{PublicBytes, SignatureBytes}, ByteArray, RuntimeDebug};
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
    #[error("Invalid length")]
    InvalidLength,
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct ResonanceSignatureWithPublic {
    pub signature: ResonanceSignature, // TODO remove these, we don't need them
    pub public: ResonancePublic,
    pub bytes: [u8; Self::TOTAL_LEN], // we have to store raw bytes for some traits
}

impl ResonanceSignatureWithPublic {
    const SIGNATURE_LEN: usize = <ResonanceSignature as ByteArray>::LEN;
    const PUBLIC_LEN: usize = <ResonancePublic as ByteArray>::LEN;
    pub const TOTAL_LEN: usize = Self::SIGNATURE_LEN + Self::PUBLIC_LEN;

    pub fn new(signature: ResonanceSignature, public: ResonancePublic) -> Self {
        let mut bytes = [0u8; Self::LEN];
        bytes[..Self::SIGNATURE_LEN].copy_from_slice(signature.as_ref());
        bytes[Self::SIGNATURE_LEN..].copy_from_slice(public.as_ref());
        Self {
            signature,
            public,
            bytes,
        }
    }

    pub fn signature(&self) -> ResonanceSignature {
        ResonanceSignature::from_slice(&self.bytes[..Self::SIGNATURE_LEN])
            .expect("Invalid signature")
    }

    pub fn public(&self) -> ResonancePublic {
        ResonancePublic::from_slice(&self.bytes[Self::SIGNATURE_LEN..])
            .expect("Invalid public key")
    }

    pub fn to_bytes(&self) -> [u8; Self::TOTAL_LEN] {
        self.bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != Self::TOTAL_LEN {
            return Err(Error::InvalidLength);
        }
        
        let signature = ResonanceSignature::from_slice(&bytes[..Self::SIGNATURE_LEN])
            .map_err(|_| Error::InvalidLength)?;
        let public = ResonancePublic::from_slice(&bytes[Self::SIGNATURE_LEN..])
            .map_err(|_| Error::InvalidLength)?;
        
        Ok(Self::new(signature, public))
    }
}

