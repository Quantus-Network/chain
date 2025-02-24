#![no_std]

use crate::SIGNATURE_BYTES;

use super::types::{WrappedPublicBytes, WrappedSignatureBytes, RezPair, RezPublic, RezSignature, RezMultiSignature, RezMultiSigner};
use scale_info::TypeInfo;
use sp_core::{ByteArray, crypto::{Derive, Signature, Public, PublicBytes, SignatureBytes}};
use sp_runtime::{AccountId32, CryptoType, traits::{IdentifyAccount, Verify}};
use sp_std::vec::Vec;
use sp_core::{ecdsa, ed25519, sr25519};
use verify::verify;
use codec::{Encode, Decode};

// 
// WrappedPublicBytes
// 

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

impl<const N: usize, SubTag> Default for WrappedPublicBytes<N, SubTag> {
    fn default() -> Self {
        WrappedPublicBytes(PublicBytes::default())
    }
}
impl<const N: usize, SubTag> sp_std::fmt::Debug for WrappedPublicBytes<N, SubTag> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "{}", sp_core::hexdisplay::HexDisplay::from(&self.0.as_ref()))
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        Ok(())
    }
}

impl<const N: usize, SubTag: Clone + Eq> IdentifyAccount for WrappedPublicBytes<N, SubTag> {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        AccountId32::new(sp_io::hashing::blake2_256(self.0.as_slice()))
    }
}

// 
// WrappedSignatureBytes
// 
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

impl<const N: usize, SubTag> Default for WrappedSignatureBytes<N, SubTag> {
    fn default() -> Self {
        WrappedSignatureBytes(SignatureBytes::default())
    }
}

impl<const N: usize, SubTag> sp_std::fmt::Debug for WrappedSignatureBytes<N, SubTag> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "{}", sp_core::hexdisplay::HexDisplay::from(&self.0.as_ref()))
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        Ok(())
    }
}

// impl Verify for RezSignature {
//     type Signer = RezPublic;
//     fn verify<L: sp_runtime::traits::Lazy<[u8]>>(
//         &self,
//         mut msg: L,
//         signer: &<Self::Signer as IdentifyAccount>::AccountId,
//     ) -> bool {
//         RezMultiSignature::from(self).verify(msg, signer);
//     }
// }

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

impl From<(RezSignature, [u8; 2592])> for RezMultiSignature {
    fn from((sig, pk): (RezSignature, [u8; 2592])) -> Self {
        Self::Rez(sig, pk)
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
            Self::Rez(sig, pk_bytes) => {
                let pk_hash = sp_io::hashing::blake2_256(pk_bytes);
                if &pk_hash != <AccountId32 as AsRef<[u8]>>::as_ref(signer) {
                    return false;
                }
                verify(pk_bytes, msg.get(), sig.as_ref())
            },
        }
    }
}

//
// RezMultiSigner
//
impl From<sr25519::Public> for RezMultiSigner {
    fn from(x: sr25519::Public) -> Self {
        Self::Sr25519(x)
    }
}

impl IdentifyAccount for RezMultiSigner {
    type AccountId = AccountId32;

    fn into_account(self) -> AccountId32 {
        match self {
            Self::Ed25519(who) => <[u8; 32]>::from(who).into(),
            Self::Sr25519(who) => <[u8; 32]>::from(who).into(),
            Self::Ecdsa(who) => sp_io::hashing::blake2_256(who.as_ref()).into(),
            Self::Rez(who) => sp_io::hashing::blake2_256(who.as_ref()).into(),
        }
    }
}
impl RezSignature {
    pub fn from_slice(slice: &[u8]) -> Result<Self, &'static str> {
        if slice.len() == SIGNATURE_BYTES {
            Ok(Self(slice.try_into().unwrap()))
        } else {
            Err("Signature length mismatch")
        }
    }
}