#![no_std]

use super::types::{RezPair, RezPublic, RezSignature};
use sp_core::{Pair, crypto::{SecretStringError, DeriveError, DeriveJunction}};
use sp_std::vec::Vec;

impl Pair for RezPair {
    type Public = RezPublic;
    type Seed = [u8; 32]; // Address seed size issue below
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

	#[cfg(feature = "full_crypto")]
    fn sign(&self, _message: &[u8]) -> Self::Signature {
        RezSignature::default()
    }

    fn verify<M: AsRef<[u8]>>(sig: &Self::Signature, message: M, pubkey: &Self::Public) -> bool {
        true // Placeholder; implement actual verification
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