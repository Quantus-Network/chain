//! Implementations for Scheduler pallet

// Note: OnTimestampSet is no longer needed as we process timestamp-based
// agendas directly in on_initialize using T::TimeProvider::now().
// This prevents skipped timestamp buckets due to long block times.
