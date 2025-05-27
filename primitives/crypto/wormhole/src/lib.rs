//! # Wormhole Utilities
//!
//! This module provides functionality for generating and verifying wormhole-based addresses,
//! using Poseidon hashing and a salt-based construction to derive deterministic addresses
//! from secrets or pre-hashed secrets.
//!
//! ## Overview
//!
//! - A `WormholePair` consists of:
//!     - `address`: a Poseidon-derived `H256` address.
//!     - `secret`: a 32-byte Poseidon hash derived from entropy or input secret.
//!
//! - You can:
//!     - Generate new wormhole identities using random entropy (`generate_new`).
//!     - Verify if a given secret or pre-hashed secret matches a wormhole address.
//!
//! The hashing strategy ensures determinism while hiding the original secret.

use poseidon_resonance::PoseidonHasher;
use sp_core::{Hasher, H256};

/// Salt used when deriving wormhole addresses.
pub const ADDRESS_SALT: [u8; 8] = *b"wormhole";

/// Error types returned from wormhole identity operations.
#[derive(Debug, Eq, PartialEq)]
pub enum WormholeError {
    /// Returned when the input random source fails or is malformed.
    InvalidSecretFormat,
}

/// A struct representing a wormhole identity pair: address + secret.
#[derive(Clone, Eq, PartialEq)]
pub struct WormholePair {
    /// Deterministic Poseidon-derived address.
    pub address: H256,
    /// The hashed secret used to generate this address.
    pub secret: [u8; 32],
}

impl WormholePair {
    /// Generates a new `WormholePair` using secure system entropy (only available with `std`).
    ///
    /// # Errors
    /// Returns `WormholeError::InvalidSecretFormat` if entropy collection fails.
    #[cfg(feature = "std")]
    pub fn generate_new() -> Result<WormholePair, WormholeError> {
        use rand::rngs::OsRng;
        use rand::RngCore;

        let mut random_bytes = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut random_bytes)
            .map_err(|_| WormholeError::InvalidSecretFormat)?;

        let secret = PoseidonHasher::hash(&random_bytes);

        Ok(Self::generate_pair_from_secret(&secret.0))
    }

    /// Verifies whether the given raw secret generates the specified wormhole address.
    ///
    /// # Arguments
    /// * `address` - The expected wormhole address.
    /// * `secret` - Raw secret to verify.
    ///
    /// # Returns
    /// `Ok(true)` if the address matches the derived one, `Ok(false)` otherwise.
    pub fn verify(address: &H256, secret: &[u8; 32]) -> Result<bool, WormholeError> {
        let generated_address = Self::generate_pair_from_secret(secret).address;
        Ok(&generated_address == address)
    }

    /// Verifies whether the given combined hash generates the specified wormhole address.
    ///
    /// # Arguments
    /// * `address` - The expected wormhole address.
    /// * `combined_hash` - The result of the first Poseidon hash of (salt + secret).
    ///                     This is the intermediate hash before the final address derivation.
    ///
    /// # Returns
    /// `Ok(true)` if the address matches the derived one, `Ok(false)` otherwise.
    pub fn verify_with_combined_hash(
        address: &H256,
        combined_hash: &[u8; 32],
    ) -> Result<bool, WormholeError> {
        let generated = PoseidonHasher::hash(combined_hash);
        Ok(&generated == address)
    }

    /// Internal function that generates a `WormholePair` from a given secret.
    ///
    /// This function performs a secondary Poseidon hash over the salt + hashed secret
    /// to derive the wormhole address.
    fn generate_pair_from_secret(secret: &[u8; 32]) -> WormholePair {
        let mut combined = Vec::with_capacity(ADDRESS_SALT.len() + secret.as_ref().len());
        combined.extend_from_slice(&ADDRESS_SALT);
        combined.extend_from_slice(secret.as_ref());

        WormholePair {
            address: PoseidonHasher::hash(PoseidonHasher::hash(&combined).as_ref()),
            secret: *secret,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn test_generate_pair_from_secret() {
        // Arrange
        let secret = [42u8; 32];

        // Act
        let pair = WormholePair::generate_pair_from_secret(&secret);

        // Assert
        assert_eq!(pair.secret, secret);

        // We can't easily predict the exact hash output without mocking PoseidonHasher,
        // but we can verify that it's not zero and that it's deterministic
        assert_ne!(pair.address, H256::zero());

        // Verify determinism
        let pair2 = WormholePair::generate_pair_from_secret(&secret);
        assert_eq!(pair.address, pair2.address);
    }

    #[test]
    fn test_verify_valid_secret() {
        // Arrange
        let secret = [1u8; 32];
        let pair = WormholePair::generate_pair_from_secret(&secret);

        // Act
        let result = WormholePair::verify(&pair.address, &secret);

        // Assert
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_invalid_secret() {
        // Arrange
        let secret = [1u8; 32];
        let wrong_secret = [2u8; 32];
        let pair = WormholePair::generate_pair_from_secret(&secret);

        // Act
        let result = WormholePair::verify(&pair.address, &wrong_secret);

        // Assert
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_verify_with_combined_hash() {
        // Arrange
        let secret = [3u8; 32];

        // Generate a pair using our normal process
        let pair = WormholePair::generate_pair_from_secret(&secret);

        // Create the combined salt + secret
        let mut combined = Vec::with_capacity(ADDRESS_SALT.len() + secret.len());
        combined.extend_from_slice(&ADDRESS_SALT);
        combined.extend_from_slice(&secret);

        // This is the combined hash (first hash in the two-step process)
        let combined_hash = PoseidonHasher::hash(&combined);

        // Act
        let result = WormholePair::verify_with_combined_hash(&pair.address, &combined_hash.0);

        // Assert
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_address_derivation_properties() {
        // Arrange
        let secret = hex!("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");

        // Act - Generate the pair
        let pair = WormholePair::generate_pair_from_secret(&secret);

        // Assert
        // 1. Verify that the secret is stored correctly
        assert_eq!(pair.secret, secret);

        // 2. Verify that the derived address is consistent with our verification method
        assert!(WormholePair::verify(&pair.address, &secret).unwrap());

        // 3. Verify that even a small change in the secret produces a different address
        let mut altered_secret = secret;
        altered_secret[0] ^= 1; // Flip one bit in the first byte
        let altered_pair = WormholePair::generate_pair_from_secret(&altered_secret);
        assert_ne!(pair.address, altered_pair.address);

        // 4. Verify that the process uses the salt
        // (Create a direct hash without salt and ensure it's different)
        let direct_hash = PoseidonHasher::hash(&secret);
        let double_hash = PoseidonHasher::hash(&direct_hash.0);
        assert_ne!(pair.address, double_hash);

        // 5. Verify that each stage of the hash process changes the result
        // (Create a hash with salt but without the second hashing step)
        let mut combined = Vec::with_capacity(ADDRESS_SALT.len() + secret.len());
        combined.extend_from_slice(&ADDRESS_SALT);
        combined.extend_from_slice(&secret);
        let first_hash = PoseidonHasher::hash(&combined);
        assert_ne!(pair.address, first_hash);
    }

    #[test]
    fn test_different_secrets_produce_different_addresses() {
        // Arrange
        let secret1 = [5u8; 32];
        let secret2 = [6u8; 32];

        // Act
        let pair1 = WormholePair::generate_pair_from_secret(&secret1);
        let pair2 = WormholePair::generate_pair_from_secret(&secret2);

        // Assert
        assert_ne!(pair1.address, pair2.address);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_generate_new_produces_valid_pair() {
        // Act
        let result = WormholePair::generate_new();

        // Assert
        assert!(result.is_ok());
        let pair = result.unwrap();

        // The secret should not be all zeros
        assert_ne!(pair.secret, [0u8; 32]);

        // Address should not be zero
        assert_ne!(pair.address, H256::zero());

        // Verification should work with the generated secret
        let verification = WormholePair::verify(&pair.address, &pair.secret);
        assert!(verification.is_ok());
        assert!(verification.unwrap());
    }

    #[test]
    fn test_salt_is_used_in_address_generation() {
        // This test verifies that the salt affects the generated address

        // Arrange
        let secret = [7u8; 32];

        // Generate a pair normally (with salt)
        let pair_with_salt = WormholePair::generate_pair_from_secret(&secret);

        // Simulate address generation without salt or with different salt
        let different_salt = b"diffrent";

        let mut combined = Vec::with_capacity(different_salt.len() + secret.len());
        combined.extend_from_slice(different_salt);
        combined.extend_from_slice(&secret);

        let first_hash = PoseidonHasher::hash(&combined);
        let address_with_different_salt = PoseidonHasher::hash(&first_hash.0);

        // Assert
        assert_ne!(pair_with_salt.address, address_with_different_salt);
    }
}
