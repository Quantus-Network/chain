#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod tests;

use sp_core::{Hasher, H256};
use poseidon_resonance::PoseidonHasher;
use core::result::Result;

pub const ADDRESS_SALT: [u8; 8] = *b"wormhole";
pub const MAX_SECRET_SIZE: usize = 1024; // 1KB max secret size

/// Error types for wormhole operations
#[derive(Debug, Eq, PartialEq)]
pub enum WormholeError {
    /// Secret is empty
    EmptySecret,
    /// Secret exceeds maximum allowed size
    SecretTooLarge,
    /// Invalid secret format
    InvalidSecretFormat,
}

pub struct Wormhole{}

impl Wormhole {
    /// Generates a wormhole address from a secret.
    ///
    /// The wormhole address is a double hash of the salt and secret.
    /// This makes the address provably unspendable because it's not derived from a private key.
    ///
    /// # Arguments
    ///
    /// * `secret` - A byte slice that holds the secret value
    ///
    /// # Returns
    ///
    /// * `Result<H256, WormholeError>` - The generated wormhole address or an error
    ///
    /// # Errors
    ///
    /// Returns `WormholeError::EmptySecret` if the secret is empty.
    /// Returns `WormholeError::SecretTooLarge` if the secret exceeds the maximum size.
    pub fn generate_wormhole_address(secret: &[u8]) -> Result<H256, WormholeError> {
        // Validate secret
        if secret.is_empty() {
            return Err(WormholeError::EmptySecret);
        }

        if secret.len() > MAX_SECRET_SIZE {
            return Err(WormholeError::SecretTooLarge);
        }

        // Combine salt and secret
        let mut combined = Vec::with_capacity(ADDRESS_SALT.len() + secret.len());
        combined.extend_from_slice(&ADDRESS_SALT);
        combined.extend_from_slice(secret);

        // Apply double hashing
        let wormhole_address = PoseidonHasher::hash(PoseidonHasher::hash(&combined).as_ref());

        Ok(wormhole_address)
    }

    /// Verifies if an address is a wormhole address generated from a specific secret.
    ///
    /// # Arguments
    ///
    /// * `address` - The address to verify
    /// * `secret` - The secret that was allegedly used to generate the address
    ///
    /// # Returns
    ///
    /// * `Result<bool, WormholeError>` - True if the address matches the expected wormhole address
    pub fn verify_wormhole_address(address: &H256, secret: &[u8]) -> Result<bool, WormholeError> {
        let generated = Self::generate_wormhole_address(secret)?;
        Ok(&generated == address)
    }
}