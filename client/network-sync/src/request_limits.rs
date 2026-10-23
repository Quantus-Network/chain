// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

use std::time::{Duration, Instant};

const SAME_REQUEST_WINDOW: Duration = Duration::from_secs(60);
const MAX_SAME_REQUESTS: usize = 2;

pub(crate) enum SeenRequest {
	First,
	Fulfilled { requests: usize, since: Instant },
}

impl SeenRequest {
	pub(crate) fn exceeds_limit(&mut self, now: Instant) -> bool {
		match self {
			Self::First => false,
			Self::Fulfilled { requests, since }
				if now.duration_since(*since) <= SAME_REQUEST_WINDOW =>
			{
				*requests = requests.saturating_add(1);
				*requests > MAX_SAME_REQUESTS
			},
			Self::Fulfilled { .. } => {
				*self = Self::First;
				false
			},
		}
	}

	pub(crate) fn note_fulfilled(&mut self, now: Instant) {
		if matches!(self, Self::First) {
			*self = Self::Fulfilled { requests: 1, since: now };
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn duplicate_quota_starts_with_the_first_fulfilled_response() {
		let now = Instant::now();
		let mut seen = SeenRequest::First;
		assert!(!seen.exceeds_limit(now));
		assert!(!seen.exceeds_limit(now));
		seen.note_fulfilled(now);
		assert!(!seen.exceeds_limit(now));
		assert!(seen.exceeds_limit(now));
	}

	#[test]
	fn legitimate_retries_get_a_new_quota_after_the_window() {
		let now = Instant::now();
		let mut seen = SeenRequest::First;
		seen.note_fulfilled(now);
		assert!(!seen.exceeds_limit(now));
		assert!(seen.exceeds_limit(now + SAME_REQUEST_WINDOW));
		let retry = now + SAME_REQUEST_WINDOW + Duration::from_secs(1);
		assert!(!seen.exceeds_limit(retry));
		seen.note_fulfilled(retry);
		assert!(!seen.exceeds_limit(retry));
		assert!(seen.exceeds_limit(retry));
	}
}
