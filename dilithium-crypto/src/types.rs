use codec::{Decode, Encode};
use scale_info::{prelude::string::String, TypeInfo};
use sp_core::crypto::{DeriveJunction, PublicBytes, SignatureBytes};
use sp_std::vec::Vec;
use sp_core::{ecdsa, ed25519, sr25519};

#[derive(Clone, Eq, PartialEq, Debug, Hash, Encode, Decode, TypeInfo)]
pub struct RezCryptoTag;

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RezPair {
    Generated,
    GeneratedWithPhrase,
    GeneratedFromPhrase { phrase: String, password: Option<String> },
    Standard { phrase: String, password: Option<String>, path: Vec<DeriveJunction> },
    Seed(Vec<u8>),
}

impl Default for RezPair {
    fn default() -> Self {
        RezPair::Generated
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo)]
pub struct WrappedPublicBytes<const N: usize, SubTag>(pub PublicBytes<N, SubTag>);

impl<const N: usize, SubTag> Default for WrappedPublicBytes<N, SubTag> {
    fn default() -> Self {
        WrappedPublicBytes(PublicBytes::default())
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo)]
pub struct WrappedSignatureBytes<const N: usize, SubTag>(pub SignatureBytes<N, SubTag>);

impl<const N: usize, SubTag> Default for WrappedSignatureBytes<N, SubTag> {
    fn default() -> Self {
        WrappedSignatureBytes(SignatureBytes::default())
    }
}

pub type RezPublic = WrappedPublicBytes<{super::crypto::PUB_KEY_BYTES}, RezCryptoTag>;
pub type RezSignature = WrappedSignatureBytes<{super::crypto::SIGNATURE_BYTES}, RezCryptoTag>;

#[derive(Clone, Eq, PartialEq, Encode, Decode, TypeInfo)]
pub enum RezMultiSignature {
    Ed25519(ed25519::Signature),
    Sr25519(sr25519::Signature),
    Ecdsa(ecdsa::Signature),
    Rez(Vec<u8>), // Combined signature and public key
}