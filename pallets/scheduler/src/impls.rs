//! Implementations for Scheduler pallet
use super::*;

impl<T: Config> OnTimestampSet<T::Moment> for Pallet<T> {
    fn on_timestamp_set(now: T::Moment) {
        let mut weight_counter = WeightMeter::with_limit(T::MaximumWeight::get());
        Self::service_agendas(
            &mut weight_counter,
            BlockNumberOrTimestamp::Timestamp(now),
            u32::max_value(),
        );
        weight_counter.consumed();
    }
}
