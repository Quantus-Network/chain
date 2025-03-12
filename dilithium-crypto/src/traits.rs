use super::types::{
    ResonancePublic, Error, WrappedPublicBytes, WrappedSignatureBytes,
    ResonancePair, ResonanceSignatureScheme, ResonanceSigner
};

use sp_core::{ByteArray, crypto::{Derive, Signature, Public, PublicBytes, SignatureBytes}};
use sp_runtime::{AccountId32, CryptoType, traits::{IdentifyAccount, Verify}};
use sp_std::vec::Vec;
use sp_core::{ecdsa, ed25519, sr25519};
use sp_runtime::traits::Hash;
use verify::verify;
use poseidon_resonance::PoseidonHasher;
use crate::{ResonanceSignature, ResonanceSignatureWithPublic};
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
    type Pair = ResonancePair;
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

impl IdentifyAccount for ResonancePublic {
    type AccountId = AccountId32;
    fn into_account(self) -> Self::AccountId {
        AccountId32::new(PoseidonHasher::hash(self.0.as_slice()).0)
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
    type Pair = ResonancePair;
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

impl CryptoType for ResonancePair {
    type Pair = Self;
}

// Conversions for ResonanceSignatureScheme
impl From<ed25519::Signature> for ResonanceSignatureScheme {
    fn from(x: ed25519::Signature) -> Self {
        Self::Ed25519(x)
    }
}

impl TryFrom<ResonanceSignatureScheme> for ed25519::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignatureScheme) -> Result<Self, Self::Error> {
        if let ResonanceSignatureScheme::Ed25519(x) = m { Ok(x) } else { Err(()) }
    }
}

impl From<sr25519::Signature> for ResonanceSignatureScheme {
    fn from(x: sr25519::Signature) -> Self {
        Self::Sr25519(x)
    }
}

impl TryFrom<ResonanceSignatureScheme> for sr25519::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignatureScheme) -> Result<Self, Self::Error> {
        if let ResonanceSignatureScheme::Sr25519(x) = m { Ok(x) } else { Err(()) }
    }
}

impl From<ecdsa::Signature> for ResonanceSignatureScheme {
    fn from(x: ecdsa::Signature) -> Self {
        Self::Ecdsa(x)
    }
}

impl TryFrom<ResonanceSignatureScheme> for ecdsa::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignatureScheme) -> Result<Self, Self::Error> {
        if let ResonanceSignatureScheme::Ecdsa(x) = m { Ok(x) } else { Err(()) }
    }
}

impl Verify for ResonanceSignatureScheme {
    type Signer = ResonanceSigner;

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
            Self::Resonance(sig_public) => {
                let pk_hash = PoseidonHasher::hash(sig_public.public.as_ref());
                if &pk_hash.0 != <AccountId32 as AsRef<[u8]>>::as_ref(signer) {
                    return false;
                }
                let result = verify(sig_public.public.as_ref(), msg.get(), sig_public.signature.as_ref());
                result
            },
        }
    }
}

//
// ResonanceSigner
//
impl From<sr25519::Public> for ResonanceSigner {
    fn from(x: sr25519::Public) -> Self {
        Self::Sr25519(x)
    }
}
impl From<ResonancePublic> for ResonanceSigner {
    fn from(x: ResonancePublic) -> Self {
        Self::Resonance(x)
    }
}

impl IdentifyAccount for ResonanceSigner {
    type AccountId = AccountId32;

    fn into_account(self) -> AccountId32 {
        match self {
            Self::Ed25519(who) => <[u8; 32]>::from(who).into(),
            Self::Sr25519(who) => <[u8; 32]>::from(who).into(),
            Self::Ecdsa(who) => sp_io::hashing::blake2_256(who.as_ref()).into(),
            Self::Resonance(who) => PoseidonHasher::hash(who.as_ref()).0.into(),
        }
    }
}

impl From<ResonancePublic> for AccountId32 {
    fn from(public: ResonancePublic) -> Self {
        return public.into_account();
    }
}

impl ResonancePair {
    pub fn from_seed(seed: &[u8]) -> Result<Self, Error> {
        let keypair = hdwallet::generate(Some(&seed)).map_err(|_| Error::KeyGenerationFailed)?;
        Ok(ResonancePair {
            secret: keypair.secret.to_bytes(),
            public: keypair.public.to_bytes()
        })
    }
    pub fn public(&self) -> ResonancePublic {
        ResonancePublic::from_slice(&self.public).unwrap()
    }
}

impl sp_std::fmt::Debug for ResonanceSignatureWithPublic {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(
            f,
            "ResonanceSignatureWithPublic {{ signature: {:?}, public: {:?} }}", self.signature, self.public
        )
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
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
        if let ResonanceSignatureScheme::Resonance(sig_public) = m {
            Ok(sig_public)
        } else {
             Err(())
        }
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
            ResonancePublic::from_slice(pub_bytes)?
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
