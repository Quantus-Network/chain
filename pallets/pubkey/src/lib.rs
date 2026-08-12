//! # Pubkey pallet
//!
//! On-chain cache of Dilithium public keys, keyed by the `AccountId32` they
//! hash to (`AccountId = Poseidon(pubkey)`).
//!
//! Dilithium (ML-DSA) signatures cannot recover the public key, and the chain
//! address is only a hash of it, so every signed extrinsic normally has to
//! carry the full public key next to the signature (~1.9–2.6 KB). This pallet
//! removes that cost for all transactions after an account's first one:
//!
//! - The first time an account signs with a full `SignatureWithPublic` transaction, the verified
//!   public key is written to [`Pubkeys`].
//! - From then on the account may sign with the `SigOnly` variants of [`DilithiumSignatureScheme`],
//!   which omit the public key; verification resolves the key from [`Pubkeys`] instead.
//! - When the system account is reaped, the cache entry is removed via [`OnKilledAccount`] so a
//!   fund → register → reap loop cannot leave unbounded orphan state. The next full-signature
//!   transaction re-registers the key.
//!
//! `SigOnly` signatures are domain-separated from full ones: they sign
//! [`sig_only_signing_payload`]`(payload)` (`"qsigonly" ‖ payload`) instead of
//! the raw payload. A signature is therefore valid only in the encoding form
//! its author chose — a third party cannot re-encode a `SigOnly` extrinsic
//! into the ~2 KB larger full form (inflating the sender's length fee and
//! changing the extrinsic hash) or vice versa.
//!
//! The pallet has no extrinsics. Registration happens as a side effect of
//! signature verification in [`CachedSignature`], the runtime's `Signature`
//! type. `CachedSignature` wraps [`DilithiumSignatureScheme`] transparently
//! (identical SCALE encoding), so existing clients that always send the full
//! form keep working unchanged; sending `SigOnly` is an opt-in optimization
//! once the key is known to be cached. Clients can query [`PubkeyApi`] (or
//! [`Pallet::pubkey_of`] via state RPC) to learn whether an account's key is
//! registered before switching to `SigOnly`.
//!
//! ## State economics (accepted trade-off)
//!
//! A cached key parks 1.9–2.6 KB of storage backed by **no deposit**: its
//! standing cost is the account's existential deposit plus one transaction
//! fee, roughly 20× more state per ED than a plain `System::Account` entry
//! buys. This is a deliberate, reviewed decision in favor of keeping
//! registration automatic (no extra extrinsic, no deposit UX on an account's
//! first transaction). The exposure is bounded: entries exist only for live,
//! ED-holding accounts, and the ED cannot be reclaimed without reaping the
//! account — which deletes the entry via [`OnKilledAccount`] — so the state
//! never outlives its collateral. Revisiting this with a storage deposit
//! later requires a migration for already-registered keys.

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::marker::PhantomData;
use frame_support::{dispatch::DispatchClass, traits::OnKilledAccount};
use qp_dilithium_crypto::{
	sig_only_signing_payload, verify_ml_dsa_65, verify_ml_dsa_87, DilithiumSignatureScheme,
	DilithiumSigner,
};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{Lazy, Verify},
	AccountId32, RuntimeDebug,
};

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// This chain's `AccountId` is always the Poseidon hash of a Dilithium
	/// public key (`AccountId32`), so the cache is keyed that way.
	#[pallet::config]
	pub trait Config: frame_system::Config<AccountId = AccountId32> {}

	/// Dilithium public keys cached on chain, keyed by the account they hash to.
	///
	/// An entry is written only after a full `SignatureWithPublic` verification
	/// succeeded, which proves both the signature and `Poseidon(pubkey) ==
	/// account`. The account cannot rotate to a key with a different hash, so
	/// entries never need updating while the account lives; they are removed
	/// when the system account is reaped (see [`OnKilledAccount`] below).
	#[pallet::storage]
	pub type Pubkeys<T: Config> =
		StorageMap<_, Blake2_128Concat, AccountId32, DilithiumSigner, OptionQuery>;

	impl<T: Config> Pallet<T> {
		/// The cached public key for `who`, if any. Once this returns `Some`,
		/// `who` can sign transactions with the `SigOnly` signature variants.
		pub fn pubkey_of(who: &AccountId32) -> Option<DilithiumSigner> {
			Pubkeys::<T>::get(who)
		}

		/// Cache the key produced by `public` for `who` unless already present.
		///
		/// Takes a closure because extracting the key copies 1.9–2.6 KB out of
		/// the signature; on every transaction after an account's first the
		/// entry already exists and the copy must not happen.
		///
		/// Callers must have proven that `who` is the hash of the produced key
		/// (a successful full-signature verification does exactly that).
		pub(crate) fn note_pubkey_with(
			who: &AccountId32,
			public: impl FnOnce() -> DilithiumSigner,
		) {
			if !Pubkeys::<T>::contains_key(who) {
				Pubkeys::<T>::insert(who, public());
			}
		}
	}

	/// Clears the cached public key when the system account is reaped.
	///
	/// The storage write is charged here, via
	/// [`register_extra_weight_unchecked`](frame_system::Pallet::register_extra_weight_unchecked),
	/// rather than on the weights of kill-capable calls: hooking the weight
	/// where the write happens covers every reap path — user-signed,
	/// scheduled, root, or from a pallet added later — by construction.
	/// "Unchecked" is acceptable for one DB write: reaps only occur inside
	/// already-weighted dispatches, so the overshoot per extrinsic is bounded.
	/// Signature-verification base weight does not cover this write — a
	/// first-registration that also reaps performs both the insert and this
	/// remove.
	///
	/// Class attribution is deliberately approximate: `OnKilledAccount` has no
	/// access to the enclosing dispatch's [`DispatchClass`], so the write is
	/// always booked against [`DispatchClass::Normal`]. Reaps inside Operational
	/// or Mandatory paths therefore consume Normal budget they did not use; the
	/// block total stays correct. The remove is also registered on every reap,
	/// including accounts that never cached a key — an over-charge, not an
	/// under-charge.
	impl<T: Config> OnKilledAccount<T::AccountId> for Pallet<T> {
		fn on_killed_account(who: &T::AccountId) {
			Pubkeys::<T>::remove(who);
			frame_system::Pallet::<T>::register_extra_weight_unchecked(
				T::DbWeight::get().writes(1),
				DispatchClass::Normal,
			);
		}
	}
}

sp_api::decl_runtime_apis! {
	/// Queries the on-chain Dilithium pubkey cache.
	pub trait PubkeyApi {
		/// The cached public key for `account`, if any.
		///
		/// Once this returns `Some` for a block the client trusts not to be
		/// reverted, `account` can sign with the `SigOnly` variants of
		/// `DilithiumSignatureScheme` (over the domain-separated payload)
		/// instead of resending the full public key.
		fn pubkey_of(account: AccountId32) -> Option<DilithiumSigner>;
	}
}

/// The runtime `Signature` type: a [`DilithiumSignatureScheme`] whose
/// verification can read and fill the on-chain pubkey cache.
///
/// Encodes exactly like the inner enum, so the wire format of existing
/// full-signature transactions is unchanged.
///
/// Verification behavior per variant:
/// - `Dilithium87`/`Dilithium65` (full): verified self-contained as before, over the raw payload;
///   on success the carried public key is written to [`Pubkeys`] if absent. The write persists only
///   when the extrinsic lands in a block (during transaction-pool validation it goes to a discarded
///   overlay).
/// - `Dilithium87SigOnly`/`Dilithium65SigOnly`: the public key is loaded from [`Pubkeys`], and the
///   signature is checked over the domain-separated message [`sig_only_signing_payload`]`(payload)`
///   so it cannot be re-encoded as (or from) a full signature. Verification fails if no key of the
///   matching parameter set is cached for the claimed signer.
///
/// # Weight accounting
///
/// This runs in `UncheckedExtrinsic::check`, before any `TxExtension` weight or
/// payment handling, so the database work here (one `Pubkeys` read, plus one
/// multi-kilobyte insert on an account's first full-signature transaction) is
/// invisible to the dispatch path. The runtime must charge that worst case in
/// the signed-extrinsic base weight (`PubkeyCacheVerifyWeight`). Separately,
/// account reaping runs `Pubkeys::remove` via `OnKilledAccount`, which
/// registers its own write via `register_extra_weight_unchecked` — a
/// first-registration that also reaps performs both the insert and the remove.
#[derive(Eq, PartialEq, Clone, Encode, Decode, RuntimeDebug, TypeInfo, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct CachedSignature<T>(pub DilithiumSignatureScheme, PhantomData<T>);

impl<T> CachedSignature<T> {
	pub fn new(signature: DilithiumSignatureScheme) -> Self {
		Self(signature, PhantomData)
	}
}

impl<T> From<DilithiumSignatureScheme> for CachedSignature<T> {
	fn from(signature: DilithiumSignatureScheme) -> Self {
		Self::new(signature)
	}
}

impl<T> From<qp_dilithium_crypto::Dilithium87SignatureWithPublic> for CachedSignature<T> {
	fn from(signature: qp_dilithium_crypto::Dilithium87SignatureWithPublic) -> Self {
		Self::new(signature.into())
	}
}

impl<T> From<qp_dilithium_crypto::Dilithium65SignatureWithPublic> for CachedSignature<T> {
	fn from(signature: qp_dilithium_crypto::Dilithium65SignatureWithPublic) -> Self {
		Self::new(signature.into())
	}
}

impl<T> MaxEncodedLen for CachedSignature<T> {
	fn max_encoded_len() -> usize {
		DilithiumSignatureScheme::max_encoded_len()
	}
}

impl<T: Config> Verify for CachedSignature<T> {
	type Signer = DilithiumSigner;

	fn verify<L: Lazy<[u8]>>(&self, mut msg: L, signer: &AccountId32) -> bool {
		match &self.0 {
			DilithiumSignatureScheme::Dilithium87(sig_public) => {
				if !self.0.verify(msg, signer) {
					return false;
				}
				Pallet::<T>::note_pubkey_with(signer, || {
					DilithiumSigner::Dilithium87(sig_public.public())
				});
				true
			},
			DilithiumSignatureScheme::Dilithium65(sig_public) => {
				if !self.0.verify(msg, signer) {
					return false;
				}
				Pallet::<T>::note_pubkey_with(signer, || {
					DilithiumSigner::Dilithium65(sig_public.public())
				});
				true
			},
			DilithiumSignatureScheme::Dilithium87SigOnly(signature) =>
				match Pubkeys::<T>::get(signer) {
					Some(DilithiumSigner::Dilithium87(public)) => verify_ml_dsa_87(
						public.as_ref(),
						&sig_only_signing_payload(msg.get()),
						signature.as_ref(),
					),
					_ => false,
				},
			DilithiumSignatureScheme::Dilithium65SigOnly(signature) =>
				match Pubkeys::<T>::get(signer) {
					Some(DilithiumSigner::Dilithium65(public)) => verify_ml_dsa_65(
						public.as_ref(),
						&sig_only_signing_payload(msg.get()),
						signature.as_ref(),
					),
					_ => false,
				},
		}
	}
}
