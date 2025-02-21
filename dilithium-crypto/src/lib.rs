#![no_std]

use core::marker::PhantomData;

use scale_info::prelude::string::String;
use sp_core::{
    crypto::{Derive, PublicBytes, Signature, SignatureBytes}, ByteArray, Get, Pair, Public
};
use sp_runtime::{transaction_validity::TransactionValidityError, AccountId32, CryptoType};
use sp_std::vec::Vec;
use sp_core::crypto::SecretStringError;
use sp_core::crypto::DeriveError;
use sp_core::crypto::DeriveJunction;
use codec::{Encode, Decode};
use scale_info::TypeInfo;
use frame_system::Config;

use sp_runtime::{
	traits::{BlakeTwo256, IdentifyAccount, Verify, Checkable, Extrinsic, SignedExtension},
	MultiAddress, MultiSignature,
};

// use sp_core::crypto::ExposeSecret;
// use sp_std::str::FromStr;
// use array_bytes::{Dehexify, Hexify};

// use sp_std::prelude::ToOwned;

////// TEST
#[derive(Clone, Eq, PartialEq, Debug, Hash, Encode, Decode, TypeInfo)]
pub struct TestCryptoTag;

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum TestPair {
    Generated,
    GeneratedWithPhrase,
    GeneratedFromPhrase { phrase: String, password: Option<String> },
    Standard { phrase: String, password: Option<String>, path: Vec<DeriveJunction> },
    Seed(Vec<u8>),
}

impl Default for TestPair {
    fn default() -> Self {
        TestPair::Generated
    }
}

impl CryptoType for TestPair {
    type Pair = Self;
}
#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo)]
pub struct WrappedPublicBytes<const N: usize, SubTag>(PublicBytes<N, SubTag>);

impl<const N: usize, SubTag> Default for WrappedPublicBytes<N, SubTag> {
    fn default() -> Self {
        WrappedPublicBytes(PublicBytes::default())
    }
}
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
            .map(WrappedPublicBytes)
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
            .map(WrappedPublicBytes)
            .map_err(|_| ())
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        self.0.as_slice().to_vec()
    }
}
impl<const N: usize, SubTag> CryptoType for WrappedPublicBytes<N, SubTag> {
    type Pair = TestPair;
}

impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> {}

pub type RezPublic = WrappedPublicBytes<100, TestCryptoTag>;

/// 
/// Signature type
/// 
/// Note: The underlying type for signaturebytes is the exact same as for publicbytes. 
/// So in order to not have to implement the same methods twice, we use the same type for both.
/// Except that doesn't work for some reason, so lets do it the hard way.
/// 
#[derive(Clone, Eq, PartialEq, Hash, Encode, Decode, TypeInfo)]
pub struct WrappedSignatureBytes<const N: usize, SubTag>(SignatureBytes<N, SubTag>);

impl<const N: usize, SubTag> Default for WrappedSignatureBytes<N, SubTag> {
    fn default() -> Self {
        WrappedSignatureBytes(SignatureBytes::default())
    }
}
impl<const N: usize, SubTag> CryptoType for WrappedSignatureBytes<N, SubTag> {
    type Pair = TestPair;
}

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
            .map(WrappedSignatureBytes)
            .map_err(|_| ())
    }
}

//
impl<const N: usize, SubTag> ByteArray for WrappedSignatureBytes<N, SubTag> {
    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
    
    const LEN: usize = N;
    
    fn from_slice(data: &[u8]) -> Result<Self, ()> {
        SignatureBytes::from_slice(data)
            .map(WrappedSignatureBytes)
            .map_err(|_| ())
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        self.0.as_slice().to_vec()
    }
}

impl<const N: usize, SubTag: Clone + Eq> Signature for WrappedSignatureBytes<N, SubTag> {}

pub type RezSignature = WrappedSignatureBytes<1000, TestCryptoTag>;

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
            msg: L,
            signer: &<Self::Signer as IdentifyAccount>::AccountId,
        ) -> bool {
        todo!()
    }
}

impl Pair for TestPair {
    type Public = RezPublic;
    type Seed = [u8; 8];
    type Signature = RezSignature;

    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self,
        path_iter: Iter,
        _seed: Option<<TestPair as Pair>::Seed>,
    ) -> Result<(Self, Option<<TestPair as Pair>::Seed>), DeriveError> {
        Ok((
            match self.clone() {
                #[cfg(feature = "std")]
                TestPair::Standard { phrase, password, path } => TestPair::Standard {
                    phrase,
                    password,
                    path: path.into_iter().chain(path_iter).collect(),
                },
                #[cfg(feature = "std")]
                TestPair::GeneratedFromPhrase { phrase, password } => TestPair::Standard {
                    phrase,
                    password,
                    path: path_iter.collect(),
                },
                x => if path_iter.count() == 0 {
                    x
                } else {
                    return Err(DeriveError::SoftKeyInPath)
                },
            },
            None,
        ))
    }

    fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
        Ok(TestPair::Seed(seed.to_vec()))
    }

	// #[cfg(feature = "full_crypto")] // the OG feature for this is full_crypto 
	#[cfg(feature = "std")]
    fn sign(&self, _message: &[u8]) -> Self::Signature {
        RezSignature::default()
    }

    fn verify<M: AsRef<[u8]>>(sig: &Self::Signature, message: M, pubkey: &Self::Public) -> bool {
        // sig.verify(message, &pubkey.into_account())
        true
    }

    fn public(&self) -> Self::Public {
        RezPublic::default()
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        Vec::new()
    }

    #[cfg(feature = "std")]
    fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError> {
        Self::from_string_with_seed(s, password_override).map(|x| x.0)
    }
    
}


// pub struct ChainContext<Runtime>(sp_std::marker::PhantomData<Runtime>)
// where
//     Runtime: Config;

// type RuntimeCall<Runtime> = <Runtime as Config>::RuntimeCall;

// impl<Runtime> Get<Runtime::Hash> for ChainContext<Runtime>
// where
//     Runtime: Config,
// {
//     fn get() -> Runtime::Hash {
//         Default::default()
//     }
// }

// impl<const N: usize, SubTag, Runtime> Checkable<ChainContext<Runtime>> for WrappedSignatureBytes<N, SubTag>
// where
//     Runtime: Config,
// {
//     type Checked = Self;

//     fn check(self, _c: &ChainContext<Runtime>) -> Result<Self::Checked, TransactionValidityError> {
//         // Implement your signature verification logic here
//         Ok(self)
//     }
// }

// impl<const N: usize, SubTag, RuntimeT> Extrinsic for WrappedSignatureBytes<N, SubTag>
// where
//     RuntimeT: Config,
// {
//     //type Call = RuntimeCall<Runtime>;
//     type SignaturePayload = ();

//     fn is_signed(&self) -> Option<bool> {
//         Some(true)
//     }
    
//     type Call = RuntimeCall<RuntimeT>;
// }
