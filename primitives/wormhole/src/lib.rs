//! Wormhole pallet primitives
//!
//! This crate provides common types and utilities for the wormhole pallet,
//! including test helpers that can be shared across pallet mocks.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use codec::Decode;
use qp_poseidon_core::rehash_to_bytes;
use sp_consensus_qpow::POW_ENGINE_ID;
use sp_runtime::generic::DigestItem;

// ============================================================================
// Test utilities (std feature only)
// ============================================================================

/// Test helper struct that derives both preimage and account from an arbitrary numeric id.
///
/// This provides a consistent way to create test accounts across different pallet mocks.
/// The preimage is deterministically derived from the id, and the account is derived
/// from the preimage via Poseidon hashing.
///
/// # Example
/// ```ignore
/// use qp_wormhole::TestMiner;
///
/// let miner = TestMiner(1);
/// set_miner_preimage_digest(miner.preimage());
/// assert_eq!(Balances::free_balance(miner.account_id()), expected);
/// ```
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug)]
pub struct TestMiner(pub u64);

#[cfg(feature = "std")]
impl TestMiner {
	/// Generate a deterministic 32-byte preimage from the miner id.
	pub fn preimage(&self) -> [u8; 32] {
		let mut buf = [0u8; 32];
		buf[..8].copy_from_slice(&self.0.to_le_bytes());
		buf
	}

	/// Derive the wormhole account address from the preimage (via Poseidon hash).
	pub fn account_id(&self) -> sp_core::crypto::AccountId32 {
		derive_wormhole_account(self.preimage())
	}
}

/// Helper function to convert a u64 to an AccountId32.
///
/// Encodes the id as little-endian bytes in the first 8 bytes of the 32-byte array.
/// This creates a simple, predictable account address for testing.
///
/// Note: This creates a "raw" account, NOT a wormhole-derived account.
/// For wormhole accounts, use `TestMiner` instead.
#[cfg(feature = "std")]
pub fn account_id(id: u64) -> sp_core::crypto::AccountId32 {
	let mut bytes = [0u8; 32];
	bytes[..8].copy_from_slice(&id.to_le_bytes());
	sp_core::crypto::AccountId32::new(bytes)
}

/// A well-known account used as the "from" address when recording transfer proofs
/// for minted tokens. This is not a real account but a sentinel value.
///
/// Uses `[3u8; 32]` as a simple, recognizable pattern that won't collide with
/// test accounts (which typically use small integers like 1, 2, 3 encoded differently).
#[cfg(feature = "std")]
pub const MINTING_ACCOUNT: sp_core::crypto::AccountId32 =
	sp_core::crypto::AccountId32::new([3u8; 32]);

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

/// Derive a wormhole address from a 32-byte inner_digest (already hashed).
///
/// This hashes the inner_digest using Poseidon to get the wormhole account address.
/// The inner_digest is the "first hash" from wormhole derivation: `hash(salt + secret)`.
/// The wormhole address is: `address = hash(hash(salt + secret))`.
///
/// The inner_digest is the serialization of 4 field elements (Poseidon output),
/// so we decode it back to 4 felts using 8 bytes/felt encoding before hashing again.
///
/// NOTE: If you have a raw secret, use `derive_wormhole_address_from_secret` instead.
pub fn derive_wormhole_address(inner_digest: [u8; 32]) -> [u8; 32] {
	rehash_to_bytes(&inner_digest)
}

/// Derive a wormhole AccountId32 from a 32-byte inner_digest.
///
/// This is a convenience wrapper around `derive_wormhole_address` that returns
/// an `sp_core::crypto::AccountId32` directly.
///
/// NOTE: The input must be an inner_digest (already `H(salt + secret)`), not a raw secret.
/// For raw secrets, use `quantus wormhole address --secret <hex>` to compute the address.
#[cfg(feature = "std")]
pub fn derive_wormhole_account(inner_digest: [u8; 32]) -> sp_core::crypto::AccountId32 {
	sp_core::crypto::AccountId32::from(derive_wormhole_address(inner_digest))
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
