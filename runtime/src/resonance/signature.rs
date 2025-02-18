use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::__private::sp_io;
use primitive_types::H256;
use scale_info::TypeInfo;
use serde::{Deserialize, Serialize};
use sp_core::{crypto, ecdsa, ed25519, RuntimeDebug};
use sp_runtime::traits;
use sp_runtime::traits::{Lazy, Verify};
use crate::resonance::account::FromEntropy;
use crate::resonance::sr25519;
use crate::ResonanceAccountId;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Eq, PartialEq, Clone, Encode, Decode, MaxEncodedLen, RuntimeDebug, TypeInfo)]
pub enum ResonanceSignature {
    /// An Ed25519 signature.
    Ed25519(ed25519::Signature),
    /// An Sr25519 signature.
    Sr25519(sr25519::Signature),
    /// An ECDSA/SECP256k1 signature.
    Ecdsa(ecdsa::Signature),
}

impl From<ed25519::Signature> for ResonanceSignature {
    fn from(x: ed25519::Signature) -> Self {
        Self::Ed25519(x)
    }
}

impl TryFrom<ResonanceSignature> for ed25519::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignature) -> Result<Self, Self::Error> {
        if let ResonanceSignature::Ed25519(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

impl From<sr25519::Signature> for ResonanceSignature {
    fn from(x: sr25519::Signature) -> Self {
        Self::Sr25519(x)
    }
}

impl TryFrom<ResonanceSignature> for sr25519::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignature) -> Result<Self, Self::Error> {
        if let ResonanceSignature::Sr25519(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

impl From<ecdsa::Signature> for ResonanceSignature {
    fn from(x: ecdsa::Signature) -> Self {
        Self::Ecdsa(x)
    }
}

impl TryFrom<ResonanceSignature> for ecdsa::Signature {
    type Error = ();
    fn try_from(m: ResonanceSignature) -> Result<Self, Self::Error> {
        if let ResonanceSignature::Ecdsa(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

/// Public key for any known crypto algorithm.
#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ResonanceSigner {
    /// An Ed25519 identity.
    Ed25519(ed25519::Public),
    /// An Sr25519 identity.
    Sr25519(sr25519::Public),
    /// An SECP256k1/ECDSA identity (actually, the Blake2 hash of the compressed pub key).
    Ecdsa(ecdsa::Public),
}

impl FromEntropy for ResonanceSigner {
    fn from_entropy(input: &mut impl codec::Input) -> Result<Self, codec::Error> {
        Ok(match input.read_byte()? % 3 {
            0 => Self::Ed25519(sp_core::crypto::FromEntropy::from_entropy(input)?),
            1 => Self::Sr25519(FromEntropy::from_entropy(input)?),
            2.. => Self::Ecdsa(sp_core::crypto::FromEntropy::from_entropy(input)?),
        })
    }
}

/// NOTE: This implementations is required by `SimpleAddressDeterminer`,
/// we convert the hash into some AccountId, it's fine to use any scheme.
/*impl<T: Into<H256>> crypto::UncheckedFrom<T> for ResonanceSigner {
    fn unchecked_from(x: T) -> Self {
        ed25519::Public::unchecked_from(x.into()).into()
    }
}*/
impl<T: Into<H256>> crypto::UncheckedFrom<T> for ResonanceSigner {
    fn unchecked_from(x: T) -> Self {
        let h: H256 = x.into();
        //let mut data = [0u8; 32];
        //data.copy_from_slice(h.as_ref());
        ResonanceSigner::Ed25519(ed25519::Public::from_raw(h.into()))
    }
}

impl AsRef<[u8]> for ResonanceSigner {
    fn as_ref(&self) -> &[u8] {
        match *self {
            Self::Ed25519(ref who) => who.as_ref(),
            Self::Sr25519(ref who) => who.as_ref(),
            Self::Ecdsa(ref who) => who.as_ref(),
        }
    }
}

impl traits::IdentifyAccount for ResonanceSigner {
    type AccountId = ResonanceAccountId;
    fn into_account(self) -> ResonanceAccountId {
        match self {
            Self::Ed25519(who) => <[u8; 32]>::from(who).into(),
            Self::Sr25519(who) => <[u8; 32]>::from(who).into(),
            Self::Ecdsa(who) => sp_io::hashing::blake2_256(who.as_ref()).into(),
        }
    }
}

impl From<ed25519::Public> for ResonanceSigner {
    fn from(x: ed25519::Public) -> Self {
        Self::Ed25519(x)
    }
}

impl TryFrom<ResonanceSigner> for ed25519::Public {
    type Error = ();
    fn try_from(m: ResonanceSigner) -> Result<Self, Self::Error> {
        if let ResonanceSigner::Ed25519(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

impl From<sr25519::Public> for ResonanceSigner {
    fn from(x: sr25519::Public) -> Self {
        Self::Sr25519(x)
    }
}

impl TryFrom<ResonanceSigner> for sr25519::Public {
    type Error = ();
    fn try_from(m: ResonanceSigner) -> Result<Self, Self::Error> {
        if let ResonanceSigner::Sr25519(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

impl From<ecdsa::Public> for ResonanceSigner {
    fn from(x: ecdsa::Public) -> Self {
        Self::Ecdsa(x)
    }
}

impl TryFrom<ResonanceSigner> for ecdsa::Public {
    type Error = ();
    fn try_from(m: ResonanceSigner) -> Result<Self, Self::Error> {
        if let ResonanceSigner::Ecdsa(x) = m {
            Ok(x)
        } else {
            Err(())
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for ResonanceSigner {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Ed25519(who) => write!(fmt, "ed25519: {}", who),
            Self::Sr25519(who) => write!(fmt, "sr25519: {}", who),
            Self::Ecdsa(who) => write!(fmt, "ecdsa: {}", who),
        }
    }
}



impl Verify for ResonanceSignature {
    type Signer = ResonanceSigner;
    fn verify<L: Lazy<[u8]>>(&self, mut msg: L, signer: &ResonanceAccountId) -> bool {
        let who: [u8; 32] = *signer.as_ref();
        match self {
            Self::Ed25519(sig) => sig.verify(msg, &who.into()),
            Self::Sr25519(sig) => sig.verify(msg, &who.into()),
            Self::Ecdsa(sig) => {
                let m = sp_io::hashing::blake2_256(msg.get());
                sp_io::crypto::secp256k1_ecdsa_recover_compressed(sig.as_ref(), &m)
                    .map_or(false, |pubkey| sp_io::hashing::blake2_256(&pubkey) == who)
            },
        }
    }
}