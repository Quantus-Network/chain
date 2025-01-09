#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use scale_info::TypeInfo;
use sp_runtime::generic::BlockId;
extern crate alloc;
use alloc::vec::Vec;
use scale_info::build::Fields;
use sp_api::__private::BlockT;
use sp_runtime::RuntimeDebug;
use sp_runtime::traits::NumberFor;

/// Engine ID for QPoW consensus.
pub const QPOW_ENGINE_ID: [u8; 4] = *b"QPoW";

sp_api::decl_runtime_apis! {
    pub trait QPoWApi {
        /// Submit and verify a QPoW proof
        fn submit_proof(
            header: [u8; 32],
            solution: [u8; 64],
            difficulty: u32
        ) -> Result<bool, Error>;

        // Compute the proof of work function
        fn compute_pow(
            header: [u8; 32],
            difficulty: u32,
            solution: [u8; 64]
        )  -> (Vec<u8>, Vec<u8>);

        /// Get the current difficulty target for proof generation
        fn get_difficulty() -> u32;

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