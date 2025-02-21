#![no_std]

use scale_info::prelude::string::String;
use sp_core::{
    crypto::{Derive, PublicBytes, Signature, SignatureBytes}, ByteArray, /*Get,*/ Pair, Public
};
use sp_runtime::{AccountId32, CryptoType};
use sp_std::vec::Vec;
use sp_core::crypto::SecretStringError;
use sp_core::crypto::DeriveError;
use sp_core::crypto::DeriveJunction;
use codec::{Encode, Decode};
use scale_info::TypeInfo;

use sp_runtime::traits::{IdentifyAccount, Verify};

// use rusty_crystals_dilithium::dilithium5;  // causes errors! TODO
// pub const PUB_KEY_BYTES: usize = dilithium5::PUBLICKEYBYTES;
// pub const SECRET_KEY_BYTES: usize = dilithium5::SECRETKEYBYTES;
// pub const SIGNATURE_BYTES: usize = dilithium5::SIGNBYTES;

////// REZ CRYPTO //////
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

impl CryptoType for RezPair {
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
    type Pair = RezPair;
}

impl<const N: usize, SubTag: Clone + Eq> Public for WrappedPublicBytes<N, SubTag> {}

pub type RezPublic = WrappedPublicBytes<100, RezCryptoTag>;

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
    type Pair = RezPair;
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

pub type RezSignature = WrappedSignatureBytes<1000, RezCryptoTag>;

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
            true
        // Extract public key (2592 bytes) and signature (4595 bytes) from self
        // let public_key_bytes = &self.0[4595..]; // Last 2592 bytes
        // let signature_bytes = &self.0[..4595];  // First 4595 bytes

        // // Convert public key bytes to Dilithium5 PublicKey
        // let public_key = dilithium5::PublicKey::from_bytes(public_key_bytes);

        // // Hash the public key to compare with signer (AccountId32)
        // let pk_hash = sp_io::hashing::blake2_256(public_key_bytes);
        // if <AccountId32 as AsRef<[u8]>>::as_ref(signer) != &pk_hash {            
        //     return false;
        // }

        // // Verify the signature with the extracted public key and message
        // public_key.verify(msg.get(), signature_bytes)
    }
}

        // let extracted_public_key = [0u8; dilithium5::PUBLICKEYBYTES];
        // let extracted_message = [0u8; 32];
        // let public_key = dilithium5::PublicKey::from_bytes(&extracted_public_key);
        // let pk_hash = sp_io::hashing::blake2_256(&extracted_public_key);
        // if <AccountId32 as AsRef<[u8]>>::as_ref(signer) != &pk_hash {            
        //     return false;
        // }
        // public_key.verify(&extracted_message, &extracted_public_key);


impl Pair for RezPair {
    type Public = RezPublic;
    type Seed = [u8; 8];
    type Signature = RezSignature;

    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self,
        path_iter: Iter,
        _seed: Option<<RezPair as Pair>::Seed>,
    ) -> Result<(Self, Option<<RezPair as Pair>::Seed>), DeriveError> {
        Ok((
            match self.clone() {
                #[cfg(feature = "std")]
                RezPair::Standard { phrase, password, path } => RezPair::Standard {
                    phrase,
                    password,
                    path: path.into_iter().chain(path_iter).collect(),
                },
                #[cfg(feature = "std")]
                RezPair::GeneratedFromPhrase { phrase, password } => RezPair::Standard {
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
        Ok(RezPair::Seed(seed.to_vec()))
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


// We got into the following traits implementation when trying to replace MultiSignature with RezSignature
// So the new straregy is to make a custom MultiSignature type that implements all this and has RezSignature as one of the sig options.
// Maybe!? 

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
