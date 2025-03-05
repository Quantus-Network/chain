use crate::{types::ResonanceCryptoTag, ResonanceSignatureScheme, ResonanceSigner, WrappedSignatureBytes};

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
            ResonancePair::Standard {
                phrase,
                password,
                path,
            } => {
                // Append the new path to the existing one
                let combined_path = path.iter().cloned().chain(new_path.into_iter()).collect();
                Ok((
                    ResonancePair::Standard {
                        phrase: phrase.clone(),
                        password: password.clone(),
                        path: combined_path,
                    },
                    None,
                ))
            }
            ResonancePair::GeneratedFromPhrase { phrase, password } => {
                // Convert to Standard with the new path
                Ok((
                    ResonancePair::Standard {
                        phrase: phrase.clone(),
                        password: password.clone(),
                        path: new_path,
                    },
                    None,
                ))
            }
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
            ResonancePair::Generated | ResonancePair::GeneratedWithPhrase => {
                if new_path.is_empty() {
                    // No path to derive; return unchanged
                    Ok((self.clone(), None))
                } else {
                    // These variants don't naturally support derivation paths
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
            ResonancePair::Generated => {
                unimplemented!("Generated can't be used for signing");
                // No specific data provided; generate random entropy
                // This might not be ideal for reproducibility; consider requiring a seed
                // let mut entropy = sp_std::vec![0u8; 32]; // 32 bytes is typical for HD wallets
                // // In a real system, use a cryptographically secure RNG
                // #[cfg(feature = "std")]
                // rand::Rng::fill(&mut rand::thread_rng(), &mut entropy[..]);
                // entropy
            }
            ResonancePair::GeneratedWithPhrase => {
                unimplemented!("GeneratedWithPhrase can't be used for signing");

                // Similar to Generated, but could imply a default phrase
                // For now, treat as random entropy (adjust as needed)
                // let mut entropy = vec![0u8; 32];
                // #[cfg(feature = "std")]
                // rand::Rng::fill(&mut rand::thread_rng(), &mut entropy[..]);
                // entropy
            }
            ResonancePair::GeneratedFromPhrase { phrase, password } => {
                unimplemented!("GeneratedFromPhrase can't be used for signing");
                // Convert mnemonic phrase (and optional password) to seed
                // This assumes a BIP-39-like mnemonic-to-seed function
                // hdwallet::mnemonic_to_seed(phrase, password.as_deref().unwrap_or(""))
                //     .expect("Invalid mnemonic phrase")
            }
            ResonancePair::Standard {
                phrase,
                password,
                path,
            } => {
                unimplemented!("Standard can't be used for signing");

                // // Convert phrase to seed, then potentially derive further with path
                // let base_seed = hdwallet::mnemonic_to_seed(phrase, password.as_deref().unwrap_or(""))
                //     .expect("Invalid mnemonic phrase");
                // if path.is_empty() {
                //     base_seed
                // } else {
                //     // Derive a child key seed using the path (HD wallet derivation)
                //     hdwallet::derive_seed_from_base(&base_seed, path)
                //         .expect("Path derivation failed")
                // }
            }
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
        ResonancePublic::default()
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        Vec::new()
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

        let seed = vec![0u8; 64];

        log::warn!("EHLLO:");

        let pair = ResonancePair::Seed(seed.clone());
        let message = b"Hello, world!";
        
        let signature = pair.sign(message);

        log::warn!("Signature length: {}", signature.as_ref().len());
        // log::warn!("Signature: {:?}", signature);

        let public = pair.public();
        log::warn!("Public length: {}", public.as_ref().len());

        let result = ResonancePair::verify(&signature, message, &public);
        
        log::warn!("result: {}", result);

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

