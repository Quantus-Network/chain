//! Wormhole pallet primitives
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// Trait for recording transfer proofs in the wormhole pallet.
/// Other pallets can use this to record proofs when they mint/transfer tokens.
pub trait TransferProofRecorder<AccountId, AssetId, Balance> {
	/// Error type for proof recording failures
	type Error;

	/// Record a transfer proof for native or asset tokens
	/// - `None` for native tokens (asset_id = 0)
	/// - `Some(asset_id)` for specific assets
	fn record_transfer_proof(
		asset_id: Option<AssetId>,
		from: AccountId,
		to: AccountId,
		amount: Balance,
	) -> Result<(), Self::Error>;
}
