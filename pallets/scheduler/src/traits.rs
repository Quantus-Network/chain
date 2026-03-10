//! Traits for the Scheduler pallet.

use crate::DispatchTime;
use codec::{Codec, EncodeLike, MaxEncodedLen};
use frame_support::traits::{
	schedule::{self, v3::TaskName},
	Bounded,
};
use sp_runtime::{traits::Hash, DispatchError};

/// A trait for scheduling tasks with a name, and with an approximate dispatch time.
pub trait ScheduleNamed<BlockNumber, Moment, Call, Origin> {
	/// Address type for the scheduled task.
	type Address: Codec + MaxEncodedLen + Clone + Eq + EncodeLike + core::fmt::Debug;
	/// The type of the hash function used for hashing.
	type Hasher: Hash;

	fn schedule_named(
		id: TaskName,
		when: DispatchTime<BlockNumber, Moment>,
		priority: schedule::Priority,
		origin: Origin,
		call: Bounded<Call, Self::Hasher>,
	) -> Result<Self::Address, DispatchError>;

	fn cancel_named(id: TaskName) -> Result<(), DispatchError>;

	fn reschedule_named(
		id: TaskName,
		when: DispatchTime<BlockNumber, Moment>,
	) -> Result<Self::Address, DispatchError>;

	fn next_dispatch_time(id: TaskName) -> Result<BlockNumber, DispatchError>;
}
