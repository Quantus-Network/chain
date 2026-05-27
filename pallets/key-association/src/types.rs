//! Types for the key-association pallet.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_core::{ecdsa, ed25519};
use sp_runtime::RuntimeDebug;

/// Supported classical key types.
///
/// These are the pre-quantum cryptographic keys that users may want to
/// associate with their post-quantum ML-DSA-87 accounts for migration purposes.
#[derive(
	Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, RuntimeDebug,
)]
pub enum ClassicalKey {
	/// ECDSA secp256k1 compressed public key (33 bytes).
	/// Compatible with Ethereum, Bitcoin, and other secp256k1-based chains.
	Ecdsa(ecdsa::Public),
	/// Ed25519 public key (32 bytes).
	/// Compatible with Polkadot (Sr25519 uses the same curve), Solana, etc.
	Ed25519(ed25519::Public),
}

impl ClassicalKey {
	/// Returns the key type discriminant.
	pub fn key_type(&self) -> KeyType {
		match self {
			ClassicalKey::Ecdsa(_) => KeyType::Ecdsa,
			ClassicalKey::Ed25519(_) => KeyType::Ed25519,
		}
	}
}

/// Signature types matching the classical keys.
#[derive(
	Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, RuntimeDebug,
)]
pub enum ClassicalSignature {
	/// ECDSA signature (65 bytes: r + s + recovery byte).
	Ecdsa(ecdsa::Signature),
	/// Ed25519 signature (64 bytes).
	Ed25519(ed25519::Signature),
}

/// Key type discriminant for events.
#[derive(
	Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, TypeInfo, MaxEncodedLen, RuntimeDebug,
)]
pub enum KeyType {
	/// ECDSA secp256k1
	Ecdsa,
	/// Ed25519
	Ed25519,
}

/// Metadata stored with each key association.
#[derive(
	Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, RuntimeDebug,
)]
pub struct AssociationRecord<BlockNumber> {
	/// Block number when the association was created.
	pub created_at: BlockNumber,
}
