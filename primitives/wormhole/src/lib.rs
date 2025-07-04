//! Wormhole pallet primitives
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;

/// Trait for managing wormhole transfer proofs.
pub trait TransferProofs<Balance, AccountId, TxCount = u64> {
    /// Get transfer proof, if any
    fn transfer_proof_exists(
        count: TxCount,
        from: &AccountId,
        to: &AccountId,
        value: Balance,
    ) -> bool;

    /// Get transfer proof key
    fn transfer_proof_key(
        count: TxCount,
        from: AccountId,
        to: AccountId,
        value: Balance,
    ) -> Vec<u8>;

    /// Store transfer proofs for a given wormhole transfer.
    fn store_transfer_proof(from: &AccountId, to: &AccountId, value: Balance);
}
