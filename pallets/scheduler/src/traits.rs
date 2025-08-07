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

	/// Schedule a task with a name, dispatch time, and optional periodicity.
	fn schedule_named(
		id: TaskName,
		when: DispatchTime<BlockNumber, Moment>,
		maybe_periodic: Option<crate::Period<BlockNumber, Moment>>,
		priority: schedule::Priority,
		origin: Origin,
		call: Bounded<Call, Self::Hasher>,
	) -> Result<Self::Address, DispatchError>;

	/// Schedule a task with a name, dispatch time, and optional periodicity.
	fn cancel_named(id: TaskName) -> Result<(), DispatchError>;

	/// Reschedule a task with a name, dispatch time, and optional periodicity.
	fn reschedule_named(
		id: TaskName,
		when: DispatchTime<BlockNumber, Moment>,
	) -> Result<Self::Address, DispatchError>;

	/// Get the approximate dispatch block number for a task with a name.
	fn next_dispatch_time(id: TaskName) -> Result<BlockNumber, DispatchError>;
}
