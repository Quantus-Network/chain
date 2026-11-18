//! Pauses block requests to peers that supplied a block from the future.

use sc_network_types::PeerId;
use schnellru::{ByLength, LruMap};
use std::time::{Duration, Instant};

/// The maximum number of peers to keep a pause for.
const MAX_PAUSED_PEERS: u32 = 512;

/// Pause after the first block from the future. Clocks of honest nodes differ by less.
const FIRST_PAUSE: Duration = Duration::from_secs(1);

/// The pause doubles on each repeat and stops growing here.
const LONGEST_PAUSE: Duration = Duration::from_secs(32);

struct Pause {
	length: Duration,
	until: Instant,
}

/// Peers that supplied a block whose timestamp is ahead of this node's clock.
///
/// The block passes once the clock catches up, so the peer is not penalised. Asking it again
/// at once would fetch the same block and restart sync again, so requests to it wait.
pub struct FutureBlockPeers {
	paused: LruMap<PeerId, Pause>,
}

impl FutureBlockPeers {
	pub fn new() -> Self {
		Self { paused: LruMap::new(ByLength::new(MAX_PAUSED_PEERS)) }
	}

	/// Pause requests to `peer`, and return for how long.
	///
	/// The pause doubles when the peer repeats itself. It starts over once the peer has stayed
	/// quiet, after a pause ends, for as long as that pause lasted.
	pub fn pause(&mut self, peer: PeerId, now: Instant) -> Duration {
		let length = match self.paused.peek(&peer) {
			Some(last) if now < last.until + last.length => (last.length * 2).min(LONGEST_PAUSE),
			_ => FIRST_PAUSE,
		};
		self.paused.insert(peer, Pause { length, until: now + length });
		length
	}

	pub fn is_paused(&self, peer: &PeerId, now: Instant) -> bool {
		self.paused.peek(peer).is_some_and(|pause| now < pause.until)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn pause_ends_after_its_length() {
		let mut peers = FutureBlockPeers::new();
		let peer = PeerId::random();
		let start = Instant::now();
		assert!(!peers.is_paused(&peer, start));

		assert_eq!(peers.pause(peer, start), FIRST_PAUSE);
		assert!(peers.is_paused(&peer, start));
		assert!(peers.is_paused(&peer, start + FIRST_PAUSE - Duration::from_millis(1)));
		assert!(!peers.is_paused(&peer, start + FIRST_PAUSE));
		assert!(!peers.is_paused(&PeerId::random(), start));
	}

	#[test]
	fn repeated_pauses_double_up_to_the_limit() {
		let mut peers = FutureBlockPeers::new();
		let peer = PeerId::random();
		let mut now = Instant::now();
		let mut lengths = Vec::new();

		for _ in 0..8 {
			let length = peers.pause(peer, now);
			lengths.push(length.as_secs());
			// The peer serves the same block again as soon as its pause ends.
			now += length;
		}

		assert_eq!(lengths, [1, 2, 4, 8, 16, 32, 32, 32]);
	}

	#[test]
	fn pause_starts_over_after_a_quiet_spell() {
		let peer = PeerId::random();
		let start = Instant::now();
		// Two pauses back to back. The second lasts two seconds and ends three seconds in.
		let paused_twice = || {
			let mut peers = FutureBlockPeers::new();
			peers.pause(peer, start);
			assert_eq!(peers.pause(peer, start + Duration::from_secs(1)), Duration::from_secs(2));
			peers
		};
		let second_ends = start + Duration::from_secs(3);

		// One millisecond short of a quiet spell as long as the last pause.
		assert_eq!(
			paused_twice().pause(peer, second_ends + Duration::from_millis(1999)),
			Duration::from_secs(4)
		);
		assert_eq!(paused_twice().pause(peer, second_ends + Duration::from_secs(2)), FIRST_PAUSE);
	}
}
