#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, Encode};
use scale_info::TypeInfo;
extern crate alloc;
use alloc::vec::Vec;
use primitive_types::U512;

/// Engine ID for QPoW consensus.
pub const QPOW_ENGINE_ID: [u8; 4] = *b"QPoW";

sp_api::decl_runtime_apis! {
    pub trait QPoWApi {
        /// Verify a nonce for a block being imported from the network
        fn verify_for_import(
            header: [u8; 32],
            nonce: [u8; 64],
        ) -> bool;

        /// Verify a nonce for a historical block that's already in the chain
        fn verify_historical_block(
            header: [u8; 32],
            nonce: [u8; 64],
            block_number: u32,
        ) -> bool;

        /// Submit a locally mined nonce
        fn submit_nonce(
            header: [u8; 32],
            nonce: [u8; 64],
        ) -> bool;

        /// Get the current difficulty target for proof generation
        fn get_difficulty() -> u64;

        /// Get difficulty at block
        fn get_difficulty_at_block(block_number: u32) -> u64;

        /// Get total difficulty at block
        fn get_total_difficulty_at_block(block_number: u32) -> u128;

        /// Get median block time for preconfigured list of elements
        fn get_median_block_time() -> u64;

        /// Get last block timestamp
        fn get_last_block_time() -> u64;

        // Get last block mining time
        fn get_last_block_duration() -> u64;

        /// Retrieve latest submitted proof
        fn get_latest_nonce() -> Option<[u8; 64]>;

        fn get_random_rsa(header: &[u8; 32]) -> (U512, U512);
        fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512;
    }
}

#[derive(Debug, Encode, Decode, TypeInfo)]
pub enum Error {
    /// Invalid proof submitted
    InvalidProof,
    /// Arithmetic calculation error
    ArithmeticError,
    /// Other error occurred
    Other(Vec<u8>),
}