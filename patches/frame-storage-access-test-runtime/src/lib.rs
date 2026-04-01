#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use codec::Decode;
use sp_core::storage::ChildInfo;
use sp_runtime::traits;
use sp_trie::StorageProof;

#[cfg(feature = "std")]
pub const WASM_BINARY: Option<&[u8]> = None;

#[derive(Decode, Clone)]
#[cfg_attr(feature = "std", derive(codec::Encode))]
pub struct StorageAccessParams<B: traits::Block> {
	pub state_root: B::Hash,
	pub storage_proof: StorageProof,
	pub payload: StorageAccessPayload,
	pub is_dry_run: bool,
}

#[derive(Debug, Clone, Decode, codec::Encode)]
pub enum StorageAccessPayload {
	Read(Vec<(Vec<u8>, Option<ChildInfo>)>),
	Write((Vec<(Vec<u8>, Vec<u8>)>, Option<ChildInfo>)),
}

impl<B: traits::Block> StorageAccessParams<B> {
	pub fn new_read(
		state_root: B::Hash,
		storage_proof: StorageProof,
		payload: Vec<(Vec<u8>, Option<ChildInfo>)>,
	) -> Self {
		Self {
			state_root,
			storage_proof,
			payload: StorageAccessPayload::Read(payload),
			is_dry_run: false,
		}
	}

	pub fn new_write(
		state_root: B::Hash,
		storage_proof: StorageProof,
		payload: (Vec<(Vec<u8>, Vec<u8>)>, Option<ChildInfo>),
	) -> Self {
		Self {
			state_root,
			storage_proof,
			payload: StorageAccessPayload::Write(payload),
			is_dry_run: false,
		}
	}

	pub fn as_dry_run(&self) -> Self {
		Self {
			state_root: self.state_root,
			storage_proof: self.storage_proof.clone(),
			payload: self.payload.clone(),
			is_dry_run: true,
		}
	}
}

#[cfg(feature = "std")]
pub fn wasm_binary_unwrap() -> &'static [u8] {
	WASM_BINARY.expect("WASM binary not available")
}
