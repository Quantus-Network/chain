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
		/// calculate hash of header with nonce using Bitcoin-style double Poseidon2
		fn get_nonce_distance(
			block_hash: [u8; 32],  // 256-bit block hash
			nonce: [u8; 64], // 512-bit nonce
		) -> U512;

		/// Get the max possible reorg depth
		fn get_max_reorg_depth() -> u32;

		/// Get the max possible difficulty for work calculation
		fn get_max_difficulty() -> U512;

		/// Get the current difficulty (max_distance / distance_threshold)
		fn get_difficulty() -> U512;



		/// Get total work
		fn get_total_work() -> U512;

		/// Get block ema
		fn get_block_time_ema() -> u64;

		/// Get last block timestamp
		fn get_last_block_time() -> u64;

		// Get last block mining time
		fn get_last_block_duration() -> u64;

		fn get_chain_height() -> u32;

		fn verify_nonce_on_import_block(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;
		fn verify_nonce_local_mining(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;
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
