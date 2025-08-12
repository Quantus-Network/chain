//! Common primitives for the Quantus blockchain.
#![cfg_attr(not(feature = "std"), no_std)]

/// Scheduler related traits and types.
use codec::{Codec, Decode, DecodeWithMemTracking, Encode, EncodeLike, MaxEncodedLen};
use frame_support::{
	traits::{
		schedule::{self, v3::TaskName, DispatchTime as DispatchBlock},
		Bounded,
	},
	Parameter,
};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{CheckedDiv, Hash, One, Saturating, Zero},
	DispatchError, RuntimeDebug,
};

/// Information relating to the period of a scheduled task. First item is the length of the
/// period and the second is the number of times it should be executed in total before the task
/// is considered finished and removed.
pub type Period<BlockNumber, Moment> = (BlockNumberOrTimestamp<BlockNumber, Moment>, u32);

/// Error type for incompatible types in saturating_add
#[derive(Debug, PartialEq, Eq)]
pub struct IncompatibleTypesError;

/// Block number or timestamp.
#[derive(
	Encode,
	Decode,
	Copy,
	Clone,
	PartialEq,
	Eq,
	RuntimeDebug,
	TypeInfo,
	MaxEncodedLen,
	Ord,
	PartialOrd,
	DecodeWithMemTracking,
)]
pub enum BlockNumberOrTimestamp<BlockNumber, Moment> {
	BlockNumber(BlockNumber),
	Timestamp(Moment),
}

impl<BlockNumber, Moment> BlockNumberOrTimestamp<BlockNumber, Moment>
where
	BlockNumber: Saturating + Copy + Parameter + One + Zero,
	Moment: Saturating + Copy + Parameter + Zero + CheckedDiv,
{
	/// Normalize timestamp value
	pub fn normalize(&self, precision: Moment) -> Self {
		match self {
			BlockNumberOrTimestamp::BlockNumber(_) => *self,
			BlockNumberOrTimestamp::Timestamp(t) => {
				let stripped_t =
					t.checked_div(&precision).unwrap_or(Zero::zero()).saturating_mul(precision);

				BlockNumberOrTimestamp::Timestamp(stripped_t.saturating_add(precision))
			},
		}
	}

	/// Returns the block number if it is a block number.
	pub fn as_block_number(&self) -> Option<BlockNumber> {
		match self {
			BlockNumberOrTimestamp::BlockNumber(x) => Some(*x),
			BlockNumberOrTimestamp::Timestamp(_) => None,
		}
	}

	/// Returns the timestamp if it is a timestamp
	pub fn as_timestamp(&self) -> Option<Moment> {
		match self {
			BlockNumberOrTimestamp::BlockNumber(_) => None,
			BlockNumberOrTimestamp::Timestamp(x) => Some(*x),
		}
	}

	/// Is zero
	pub fn is_zero(&self) -> bool {
		match self {
			BlockNumberOrTimestamp::BlockNumber(x) => x.is_zero(),
			BlockNumberOrTimestamp::Timestamp(x) => x.is_zero(),
		}
	}

	/// Saturating add two `BlockNumberOrTimestamp`.
	pub fn saturating_add(
		&self,
		other: &BlockNumberOrTimestamp<BlockNumber, Moment>,
	) -> Result<BlockNumberOrTimestamp<BlockNumber, Moment>, IncompatibleTypesError> {
		match (self, other) {
			(BlockNumberOrTimestamp::BlockNumber(x), BlockNumberOrTimestamp::BlockNumber(y)) =>
				Ok(BlockNumberOrTimestamp::BlockNumber(x.saturating_add(*y))),
			(BlockNumberOrTimestamp::Timestamp(x), BlockNumberOrTimestamp::Timestamp(y)) =>
				Ok(BlockNumberOrTimestamp::Timestamp(x.saturating_add(*y))),
			_ => Err(IncompatibleTypesError),
		}
	}
}

/// The dispatch time of a scheduled task.
///
/// This is an extended version of `frame_support::traits::schedule::DispatchTime` which allows
/// for a task to be scheduled at or close to specific timestamps. This is useful for chains that
/// does not have a fixed block time, such as PoW chains.
#[derive(
	Encode,
	Decode,
	Copy,
	Clone,
	PartialEq,
	Eq,
	RuntimeDebug,
	TypeInfo,
	MaxEncodedLen,
	DecodeWithMemTracking,
)]
pub enum DispatchTime<BlockNumber, Moment> {
	/// At specified block.
	At(BlockNumber),
	/// After specified number of blocks.
	After(BlockNumberOrTimestamp<BlockNumber, Moment>),
}

impl<BlockNumber, Moment> From<DispatchBlock<BlockNumber>> for DispatchTime<BlockNumber, Moment> {
	fn from(value: DispatchBlock<BlockNumber>) -> Self {
		match value {
			DispatchBlock::At(x) => DispatchTime::At(x),
			DispatchBlock::After(x) => DispatchTime::After(BlockNumberOrTimestamp::BlockNumber(x)),
		}
	}
}

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
		maybe_periodic: Option<Period<BlockNumber, Moment>>,
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

#[cfg(test)]
mod tests {
	use super::BlockNumberOrTimestamp; // Adjust path as needed

	type DefaultBlockNumberOrTimestamp = BlockNumberOrTimestamp<u64, u64>;

	#[test]
	fn normalize_block_number_is_unchanged() {
		let bn = DefaultBlockNumberOrTimestamp::BlockNumber(123u64);
		assert_eq!(bn.normalize(10u64), DefaultBlockNumberOrTimestamp::BlockNumber(123u64));
	}

	#[test]
	fn normalize_timestamp_mid_bucket() {
		// Tests the common case: timestamp within a bucket.
		// Expected: start of the *next* bucket.
		let ts = DefaultBlockNumberOrTimestamp::Timestamp(15500u64); // Bucket [14000, 15999]
															   // Calculation: (15500 / 2000) * 2000 + 2000 = 14000 + 2000 = 16000
		assert_eq!(ts.normalize(2000u64), DefaultBlockNumberOrTimestamp::Timestamp(16000u64));
	}

	#[test]
	fn normalize_timestamp_at_bucket_start_boundary() {
		// Tests behavior when timestamp is exactly at a bucket start.
		// Expected: start of the *next* bucket.
		let ts = DefaultBlockNumberOrTimestamp::Timestamp(14000u64); // Exactly at start of bucket [14000, 15999]
		let precision = 2000u64;
		// Calculation: (14000 / 2000) * 2000 + 2000 = 14000 + 2000 = 16000
		assert_eq!(ts.normalize(precision), DefaultBlockNumberOrTimestamp::Timestamp(16000u64));
	}

	#[test]
	fn normalize_timestamp_zero_value() {
		// Tests the zero timestamp edge case.
		// Expected: 0 + precision.
		let ts = DefaultBlockNumberOrTimestamp::Timestamp(0u64);
		let precision = 2000u64;
		// Calculation: (0 / 2000) * 2000 + 2000 = 0 + 2000 = 2000
		assert_eq!(ts.normalize(precision), DefaultBlockNumberOrTimestamp::Timestamp(2000u64));
	}

	#[test]
	fn normalize_timestamp_less_than_precision() {
		// Tests when timestamp is smaller than the precision (falls into the first bucket [0,
		// precision-1]). Expected: 0 + precision.
		let ts = DefaultBlockNumberOrTimestamp::Timestamp(500u64);
		let precision = 2000u64;
		// Calculation: (500 / 2000) * 2000 + 2000 = 0 + 2000 = 2000
		assert_eq!(ts.normalize(precision), DefaultBlockNumberOrTimestamp::Timestamp(2000u64));
	}
}
