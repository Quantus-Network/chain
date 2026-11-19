#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::vec::Vec;
use primitive_types::U512;
use sp_runtime::ConsensusEngineId;

pub const POW_ENGINE_ID: ConsensusEngineId = [b'p', b'o', b'w', b'_'];

pub type Seal = Vec<u8>;

/// Import error for a block the node judged invalid, as opposed to a failure of the node itself.
///
/// Block import wraps this in `sp_consensus::Error::Other`. The import queue reports that
/// variant without a peer, so sync looks for this type to decide whether to disconnect the
/// peer that supplied the block.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct InvalidBlock(alloc::string::String);

#[cfg(feature = "std")]
impl InvalidBlock {
	/// `reason` is the text of the error that rejected the block.
	pub fn new(reason: impl Into<alloc::string::String>) -> Self {
		Self(reason.into())
	}
}

#[cfg(feature = "std")]
impl core::fmt::Display for InvalidBlock {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.0)
	}
}

#[cfg(feature = "std")]
impl std::error::Error for InvalidBlock {}

/// Import error for a block whose timestamp is further ahead of this node's clock than the
/// runtime allows.
///
/// The block is not invalid: the same block passes once the clock catches up. Sync does not
/// penalise the peer that supplied it and asks again later.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct FutureBlock;

#[cfg(feature = "std")]
impl core::fmt::Display for FutureBlock {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str("block timestamp is too far in the future")
	}
}

#[cfg(feature = "std")]
impl std::error::Error for FutureBlock {}

sp_api::decl_runtime_apis! {
	pub trait QPoWApi {
		/// Get the max possible reorg depth
		fn get_max_reorg_depth() -> u32;

		/// Get the max possible difficulty for work calculation
		fn get_max_difficulty() -> U512;

		/// Get the current mining difficulty
		fn get_difficulty() -> U512;

		/// Get last block timestamp
		fn get_last_block_time() -> u64;

		/// Get last block mining time
		fn get_last_block_duration() -> u64;

		fn get_chain_height() -> u32;

		fn verify_nonce_on_import_block(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;

		fn verify_nonce_local_mining(block_hash: [u8; 32], nonce: [u8; 64]) -> bool;

		fn verify_and_get_achieved_difficulty(block_hash: [u8; 32], nonce: [u8; 64]) -> (bool, U512);
	}
}
