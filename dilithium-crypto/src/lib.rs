#![no_std]

use scale_info::prelude::string::String;
use sp_core::{
    crypto::{Derive, PublicBytes, Signature, SignatureBytes}, ByteArray, Pair, Public
};
use sp_runtime::CryptoType;
use sp_std::vec::Vec;
use sp_core::crypto::SecretStringError;
use sp_core::crypto::DeriveError;
use sp_core::crypto::DeriveJunction;
use sp_std::vec;

use sp_std::prelude::ToOwned;

////// TEST
#[derive(Clone, Eq, PartialEq, Debug, Hash)]
struct TestCryptoTag;

#[derive(Clone, Eq, PartialEq, Debug)]
enum TestPair {
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
#[derive(Clone, Eq, PartialEq, Hash)]
struct WrappedPublicBytes<const N: usize, SubTag>(PublicBytes<N, SubTag>);

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

type TestPublic = WrappedPublicBytes<100, TestCryptoTag>;

/// 
/// Signature type
/// 
/// Note: The underlying type for signaturebytes is the exact same as for publicbytes. 
/// So in order to not have to implement the same methods twice, we use the same type for both.
/// Except that doesn't work for some reason, so lets do it the hard way.
/// 
#[derive(Clone, Eq, PartialEq, Hash)]
struct WrappedSignatureBytes<const N: usize, SubTag>(SignatureBytes<N, SubTag>);

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

type TestSignature = WrappedSignatureBytes<1000, TestCryptoTag>;

impl Pair for TestPair {
    type Public = TestPublic;
    type Seed = [u8; 8];
    type Signature = TestSignature;

    fn generate() -> (Self, <Self as Pair>::Seed) {
        (TestPair::Generated, [0u8; 8])
    }

    fn generate_with_phrase(_password: Option<&str>) -> (Self, String, <Self as Pair>::Seed) {
        (TestPair::GeneratedWithPhrase, "".into(), [0u8; 8])
    }

    fn from_phrase(
        phrase: &str,
        password: Option<&str>,
    ) -> Result<(Self, <Self as Pair>::Seed), SecretStringError> {
        Ok((
            TestPair::GeneratedFromPhrase {
                phrase: phrase.to_owned(),
                password: password.map(Into::into),
            },
            [0u8; 8],
        ))
    }

    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self,
        path_iter: Iter,
        _: Option<[u8; 8]>,
    ) -> Result<(Self, Option<[u8; 8]>), DeriveError> {
        Ok((
            match self.clone() {
                TestPair::Standard { phrase, password, path } => TestPair::Standard {
                    phrase,
                    password,
                    path: path.into_iter().chain(path_iter).collect(),
                },
                TestPair::GeneratedFromPhrase { phrase, password } =>
                    TestPair::Standard { phrase, password, path: path_iter.collect() },
                x =>
                    if path_iter.count() == 0 {
                        x
                    } else {
                        return Err(DeriveError::SoftKeyInPath)
                    },
            },
            None,
        ))
    }

    fn sign(&self, _message: &[u8]) -> Self::Signature {
        TestSignature::default()
    }

    fn verify<M: AsRef<[u8]>>(_: &Self::Signature, _: M, _: &Self::Public) -> bool {
        true
    }

    fn public(&self) -> Self::Public {
        TestPublic::default()
    }

    fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
        Ok(TestPair::Seed(seed.to_vec()))
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        vec![]
    }
}

///// DUMMY

// pub struct DummyTag;

// /// Dummy cryptography. Doesn't do anything.
// pub type Dummy = CryptoBytes<0, DummyTag>;

// impl CryptoType for Dummy {
//     type Pair = Dummy;
// }

// impl Derive for Dummy {}

// impl Public for Dummy {}

// impl Signature for Dummy {}

// impl Pair for Dummy {
//     type Public = Dummy;
//     type Seed = Dummy;
//     type Signature = Dummy;

//     #[cfg(feature = "std")]
//     fn generate_with_phrase(_: Option<&str>) -> (Self, String, Self::Seed) {
//         Default::default()
//     }

//     #[cfg(feature = "std")]
//     fn from_phrase(_: &str, _: Option<&str>) -> Result<(Self, Self::Seed), SecretStringError> {
//         Ok(Default::default())
//     }

//     fn derive<Iter: Iterator<Item = DeriveJunction>>(
//         &self,
//         _: Iter,
//         _: Option<Dummy>,
//     ) -> Result<(Self, Option<Dummy>), DeriveError> {
//         Ok((Self::default(), None))
//     }

//     fn from_seed_slice(_: &[u8]) -> Result<Self, SecretStringError> {
//         Ok(Self::default())
//     }

//     fn sign(&self, _: &[u8]) -> Self::Signature {
//         Self::default()
//     }

//     fn verify<M: AsRef<[u8]>>(_: &Self::Signature, _: M, _: &Self::Public) -> bool {
//         true
//     }

//     fn public(&self) -> Self::Public {
//         Self::default()
//     }

//     fn to_raw_vec(&self) -> Vec<u8> {
//         Default::default()
//     }
// }
