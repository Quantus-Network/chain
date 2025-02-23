#![no_std]

use super::types::{WrappedPublicBytes, WrappedSignatureBytes, RezPair, RezPublic, RezSignature, RezMultiSignature};
use sp_core::{ByteArray, crypto::{Derive, Signature, Public, PublicBytes, SignatureBytes}};
use sp_runtime::{AccountId32, CryptoType, traits::{IdentifyAccount, Verify}};
use sp_std::vec::Vec;
use sp_core::{ecdsa, ed25519, sr25519};
use verify::verify;

impl<const N: usize, SubTag> Derive for WrappedPublicBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedPublicBytes<N, SubTag> {
    fn as_mut(&mut self) -> &mut [u8] { self.0.as_mut() }
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedPublicBytes<N, SubTag> {
    fn as_ref(&self) -> &[u8] { self.0.as_slice() }
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedPublicBytes<N, SubTag> {
    type Error = ();
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        PublicBytes::from_slice(data).map(|bytes| WrappedPublicBytes(bytes)).map_err(|_| ())
    }
}
impl<const N: usize, SubTag> ByteArray for WrappedPublicBytes<N, SubTag> {
    fn as_slice(&self) -> &[u8] { self.0.as_slice() }
    const LEN: usize = N;
    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        PublicBytes::from_slice(data).map(|bytes| WrappedPublicBytes(bytes)).map_err(|_| ())
    }
    fn to_raw_vec(&self) -> Vec<u8> { self.0.as_slice().to_vec() }
}
impl<const N: usize, SubTag> CryptoType for WrappedPublicBytes<N, SubTag> {
    type Pair = RezPair;
}
impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> {}

impl<const N: usize, SubTag> Derive for WrappedSignatureBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedSignatureBytes<N, SubTag> {
    fn as_mut(&mut self) -> &mut [u8] { self.0.as_mut() }
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedSignatureBytes<N, SubTag> {
    fn as_ref(&self) -> &[u8] { self.0.as_slice() }
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedSignatureBytes<N, SubTag> {
    type Error = ();
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        SignatureBytes::from_slice(data).map(|bytes| WrappedSignatureBytes(bytes)).map_err(|_| ())
    }
}
impl<const N: usize, SubTag> ByteArray for WrappedSignatureBytes<N, SubTag> {
    fn as_slice(&self) -> &[u8] { self.0.as_slice() }
    const LEN: usize = N;
    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        SignatureBytes::from_slice(data).map(|bytes| WrappedSignatureBytes(bytes)).map_err(|_| ())
    }
    fn to_raw_vec(&self) -> Vec<u8> { self.0.as_slice().to_vec() }
}
impl<const N: usize, SubTag> CryptoType for WrappedSignatureBytes<N, SubTag> {
    type Pair = RezPair;
}
impl<const N: usize, SubTag: Clone + Eq> Signature for WrappedSignatureBytes<N, SubTag> {}

impl<const N: usize, SubTag: Clone + Eq> IdentifyAccount for WrappedPublicBytes<N, SubTag> {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        AccountId32::new(sp_io::hashing::blake2_256(self.0.as_slice()))
    }
}

impl Verify for RezSignature {
    type Signer = RezPublic;
    fn verify<L: sp_runtime::traits::Lazy<[u8]>>(
        &self,
        mut msg: L,
        signer: &<Self::Signer as IdentifyAccount>::AccountId,
    ) -> bool {
        true // Placeholder
    }
}

impl CryptoType for RezPair {
    type Pair = Self;
}

// Conversions for RezMultiSignature
impl From<ed25519::Signature> for RezMultiSignature {
    fn from(x: ed25519::Signature) -> Self {
        Self::Ed25519(x)
    }
}

impl TryFrom<RezMultiSignature> for ed25519::Signature {
    type Error = ();
    fn try_from(m: RezMultiSignature) -> Result<Self, Self::Error> {
        if let RezMultiSignature::Ed25519(x) = m { Ok(x) } else { Err(()) }
    }
}

impl From<sr25519::Signature> for RezMultiSignature {
    fn from(x: sr25519::Signature) -> Self {
        Self::Sr25519(x)
    }
}

impl TryFrom<RezMultiSignature> for sr25519::Signature {
    type Error = ();
    fn try_from(m: RezMultiSignature) -> Result<Self, Self::Error> {
        if let RezMultiSignature::Sr25519(x) = m { Ok(x) } else { Err(()) }
    }
}

impl From<ecdsa::Signature> for RezMultiSignature {
    fn from(x: ecdsa::Signature) -> Self {
        Self::Ecdsa(x)
    }
}

impl TryFrom<RezMultiSignature> for ecdsa::Signature {
    type Error = ();
    fn try_from(m: RezMultiSignature) -> Result<Self, Self::Error> {
        if let RezMultiSignature::Ecdsa(x) = m { Ok(x) } else { Err(()) }
    }
}

// impl From<RezSignature> for RezMultiSignature {
//     fn from(sig: RezSignature) -> Self {
//         Self::Rez(sig)
//     }
// }
impl From<(RezSignature, Vec<u8>)> for RezMultiSignature {
    fn from((sig, pk_bytes): (RezSignature, Vec<u8>)) -> Self {
        let mut combined = Vec::new();
        combined.extend_from_slice(&pk_bytes); // PUB_KEY_BYTES
        combined.extend_from_slice(sig.as_ref()); // SIGNATURE_BYTES
        Self::Rez(combined)
    }
}

// Define RezMultiSigner (simplified to just use AccountId32)
#[derive(Clone, Eq, PartialEq)]
pub struct RezMultiSigner(AccountId32);

impl IdentifyAccount for RezMultiSigner {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        self.0
    }
}

impl Verify for RezMultiSignature {
    type Signer = RezMultiSigner;

    fn verify<L: sp_runtime::traits::Lazy<[u8]>>(
        &self,
        mut msg: L,
        signer: &<Self::Signer as IdentifyAccount>::AccountId,
    ) -> bool {
        match self {
            Self::Ed25519(sig) => {
                let pk = ed25519::Public::from_slice(signer.as_ref()).unwrap_or_default();
                sig.verify(msg, &pk)
            },
            Self::Sr25519(sig) => {
                let pk = sr25519::Public::from_slice(signer.as_ref()).unwrap_or_default();
                sig.verify(msg, &pk)
            },
            Self::Ecdsa(sig) => {
                let m = sp_io::hashing::blake2_256(msg.get());
                sp_io::crypto::secp256k1_ecdsa_recover_compressed(sig.as_ref(), &m)
                    .map_or(false, |pubkey| sp_io::hashing::blake2_256(&pubkey) == <AccountId32 as AsRef<[u8]>>::as_ref(signer))
            },
            Self::Rez(data) => {
                if data.len() != super::crypto::PUB_KEY_BYTES + super::crypto::SIGNATURE_BYTES {
                    return false;
                }
                let pk_bytes = &data[..super::crypto::PUB_KEY_BYTES];
                let sig_bytes = &data[super::crypto::PUB_KEY_BYTES..];
                let pk_hash = sp_io::hashing::blake2_256(pk_bytes);
                if &pk_hash != <AccountId32 as AsRef<[u8]>>::as_ref(signer) {
                    return false;
                }
                verify(pk_bytes, msg.get(), sig_bytes)
            },
        }
    }
}
