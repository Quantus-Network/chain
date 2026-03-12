//! Wormhole pallet primitives
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use codec::Decode;
use qp_poseidon::{PoseidonHasher, ToFelts};
pub use sp_consensus_qpow::POW_ENGINE_ID;
use sp_runtime::generic::DigestItem;

/// Trait for recording transfer proofs in the wormhole pallet.
/// Other pallets can use this to record proofs when they mint/transfer tokens.
pub trait TransferProofRecorder<AccountId, AssetId, Balance> {
	/// Record a transfer proof for native or asset tokens
	/// - `None` for native tokens (asset_id = 0)
	/// - `Some(asset_id)` for specific assets
	fn record_transfer_proof(
		asset_id: Option<AssetId>,
		from: AccountId,
		to: AccountId,
		amount: Balance,
	);
}

/// Derive a wormhole address from a 32-byte preimage.
///
/// This hashes the preimage using Poseidon to get the wormhole account address.
/// The preimage is the "first_hash" from wormhole derivation: `first_hash = hash(salt + secret)`.
/// The wormhole address is: `address = hash(first_hash)`.
///
/// This function uses the same serialization as the ZK circuit:
/// - Convert 32 bytes to 4 field elements using ToFelts (8 bytes per element)
/// - Hash without padding using hash_variable_length
pub fn derive_wormhole_address(preimage: [u8; 32]) -> [u8; 32] {
	let preimage_felts = preimage.to_felts();
	PoseidonHasher::hash_variable_length(preimage_felts)
}

/// Derive a wormhole AccountId32 from a 32-byte preimage.
///
/// This is a convenience wrapper around `derive_wormhole_address` that returns
/// an `sp_core::crypto::AccountId32` directly. Useful for tests.
#[cfg(feature = "std")]
pub fn derive_wormhole_account(preimage: [u8; 32]) -> sp_core::crypto::AccountId32 {
	sp_core::crypto::AccountId32::from(derive_wormhole_address(preimage))
}

/// Extract the block author (miner) account from a digest.
///
/// This looks for a pre-runtime digest entry with POW_ENGINE_ID containing
/// a 32-byte preimage, then derives the wormhole address from it and decodes
/// it as the specified AccountId type.
///
/// Returns `None` if no valid pre-runtime digest is found or decoding fails.
pub fn extract_author_from_digest<AccountId, Digest>(digest: Digest) -> Option<AccountId>
where
	AccountId: Decode,
	Digest: IntoIterator<Item = DigestItem>,
{
	for log in digest {
		if let DigestItem::PreRuntime(engine_id, data) = log {
			if engine_id == POW_ENGINE_ID && data.len() == 32 {
				let preimage: [u8; 32] = match data.as_slice().try_into() {
					Ok(arr) => arr,
					Err(_) => continue,
				};
				let address_bytes = derive_wormhole_address(preimage);
				return AccountId::decode(&mut &address_bytes[..]).ok();
			}
		}
	}
	None
}
