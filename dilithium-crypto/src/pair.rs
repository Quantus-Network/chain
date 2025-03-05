use crate::{ResonanceSignatureScheme, ResonanceSigner, PUB_KEY_BYTES};

use super::types::{ResonancePair, ResonancePublic, ResonanceSignature};
use sp_core::{
    crypto::{DeriveError, DeriveJunction, SecretStringError}, ByteArray, Pair
};
use sp_runtime::traits::{IdentifyAccount, Verify};
use sp_std::vec::Vec;

impl Pair for ResonancePair {
    type Public = ResonancePublic;
    type Seed = Vec<u8>;
    type Signature = ResonanceSignature;

    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self,
        path_iter: Iter,
        seed: Option<<ResonancePair as Pair>::Seed>,
    ) -> Result<(Self, Option<<ResonancePair as Pair>::Seed>), DeriveError> {
        // Collect the path_iter into a Vec to avoid consuming it prematurely in checks
        let new_path: Vec<DeriveJunction> = path_iter.collect();

        match self {
            ResonancePair::Seed(seed_vec) => {
                if new_path.is_empty() {
                    // No derivation needed; return the same seed
                    Ok((
                        ResonancePair::Seed(seed_vec.clone()),
                        Some(seed_vec.clone()),
                    ))
                } else {
                    // Use the provided seed parameter if available, otherwise use the variant's seed
                    let _effective_seed = seed.unwrap_or_else(|| seed_vec.clone());
                    // Here, we could derive a new seed using the path, but since Seed doesn't
                    // inherently support paths, we might need to transition to Standard or reject
                    // For simplicity, reject derivation with paths for raw seeds
                    Err(DeriveError::SoftKeyInPath)
                }
            }
        }
    }

    fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
        Ok(ResonancePair::Seed(seed.to_vec()))
    }

    #[cfg(any(feature = "default", feature = "full_crypto"))]
    fn sign(&self, message: &[u8]) -> ResonanceSignature {
        // Helper function to derive a seed from the variant
        let seed = match self {
            ResonancePair::Seed(seed) => {
                // Directly use the provided seed
                seed.clone()
            }
        };
        // Generate a keypair from the seed
        let keypair = hdwallet::generate(Some(&seed)).expect("Failed to generate keypair");

        // Sign the message
        let signature = keypair
            .sign(message, None, false)
            .expect("Signing should succeed");

        // Wrap the signature bytes in ResonanceSignature
        ResonanceSignature::try_from(signature.as_ref()).expect("Wrap doesn't fail")
    }

    fn verify<M: AsRef<[u8]>>(sig: &ResonanceSignature, message: M, pubkey: &ResonancePublic) -> bool {
        let sig_scheme = ResonanceSignatureScheme::Resonance(sig.clone(), pubkey.as_slice().try_into().unwrap());
        let signer = ResonanceSigner::Resonance(pubkey.clone());
        sig_scheme.verify(message.as_ref(), &signer.into_account())
    }

    fn public(&self) -> Self::Public {
        let seed = match self {
            ResonancePair::Seed(seed) => seed,
        };
        let keypair = hdwallet::generate(Some(&seed)).expect("Failed to generate keypair");
        let pk_bytes: [u8; PUB_KEY_BYTES as usize] = keypair.public.to_bytes();
        ResonancePublic::from_slice(&pk_bytes).expect("Failed to create ResonancePublic")
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        unimplemented!("to_raw_vec not implemented");
    }

    #[cfg(feature = "std")]
    fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError> {
        Self::from_string_with_seed(s, password_override).map(|x| x.0)
    }
}


#[cfg(test)]
mod tests {
    use sp_std::vec;

    use super::*;

    fn setup() {
        // Initialize the logger once per test run
        // Using try_init to avoid panics if called multiple times
        let _ = env_logger::try_init();
    }

    #[test]
    fn test_sign_and_verify() {
        setup();

        let seed = vec![0u8; 32];

        let pair = ResonancePair::from_seed_slice(&seed).expect("Failed to create pair");
        let message: Vec<u8> = b"Hello, world!".to_vec();
        
        let signature = pair.sign(&message);

        // sanity check
        let keypair = hdwallet::generate(Some(&seed)).expect("Failed to generate keypair");
        let sig_bytes = keypair.sign(&message, None, false).expect("Signing failed");
        assert_eq!(signature.as_ref(), sig_bytes, "Signatures should match");

        
        let public = pair.public();

        let result = ResonancePair::verify(&signature, message, &public);

        assert!(result, "Signature should verify");
    }

    #[test]
    fn test_sign_different_message_fails() {
        let seed = vec![0u8; 32];
        let pair = ResonancePair::Seed(seed.clone());
        let message = b"Hello, world!";
        let wrong_message = b"Goodbye, world!";
        
        let signature = pair.sign(message);
        let public = pair.public();
        
        assert!(
            !ResonancePair::verify(&signature, wrong_message, &public),
            "Signature should not verify with wrong message"
        );
    }

    #[test]
    fn test_wrong_signature_fails() {
        let seed = vec![0u8; 32];
        let pair = ResonancePair::Seed(seed.clone());
        let message = b"Hello, world!";
        
        let mut signature = pair.sign(message);
        // Corrupt the signature by flipping a bit
        if let Some(byte) = signature.as_mut().get_mut(0) {
            *byte ^= 1;
        }
        let public = pair.public();
        
        assert!(
            !ResonancePair::verify(&signature, message, &public),
            "Corrupted signature should not verify"
        );
    }

    #[test]
    fn test_different_seed_different_public() {
        let seed1 = vec![0u8; 32];
        let seed2 = vec![1u8; 32];
        let pair1 = ResonancePair::Seed(seed1);
        let pair2 = ResonancePair::Seed(seed2);
        
        let pub1 = pair1.public();
        let pub2 = pair2.public();
        
        assert_ne!(pub1.as_ref(), pub2.as_ref(), "Different seeds should produce different public keys");
    }
}

