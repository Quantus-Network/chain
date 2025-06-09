//! Common primitives for the Quantus blockchain.
#![cfg_attr(not(feature = "std"), no_std)]

/// Scheduler related traits and types.
pub mod scheduler {
    use codec::{Codec, Decode, Encode, EncodeLike, MaxEncodedLen};
    use frame_support::{
        traits::{
            schedule::{self, v3::TaskName, DispatchTime as DispatchBlock},
            Bounded,
        },
        Parameter,
    };
    use scale_info::TypeInfo;
    use sp_runtime::{
        traits::{Hash, One, Saturating, Zero},
        DispatchError, RuntimeDebug,
    };

    /// Information relating to the period of a scheduled task. First item is the length of the
    /// period and the second is the number of times it should be executed in total before the task
    /// is considered finished and removed.
    pub type Period<BlockNumber, Moment> = (BlockNumberOrTimestamp<BlockNumber, Moment>, u32);

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
    )]
    pub enum BlockNumberOrTimestamp<BlockNumber, Moment> {
        BlockNumber(BlockNumber),
        Timestamp(Moment),
    }

    impl<BlockNumber, Moment> BlockNumberOrTimestamp<BlockNumber, Moment>
    where
        BlockNumber: Saturating + Copy + Parameter + One + Zero,
        Moment: Saturating + Copy + Parameter + Zero,
    {
        /// Returns the block number if it is a block number.
        pub fn as_block_number(&self) -> Option<BlockNumber> {
            match self {
                BlockNumberOrTimestamp::BlockNumber(x) => Some(*x),
                BlockNumberOrTimestamp::Timestamp(_) => None,
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
        ) -> Option<BlockNumberOrTimestamp<BlockNumber, Moment>> {
            match (self, other) {
                (
                    BlockNumberOrTimestamp::BlockNumber(x),
                    BlockNumberOrTimestamp::BlockNumber(y),
                ) => Some(BlockNumberOrTimestamp::BlockNumber(x.saturating_add(*y))),
                (BlockNumberOrTimestamp::Timestamp(x), BlockNumberOrTimestamp::Timestamp(y)) => {
                    Some(BlockNumberOrTimestamp::Timestamp(x.saturating_add(*y)))
                }
                _ => None,
            }
        }
    }

    /// The dispatch time of a scheduled task.
    ///
    /// This is an extended version of `frame_support::traits::schedule::DispatchTime` which allows
    /// for a task to be scheduled at or close to specific timestamps. This is useful for chains that
    /// does not have a fixed block time, such as PoW chains.
    #[derive(Encode, Decode, Copy, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
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
                DispatchBlock::After(x) => {
                    DispatchTime::After(BlockNumberOrTimestamp::BlockNumber(x))
                }
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
}
