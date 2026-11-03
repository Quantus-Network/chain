//! Status streams for watched private-batch submissions the aggregator
//! claimed.
//!
//! A claimed proof never enters the transaction pool, so its submitter cannot
//! be told about *its* extrinsic. What they get instead tracks the proof's
//! nullifiers: `Ready` on claim, `InBlock` / `Finalized` pointing at the
//! extrinsic that settled them (normally the aggregator's public batch), and
//! `Dropped` if the proof leaves the pool unsettled.

use std::{
	collections::HashMap,
	time::{Duration, Instant},
};

use futures::channel::mpsc;
use qp_wormhole_aggregator::pool::BatchKey;
use qp_wormhole_inputs::BytesDigest;
use quantus_runtime::opaque::Block;
use sc_transaction_pool::InterceptedStatusStream;
use sc_transaction_pool_api::{TransactionStatus, TxIndex};
use sp_core::H256;

type Status = TransactionStatus<H256, H256>;

struct Watcher {
	nullifiers: Vec<BytesDigest>,
	bucket: BatchKey,
	claimed_at: Instant,
	sender: mpsc::UnboundedSender<Status>,
}

impl Watcher {
	fn send(&self, status: Status) {
		let _ = self.sender.unbounded_send(status);
	}

	fn settled_by(&self, settled: &HashMap<BytesDigest, TxIndex>) -> Option<TxIndex> {
		self.nullifiers.iter().find_map(|n| settled.get(n).copied())
	}
}

struct InBlock {
	block: H256,
	number: u32,
	tx_index: TxIndex,
	watcher: Watcher,
}

/// Watchers of claimed proofs, keyed by what they are waiting for.
#[derive(Default)]
pub struct Watchers {
	pending: Vec<Watcher>,
	in_block: Vec<InBlock>,
}

impl Watchers {
	/// Register a watcher for a proof just pooled under `bucket`.
	pub fn watch(
		&mut self,
		nullifiers: Vec<BytesDigest>,
		bucket: BatchKey,
	) -> InterceptedStatusStream<Block> {
		let (sender, receiver) = mpsc::unbounded();
		let watcher = Watcher { nullifiers, bucket, claimed_at: Instant::now(), sender };
		watcher.send(TransactionStatus::Ready);
		self.pending.push(watcher);
		Box::pin(receiver)
	}

	/// Nullifiers settled in `block`: the extrinsic that settled each one.
	pub fn on_settled(
		&mut self,
		block: H256,
		number: u32,
		settled: &HashMap<BytesDigest, TxIndex>,
	) {
		if settled.is_empty() {
			return;
		}
		let (hit, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
			.into_iter()
			.partition(|w| w.settled_by(settled).is_some());
		self.pending = pending;
		for watcher in hit {
			let tx_index = watcher.settled_by(settled).unwrap_or_default();
			watcher.send(TransactionStatus::InBlock((block, tx_index)));
			self.in_block.push(InBlock { block, number, tx_index, watcher });
		}
	}

	/// The chain finalized up to `finalized_number`; `canonical(number, hash)`
	/// tells whether a settlement block is still on the finalized chain.
	pub fn on_finalized(&mut self, finalized_number: u32, canonical: impl Fn(u32, H256) -> bool) {
		let mut still_waiting = Vec::new();
		for entry in std::mem::take(&mut self.in_block) {
			if entry.number > finalized_number {
				still_waiting.push(entry);
			} else if canonical(entry.number, entry.block) {
				entry.watcher.send(TransactionStatus::Finalized((entry.block, entry.tx_index)));
			} else {
				entry.watcher.send(TransactionStatus::Retracted(entry.block));
				self.pending.push(entry.watcher);
			}
		}
		self.in_block = still_waiting;
	}

	/// `Dropped` for pending watchers whose proof the pool expired.
	pub fn drop_expired(&mut self, max_age: Duration) {
		let now = Instant::now();
		self.drop_pending(|w| now.saturating_duration_since(w.claimed_at) > max_age);
	}

	/// `Dropped` for pending watchers whose bucket the pool removed.
	pub fn drop_bucket(&mut self, key: &BatchKey) {
		self.drop_pending(|w| w.bucket == *key);
	}

	fn drop_pending(&mut self, dropped: impl Fn(&Watcher) -> bool) {
		let (gone, pending): (Vec<_>, Vec<_>) =
			std::mem::take(&mut self.pending).into_iter().partition(|w| dropped(w));
		self.pending = pending;
		for watcher in gone {
			watcher.send(TransactionStatus::Dropped);
		}
	}

	/// Forget watchers whose subscriber went away.
	pub fn prune_closed(&mut self) {
		self.pending.retain(|w| !w.sender.is_closed());
		self.in_block.retain(|e| !e.watcher.sender.is_closed());
	}

	#[cfg(test)]
	fn len(&self) -> usize {
		self.pending.len() + self.in_block.len()
	}

	#[cfg(test)]
	fn pending_nullifiers(&self) -> std::collections::HashSet<BytesDigest> {
		self.pending.iter().flat_map(|w| w.nullifiers.iter().copied()).collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures::{FutureExt, StreamExt};
	use std::collections::HashSet;

	fn digest(byte: u8) -> BytesDigest {
		BytesDigest::new_unchecked([byte; 32])
	}

	fn key() -> BatchKey {
		BatchKey { block_hash: digest(9), asset_id: 0, volume_fee_bps: 4 }
	}

	fn drain(stream: &mut InterceptedStatusStream<Block>) -> Vec<Status> {
		let mut out = Vec::new();
		while let Some(Some(status)) = stream.next().now_or_never() {
			out.push(status);
		}
		out
	}

	#[test]
	fn settles_then_finalizes_through_the_settling_extrinsic() {
		let mut watchers = Watchers::default();
		let mut stream = watchers.watch(vec![digest(1), digest(2)], key());
		assert_eq!(drain(&mut stream), vec![TransactionStatus::Ready]);

		let block = H256::repeat_byte(0xaa);
		watchers.on_settled(block, 7, &HashMap::from([(digest(2), 3)]));
		assert_eq!(drain(&mut stream), vec![TransactionStatus::InBlock((block, 3))]);
		assert!(watchers.pending_nullifiers().is_empty());

		watchers.on_finalized(6, |_, _| true);
		assert!(drain(&mut stream).is_empty());

		watchers.on_finalized(7, |_, _| true);
		assert_eq!(drain(&mut stream), vec![TransactionStatus::Finalized((block, 3))]);
		assert_eq!(watchers.len(), 0);
	}

	#[test]
	fn retracted_settlement_goes_back_to_pending() {
		let mut watchers = Watchers::default();
		let mut stream = watchers.watch(vec![digest(1)], key());
		let block = H256::repeat_byte(0xaa);
		watchers.on_settled(block, 7, &HashMap::from([(digest(1), 0)]));
		drain(&mut stream);

		watchers.on_finalized(7, |_, _| false);
		assert_eq!(drain(&mut stream), vec![TransactionStatus::Retracted(block)]);
		assert_eq!(watchers.pending_nullifiers(), HashSet::from([digest(1)]));
	}

	#[test]
	fn unsettled_watchers_are_dropped_with_their_proof() {
		let mut watchers = Watchers::default();
		let mut expired = watchers.watch(vec![digest(1)], key());
		let other_bucket = BatchKey { asset_id: 1, ..key() };
		let mut removed = watchers.watch(vec![digest(2)], other_bucket.clone());
		let mut kept = watchers.watch(vec![digest(3)], key());
		drain(&mut expired);
		drain(&mut removed);
		drain(&mut kept);

		watchers.drop_bucket(&other_bucket);
		assert_eq!(drain(&mut removed), vec![TransactionStatus::Dropped]);

		watchers.pending[0].claimed_at = Instant::now() - Duration::from_secs(10);
		watchers.drop_expired(Duration::from_secs(5));
		assert_eq!(drain(&mut expired), vec![TransactionStatus::Dropped]);
		assert!(drain(&mut kept).is_empty());
		assert_eq!(watchers.pending_nullifiers(), HashSet::from([digest(3)]));
	}

	#[test]
	fn closed_subscribers_are_pruned() {
		let mut watchers = Watchers::default();
		let stream = watchers.watch(vec![digest(1)], key());
		let _kept = watchers.watch(vec![digest(2)], key());
		drop(stream);
		watchers.prune_closed();
		assert_eq!(watchers.pending_nullifiers(), HashSet::from([digest(2)]));
	}
}
