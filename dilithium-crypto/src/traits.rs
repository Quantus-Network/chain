use super::types::{
    Error, ResonancePair, ResonancePublic, ResonanceSignatureScheme, ResonanceSigner,
    WrappedPublicBytes, WrappedSignatureBytes,
};

use crate::{ResonanceSignature, ResonanceSignatureWithPublic};
use alloc::vec::Vec;
use poseidon_resonance::PoseidonHasher;
use sp_core::H256;
use sp_core::{
    crypto::{Derive, Public, PublicBytes, Signature, SignatureBytes},
    ByteArray, Hasher,
};
use sp_runtime::{
    traits::{IdentifyAccount, Verify},
    AccountId32, CryptoType,
};
use verify::verify;

impl<const N: usize, SubTag> Derive for WrappedPublicBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedPublicBytes<N, SubTag> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedPublicBytes<N, SubTag> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedPublicBytes<N, SubTag> {
    type Error = ();
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        PublicBytes::from_slice(data)
            .map(|bytes| WrappedPublicBytes(bytes))
            .map_err(|_| ())
    }
}
impl<const N: usize, SubTag> ByteArray for WrappedPublicBytes<N, SubTag> {
    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
    const LEN: usize = N;
    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        PublicBytes::from_slice(data)
            .map(|bytes| WrappedPublicBytes(bytes))
            .map_err(|_| ())
    }
    fn to_raw_vec(&self) -> Vec<u8> {
        self.0.as_slice().to_vec()
    }
}
impl<const N: usize, SubTag> CryptoType for WrappedPublicBytes<N, SubTag> {
    type Pair = ResonancePair;
}
impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> {}

impl<const N: usize, SubTag> Default for WrappedPublicBytes<N, SubTag> {
    fn default() -> Self {
        WrappedPublicBytes(PublicBytes::default())
    }
}
impl<const N: usize, SubTag> core::fmt::Debug for WrappedPublicBytes<N, SubTag> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{}",
            sp_core::hexdisplay::HexDisplay::from(&self.0.as_ref())
        )
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl IdentifyAccount for ResonancePublic {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        AccountId32::new(PoseidonHasher::hash(self.0.as_slice()).0)
    }
}

pub struct WormholeAddress(pub H256);

// AccountID32 for a wormhole address is the same as the address itself
impl IdentifyAccount for WormholeAddress {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        AccountId32::new(self.0.as_bytes().try_into().unwrap())
    }
}

//
// WrappedSignatureBytes
//
impl<const N: usize, SubTag> Derive for WrappedSignatureBytes<N, SubTag> {}
impl<const N: usize, SubTag> AsMut<[u8]> for WrappedSignatureBytes<N, SubTag> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}
impl<const N: usize, SubTag> AsRef<[u8]> for WrappedSignatureBytes<N, SubTag> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}
impl<const N: usize, SubTag> TryFrom<&[u8]> for WrappedSignatureBytes<N, SubTag> {
    type Error = ();
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        SignatureBytes::from_slice(data)
            .map(|bytes| WrappedSignatureBytes(bytes))
            .map_err(|_| ())
    }
}
impl<const N: usize, SubTag> ByteArray for WrappedSignatureBytes<N, SubTag> {
    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
    const LEN: usize = N;
    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        SignatureBytes::from_slice(data)
            .map(|bytes| WrappedSignatureBytes(bytes))
            .map_err(|_| ())
    }
    fn to_raw_vec(&self) -> Vec<u8> {
        self.0.as_slice().to_vec()
    }
}
impl<const N: usize, SubTag> CryptoType for WrappedSignatureBytes<N, SubTag> {
    type Pair = ResonancePair;
}
impl<const N: usize, SubTag: Clone + Eq> Signature for WrappedSignatureBytes<N, SubTag> {}

impl<const N: usize, SubTag> Default for WrappedSignatureBytes<N, SubTag> {
    fn default() -> Self {
        WrappedSignatureBytes(SignatureBytes::default())
    }
}

impl<const N: usize, SubTag> core::fmt::Debug for WrappedSignatureBytes<N, SubTag> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{}",
            sp_core::hexdisplay::HexDisplay::from(&self.0.as_ref())
        )
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl CryptoType for ResonancePair {
    type Pair = Self;
}

impl Verify for ResonanceSignatureScheme {
    type Signer = ResonanceSigner;

    fn verify<L: sp_runtime::traits::Lazy<[u8]>>(
        &self,
        mut msg: L,
        signer: &<Self::Signer as IdentifyAccount>::AccountId,
    ) -> bool {
        let Self::Resonance(sig_public) = self;
        let account = sig_public.public().clone().into_account();
        if account != *signer {
            return false;
        }
        let result = verify(
            sig_public.public().as_ref(),
            msg.get(),
            sig_public.signature().as_ref(),
        );
        result
    }
}

//
// ResonanceSigner
//
impl From<ResonancePublic> for ResonanceSigner {
    fn from(x: ResonancePublic) -> Self {
        Self::Resonance(x)
    }
}

impl IdentifyAccount for ResonanceSigner {
    type AccountId = AccountId32;

    fn into_account(self) -> AccountId32 {
        let Self::Resonance(who) = self;
        PoseidonHasher::hash(who.as_ref()).0.into()
    }
}

impl From<ResonancePublic> for AccountId32 {
    fn from(public: ResonancePublic) -> Self {
        public.into_account()
    }
}

impl ResonancePair {
    pub fn from_seed(seed: &[u8]) -> Result<Self, Error> {
        let keypair = hdwallet::generate(Some(seed)).map_err(|_| Error::KeyGenerationFailed)?;
        Ok(ResonancePair {
            secret: keypair.secret.to_bytes(),
            public: keypair.public.to_bytes(),
        })
    }
    pub fn public(&self) -> ResonancePublic {
        ResonancePublic::from_slice(&self.public).unwrap()
    }
}

impl core::fmt::Debug for ResonanceSignatureWithPublic {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "ResonanceSignatureWithPublic {{ signature: {:?}, public: {:?} }}",
            self.signature(),
            self.public()
        )
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "ResonanceSignatureWithPublic")
    }
}

impl From<ResonanceSignatureWithPublic> for ResonanceSignatureScheme {
    fn from(x: ResonanceSignatureWithPublic) -> Self {
        Self::Resonance(x)
    }
}

impl TryFrom<ResonanceSignatureScheme> for ResonanceSignatureWithPublic {
    type Error = (); // TODO: fix errors
    fn try_from(m: ResonanceSignatureScheme) -> Result<Self, Self::Error> {
        let ResonanceSignatureScheme::Resonance(sig_public) = m;
        Ok(sig_public)
    }
}

impl AsMut<[u8]> for ResonanceSignatureWithPublic {
    fn as_mut(&mut self) -> &mut [u8] {
        self.bytes.as_mut()
    }
}
impl TryFrom<&[u8]> for ResonanceSignatureWithPublic {
    type Error = ();
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let (sig_bytes, pub_bytes) = data.split_at(<ResonanceSignature as ByteArray>::LEN);
        Ok(Self::new(
            ResonanceSignature::from_slice(sig_bytes)?,
            ResonancePublic::from_slice(pub_bytes)?,
        ))
    }
}

impl ByteArray for ResonanceSignatureWithPublic {
    const LEN: usize = Self::TOTAL_LEN;

    fn to_raw_vec(&self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }

    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        if data.len() != Self::LEN {
            return Err(());
        }
        let bytes = <[u8; Self::LEN]>::try_from(data).map_err(|_| ())?;
        Self::from_bytes(&bytes).map_err(|_| ())
    }

    fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}
impl AsRef<[u8; Self::LEN]> for ResonanceSignatureWithPublic {
    fn as_ref(&self) -> &[u8; Self::LEN] {
        &self.bytes
    }
}

impl AsRef<[u8]> for ResonanceSignatureWithPublic {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
impl Signature for ResonanceSignatureWithPublic {}

impl CryptoType for ResonanceSignatureWithPublic {
    type Pair = ResonancePair;
}
