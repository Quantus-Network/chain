#![no_std]

use super::types::{WrappedPublicBytes, WrappedSignatureBytes, RezPair, RezPublic, RezSignature};
use sp_core::{ByteArray, crypto::{Derive, Signature, Public, PublicBytes, SignatureBytes}};
use sp_runtime::{AccountId32, CryptoType, traits::{IdentifyAccount, Verify}};
use sp_std::vec::Vec;

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