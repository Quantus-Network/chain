use codec::{Decode, Encode, MaxEncodedLen};
use rusty_crystals_dilithium::ml_dsa_87::{PUBLICKEYBYTES, SECRETKEYBYTES};
use scale_info::TypeInfo;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sp_core::{
    crypto::{PublicBytes, SignatureBytes},
    ByteArray, RuntimeDebug,
};
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
pub struct DilithiumCryptoTag;

// TODO: Review if we even need Pair - we need some sort of pair trait in order to satisfy crypto bytes
// which is one of the wrapped public key types. But I am not sure we need that either.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DilithiumPair {
    pub secret: [u8; SECRETKEYBYTES],
    pub public: [u8; PUBLICKEYBYTES],
}

impl Default for DilithiumPair {
    fn default() -> Self {
        let seed = sp_std::vec![0u8; 32];
        DilithiumPair::from_seed(&seed).expect("Failed to generate keypair")
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct WrappedPublicBytes<const N: usize, SubTag>(pub PublicBytes<N, SubTag>);

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct WrappedSignatureBytes<const N: usize, SubTag>(pub SignatureBytes<N, SubTag>);

pub type DilithiumPublic = WrappedPublicBytes<{ crate::PUB_KEY_BYTES }, DilithiumCryptoTag>;
pub type DilithiumSignature = WrappedSignatureBytes<{ crate::SIGNATURE_BYTES }, DilithiumCryptoTag>;

// ResonanceSignatureScheme drop-in replacement for MultiSignature
// For now it's a single scheme but we leave this struct in place so we can easily plug in
// future signature schemes.
#[derive(Eq, PartialEq, Clone, Encode, Decode, MaxEncodedLen, RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DilithiumSignatureScheme {
    Dilithium(DilithiumSignatureWithPublic),
}

// Replacement for MultiSigner
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DilithiumSigner {
    Dilithium(DilithiumPublic),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to generate keypair")]
    KeyGenerationFailed,
    #[error("Invalid length")]
    InvalidLength,
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo, MaxEncodedLen, Ord, PartialOrd)]
pub struct DilithiumSignatureWithPublic {
    pub bytes: [u8; Self::TOTAL_LEN], // we have to store raw bytes for some traits
}

impl DilithiumSignatureWithPublic {
    const SIGNATURE_LEN: usize = <DilithiumSignature as ByteArray>::LEN;
    const PUBLIC_LEN: usize = <DilithiumPublic as ByteArray>::LEN;
    pub const TOTAL_LEN: usize = Self::SIGNATURE_LEN + Self::PUBLIC_LEN;

    pub fn new(signature: DilithiumSignature, public: DilithiumPublic) -> Self {
        let mut bytes = [0u8; Self::LEN];
        bytes[..Self::SIGNATURE_LEN].copy_from_slice(signature.as_ref());
        bytes[Self::SIGNATURE_LEN..].copy_from_slice(public.as_ref());
        Self { bytes }
    }

    pub fn signature(&self) -> DilithiumSignature {
        DilithiumSignature::from_slice(&self.bytes[..Self::SIGNATURE_LEN])
            .expect("Invalid signature")
    }

    pub fn public(&self) -> DilithiumPublic {
        DilithiumPublic::from_slice(&self.bytes[Self::SIGNATURE_LEN..]).expect("Invalid public key")
    }

    pub fn to_bytes(&self) -> [u8; Self::TOTAL_LEN] {
        self.bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != Self::TOTAL_LEN {
            return Err(Error::InvalidLength);
        }

        let signature = DilithiumSignature::from_slice(&bytes[..Self::SIGNATURE_LEN])
            .map_err(|_| Error::InvalidLength)?;
        let public = DilithiumPublic::from_slice(&bytes[Self::SIGNATURE_LEN..])
            .map_err(|_| Error::InvalidLength)?;

        Ok(Self::new(signature, public))
    }
}
