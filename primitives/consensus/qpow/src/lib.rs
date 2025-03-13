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
        /// Check if nonce is valid with given difficulty
        fn verify_nonce(
            header: [u8; 32],
            nonce: [u8; 64],
            difficulty: u64,
        ) -> bool;

        /// Get the current difficulty target for proof generation
        fn get_difficulty() -> u64;
        fn get_median_block_time() -> u64;

        /// Retrieve latest submitted proof
        fn get_latest_proof() -> Option<[u8; 64]>;

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