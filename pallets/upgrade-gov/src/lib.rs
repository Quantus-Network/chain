#![cfg_attr(not(feature = "std"), no_std)]

//! # Upgrade Governance Pallet
//!
//! A deliberately minimal governance pallet whose only purpose is to authorize runtime
//! upgrades behind an M-of-N member approval plus a timelock.
//!
//! Unlike a general referenda/sudo system, this pallet can NEVER dispatch an arbitrary call.
//! The complete set of effects it can produce is the fixed [`Action`] enum: authorize a runtime
//! upgrade by code hash, or adjust its own membership / threshold / enactment delay. Membership
//! and configuration are self-governed through the same propose/approve flow, so no external
//! `Root` origin is required.
//!
//! Flow:
//! 1. A member calls [`Pallet::propose`] with an [`Action`].
//! 2. Members call [`Pallet::approve`]. Once the number of approving current members reaches the
//!    threshold, the proposal is "armed": `enact_at = now + enactment_delay`.
//! 3. In `on_initialize`, every armed proposal whose `enact_at` has elapsed is re-validated against
//!    current membership and enacted. `AuthorizeUpgrade` calls
//!    [`frame_system::Pallet::do_authorize_upgrade`]; the runtime blob is then supplied
//!    permissionlessly via `System::apply_authorized_upgrade`, so this pallet only ever stores a
//!    32-byte hash.

extern crate alloc;

pub mod weights;
pub use weights::WeightInfo;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

const LOG_TARGET: &str = "runtime::upgrade-gov";

#[frame_support::pallet]
pub mod pallet {
	use super::{WeightInfo, LOG_TARGET};
	use alloc::vec::Vec;
	use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
	use frame_support::{pallet_prelude::*, weights::Weight};
	use frame_system::pallet_prelude::*;
	use scale_info::TypeInfo;
	use sp_runtime::traits::Saturating;

	/// In-code storage version. Bump and add a migration when the layout changes.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The aggregated event type.
		type RuntimeEvent: From<Event<Self>>
			+ IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Maximum number of members in the collective.
		#[pallet::constant]
		type MaxMembers: Get<u32>;

		/// Maximum number of proposals that may be live at once. Bounds `on_initialize` work.
		#[pallet::constant]
		type MaxProposals: Get<u32>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// The fixed set of effects the collective can authorize. There is intentionally no variant
	/// that dispatches an arbitrary call.
	#[derive(
		Clone,
		Encode,
		Decode,
		DecodeWithMemTracking,
		Eq,
		PartialEq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	pub enum Action<AccountId, Hash, BlockNumber> {
		/// Authorize a runtime upgrade to the given code hash (with version checks).
		AuthorizeUpgrade(Hash),
		/// Add a new member to the collective.
		AddMember(AccountId),
		/// Remove a member from the collective.
		RemoveMember(AccountId),
		/// Set the approval threshold.
		SetThreshold(u32),
		/// Set the enactment delay (in blocks) applied after a proposal is armed.
		SetEnactmentDelay(BlockNumber),
	}

	/// Concrete [`Action`] for this runtime.
	pub type ActionOf<T> = Action<
		<T as frame_system::Config>::AccountId,
		<T as frame_system::Config>::Hash,
		BlockNumberFor<T>,
	>;

	/// A pending proposal.
	#[derive(
		Clone,
		Encode,
		Decode,
		DecodeWithMemTracking,
		Eq,
		PartialEq,
		RuntimeDebug,
		TypeInfo,
		MaxEncodedLen,
	)]
	#[scale_info(skip_type_params(T))]
	pub struct Proposal<T: Config> {
		/// The effect to apply on enactment.
		pub action: ActionOf<T>,
		/// Current members who have approved.
		pub approvals: BoundedVec<T::AccountId, T::MaxMembers>,
		/// Block at which the proposal may be enacted, once the threshold is reached.
		pub enact_at: Option<BlockNumberFor<T>>,
	}

	/// Members allowed to propose and approve.
	#[pallet::storage]
	pub type Members<T: Config> =
		StorageValue<_, BoundedVec<T::AccountId, T::MaxMembers>, ValueQuery>;

	/// Number of approving members required to arm a proposal.
	#[pallet::storage]
	pub type Threshold<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// Delay (in blocks) between arming and enactment.
	#[pallet::storage]
	pub type EnactmentDelay<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

	/// Next proposal id.
	#[pallet::storage]
	pub type NextProposalId<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// Number of live proposals (bounded by `MaxProposals`).
	#[pallet::storage]
	pub type ProposalCount<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// Live proposals by id.
	#[pallet::storage]
	pub type Proposals<T: Config> = StorageMap<_, Twox64Concat, u32, Proposal<T>, OptionQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		/// Initial members.
		pub members: Vec<T::AccountId>,
		/// Initial approval threshold.
		pub threshold: u32,
		/// Initial enactment delay in blocks.
		pub enactment_delay: BlockNumberFor<T>,
	}

	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { members: Vec::new(), threshold: 1, enactment_delay: BlockNumberFor::<T>::from(0u32) }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			let members = BoundedVec::<T::AccountId, T::MaxMembers>::try_from(self.members.clone())
				.expect("upgrade-gov genesis members exceed MaxMembers; chain misconfigured");

			let mut sorted = members.clone().into_inner();
			let len_before = sorted.len();
			sorted.sort();
			sorted.dedup();
			assert!(
				sorted.len() == len_before,
				"upgrade-gov genesis members contain duplicates"
			);

			if !self.members.is_empty() {
				assert!(self.threshold > 0, "upgrade-gov threshold must be > 0");
				assert!(
					self.threshold <= members.len() as u32,
					"upgrade-gov threshold exceeds member count"
				);
			}

			Members::<T>::put(members);
			Threshold::<T>::put(self.threshold.max(1));
			EnactmentDelay::<T>::put(self.enactment_delay);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A proposal was created.
		Proposed { id: u32, who: T::AccountId },
		/// A proposal was approved by a member.
		Approved { id: u32, who: T::AccountId },
		/// A proposal reached the threshold and is scheduled for enactment.
		Armed { id: u32, enact_at: BlockNumberFor<T> },
		/// A proposal was cancelled.
		Cancelled { id: u32 },
		/// A proposal was enacted successfully.
		Enacted { id: u32 },
		/// A proposal failed at enactment and was discarded.
		EnactmentFailed { id: u32 },
		/// A member was added.
		MemberAdded { who: T::AccountId },
		/// A member was removed.
		MemberRemoved { who: T::AccountId },
		/// The approval threshold was changed.
		ThresholdChanged { threshold: u32 },
		/// The enactment delay was changed.
		EnactmentDelayChanged { delay: BlockNumberFor<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The caller is not a member.
		NotMember,
		/// No proposal with this id.
		UnknownProposal,
		/// The caller already approved this proposal.
		AlreadyApproved,
		/// The proposal already reached its threshold and is locked for enactment.
		AlreadyArmed,
		/// The collective is full.
		TooManyMembers,
		/// The account is already a member.
		AlreadyMember,
		/// The threshold is zero or exceeds the member count.
		InvalidThreshold,
		/// Too many live proposals.
		TooManyProposals,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(now: BlockNumberFor<T>) -> Weight {
			let count = ProposalCount::<T>::get() as u64;
			let mut weight = T::DbWeight::get().reads(count.saturating_add(1));

			let due: Vec<u32> = Proposals::<T>::iter()
				.filter_map(|(id, p)| match p.enact_at {
					Some(at) if at <= now => Some(id),
					_ => None,
				})
				.collect();

			for id in due {
				weight = weight.saturating_add(Self::enact(id));
			}
			weight
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a proposal. Caller must be a member; the proposer counts as the first approval.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::propose())]
		pub fn propose(origin: OriginFor<T>, action: ActionOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_member(&who), Error::<T>::NotMember);
			Self::validate_action(&action)?;
			ensure!(
				ProposalCount::<T>::get() < T::MaxProposals::get(),
				Error::<T>::TooManyProposals
			);

			let mut approvals = BoundedVec::<T::AccountId, T::MaxMembers>::new();
			approvals.try_push(who.clone()).map_err(|_| Error::<T>::TooManyMembers)?;
			let enact_at = Self::arm_if_threshold(&approvals);

			let id = NextProposalId::<T>::get();
			Proposals::<T>::insert(id, Proposal { action, approvals, enact_at });
			NextProposalId::<T>::put(id.saturating_add(1));
			ProposalCount::<T>::mutate(|c| *c = c.saturating_add(1));

			Self::deposit_event(Event::Proposed { id, who });
			if let Some(enact_at) = enact_at {
				Self::deposit_event(Event::Armed { id, enact_at });
			}
			Ok(())
		}

		/// Approve a proposal. Once a threshold of current members approve, it is armed.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::approve())]
		pub fn approve(origin: OriginFor<T>, id: u32) -> DispatchResult {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_member(&who), Error::<T>::NotMember);

			let mut proposal = Proposals::<T>::get(id).ok_or(Error::<T>::UnknownProposal)?;
			ensure!(proposal.enact_at.is_none(), Error::<T>::AlreadyArmed);
			ensure!(!proposal.approvals.contains(&who), Error::<T>::AlreadyApproved);
			proposal.approvals.try_push(who.clone()).map_err(|_| Error::<T>::TooManyMembers)?;
			proposal.enact_at = Self::arm_if_threshold(&proposal.approvals);
			let enact_at = proposal.enact_at;
			Proposals::<T>::insert(id, proposal);

			Self::deposit_event(Event::Approved { id, who });
			if let Some(enact_at) = enact_at {
				Self::deposit_event(Event::Armed { id, enact_at });
			}
			Ok(())
		}

		/// Cancel a proposal. Any member may cancel.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::cancel())]
		pub fn cancel(origin: OriginFor<T>, id: u32) -> DispatchResult {
			let who = ensure_signed(origin)?;
			ensure!(Self::is_member(&who), Error::<T>::NotMember);
			ensure!(Proposals::<T>::contains_key(id), Error::<T>::UnknownProposal);
			Proposals::<T>::remove(id);
			ProposalCount::<T>::mutate(|c| *c = c.saturating_sub(1));
			Self::deposit_event(Event::Cancelled { id });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Whether `who` is a current member.
		pub fn is_member(who: &T::AccountId) -> bool {
			Members::<T>::get().contains(who)
		}

		/// Light pre-validation at proposal time. Full validation happens at enactment.
		fn validate_action(action: &ActionOf<T>) -> DispatchResult {
			if let Action::SetThreshold(t) = action {
				ensure!(*t > 0, Error::<T>::InvalidThreshold);
			}
			Ok(())
		}

		/// If the number of approving *current* members meets the threshold, return the enactment
		/// block; otherwise `None`.
		fn arm_if_threshold(
			approvals: &BoundedVec<T::AccountId, T::MaxMembers>,
		) -> Option<BlockNumberFor<T>> {
			let valid = approvals.iter().filter(|a| Self::is_member(a)).count() as u32;
			if valid >= Threshold::<T>::get() {
				let now = frame_system::Pallet::<T>::block_number();
				Some(now.saturating_add(EnactmentDelay::<T>::get()))
			} else {
				None
			}
		}

		/// Take a due proposal, re-validate it against current membership, apply it, and emit the
		/// outcome. Never panics; failures are logged and surfaced as `EnactmentFailed`.
		fn enact(id: u32) -> Weight {
			let proposal = match Proposals::<T>::take(id) {
				Some(p) => p,
				None => return T::DbWeight::get().reads(1),
			};
			ProposalCount::<T>::mutate(|c| *c = c.saturating_sub(1));

			let valid = proposal.approvals.iter().filter(|a| Self::is_member(a)).count() as u32;
			if valid < Threshold::<T>::get() {
				log::error!(
					target: LOG_TARGET,
					"proposal {id} has insufficient current approvals at enactment ({valid} < threshold)"
				);
				Self::deposit_event(Event::EnactmentFailed { id });
				return T::DbWeight::get().reads_writes(2, 2);
			}

			match Self::apply_action(&proposal.action) {
				Ok(()) => Self::deposit_event(Event::Enacted { id }),
				Err(e) => {
					log::error!(target: LOG_TARGET, "proposal {id} enactment failed: {e:?}");
					Self::deposit_event(Event::EnactmentFailed { id });
				},
			}
			T::DbWeight::get().reads_writes(2, 3)
		}

		/// Apply a single action. The only externally-visible effect of `AuthorizeUpgrade` is a
		/// call to `frame_system::do_authorize_upgrade`.
		fn apply_action(action: &ActionOf<T>) -> DispatchResult {
			match action {
				Action::AuthorizeUpgrade(code_hash) => {
					frame_system::Pallet::<T>::do_authorize_upgrade(*code_hash, true);
				},
				Action::AddMember(who) => {
					Members::<T>::try_mutate(|members| -> DispatchResult {
						ensure!(!members.contains(who), Error::<T>::AlreadyMember);
						members.try_push(who.clone()).map_err(|_| Error::<T>::TooManyMembers)?;
						Ok(())
					})?;
					Self::deposit_event(Event::MemberAdded { who: who.clone() });
				},
				Action::RemoveMember(who) => {
					Members::<T>::try_mutate(|members| -> DispatchResult {
						let pos = members
							.iter()
							.position(|m| m == who)
							.ok_or(Error::<T>::NotMember)?;
						ensure!(
							Threshold::<T>::get() <= (members.len() as u32).saturating_sub(1),
							Error::<T>::InvalidThreshold
						);
						members.remove(pos);
						Ok(())
					})?;
					Self::deposit_event(Event::MemberRemoved { who: who.clone() });
				},
				Action::SetThreshold(t) => {
					ensure!(*t > 0, Error::<T>::InvalidThreshold);
					ensure!(
						*t <= Members::<T>::get().len() as u32,
						Error::<T>::InvalidThreshold
					);
					Threshold::<T>::put(*t);
					Self::deposit_event(Event::ThresholdChanged { threshold: *t });
				},
				Action::SetEnactmentDelay(d) => {
					EnactmentDelay::<T>::put(*d);
					Self::deposit_event(Event::EnactmentDelayChanged { delay: *d });
				},
			}
			Ok(())
		}
	}
}
