//! Custom governance origins, dispatched by passed tech referenda on
//! dedicated tracks (see `definitions::TechCollectiveTracksInfo`).

// The `#[frame_support::pallet]` macro generates `expect()` calls (PalletInfo lookups).
#![allow(clippy::expect_used)]

pub use pallet_custom_origins::*;

#[frame_support::pallet]
pub mod pallet_custom_origins {
	use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
	use frame_support::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[derive(
		PartialEq, Eq, Clone, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
	)]
	#[pallet::origin]
	pub enum Origin {
		/// Dispatched by an approved referendum on the fast-upgrade track; may only
		/// authorize a runtime upgrade (`system.authorize_upgrade`), never arbitrary
		/// Root calls.
		FastUpgrade,
	}

	/// `EnsureOrigin` accepting only [`Origin::FastUpgrade`].
	pub struct FastUpgrade;

	impl<O: Into<Result<Origin, O>> + From<Origin>> EnsureOrigin<O> for FastUpgrade {
		type Success = ();

		fn try_origin(o: O) -> Result<Self::Success, O> {
			o.into().map(|Origin::FastUpgrade| ())
		}

		#[cfg(feature = "runtime-benchmarks")]
		fn try_successful_origin() -> Result<O, ()> {
			Ok(O::from(Origin::FastUpgrade))
		}
	}
}
