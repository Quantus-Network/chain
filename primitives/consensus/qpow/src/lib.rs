#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, Encode};
use scale_info::TypeInfo;
extern crate alloc;
use alloc::vec::Vec;

/// Engine ID for QPoW consensus.
pub const QPOW_ENGINE_ID: [u8; 4] = *b"QPoW";

sp_api::decl_runtime_apis! {
    pub trait QPoWApi {
        /// Check if solution is valid with given difficulty
        fn verify_solution(
            header: [u8; 32],
            solution: [u8; 64],
            difficulty: u64,
        ) -> bool;

        /// Get the current difficulty target for proof generation
        fn get_difficulty() -> u64;

        /// Retrieve latest submitted proof
        fn get_latest_proof() -> Option<[u8; 64]>;
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