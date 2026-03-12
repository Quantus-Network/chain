#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::vec::Vec;
use primitive_types::U512;
use sp_runtime::ConsensusEngineId;

pub const POW_ENGINE_ID: ConsensusEngineId = [b'p', b'o', b'w', b'_'];

pub type Seal = Vec<u8>;

sp_api::decl_runtime_apis! {
	pub trait QPoWApi {
		/// Get the max possible reorg depth
		fn get_max_reorg_depth() -> u32;

		/// Get the max possible difficulty for work calculation
		fn get_max_difficulty() -> U512;

		/// Get the current difficulty (max_distance / distance_threshold)
		fn get_difficulty() -> U512;

		/// Get block ema
		fn get_block_time_ema() -> u64;

		/// Get last block timestamp
		fn get_last_block_time() -> u64;

		// Get last block mining time
		fn get_last_block_duration() -> u64;
		fn get_chain_height() -> u32;
		fn verify_nonce_on_import_block(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;
		fn verify_nonce_local_mining(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;
		fn verify_and_get_achieved_difficulty(block_hash: [u8; 32], nonce: [u8; 64]) -> (bool, U512);
	}
}
