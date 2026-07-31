// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! The background worker for the [`View`] and [`TxMemPool`] revalidation.
//!
//! View and mempool revalidation run on **separate** worker loops so that
//! [`View::finish_revalidation`] (awaited on the maintain critical path) cannot
//! stall behind an uncancellable [`TxMemPool::revalidate`] batch.
//!
//! Queues are capacity-limited:
//! - view jobs use a bounded channel (inline revalidation when full)
//! - mempool jobs use a latest-wins slot so finalization bursts cannot accumulate
//!   unbounded `RevalidateMempool` payloads
//!
//! The [*Background tasks*](../index.html#background-tasks) section provides some extra details on
//! revalidation process.

use std::{marker::PhantomData, pin::Pin, sync::Arc};

use crate::{graph::ChainApi, LOG_TARGET};
use futures::{
	channel::mpsc::{channel, Receiver, Sender},
	prelude::*,
};
use parking_lot::Mutex;
use sp_blockchain::HashAndNumber;
use sp_runtime::traits::Block as BlockT;
use tracing::{debug, warn};

use super::{
	tx_mem_pool::TxMemPool,
	view::{FinishRevalidationWorkerChannels, View},
	view_store::ViewStore,
};

/// Bound for the view-revalidation job channel.
///
/// Maintain enqueues at most one job per active view tip per cycle and waits on
/// completion, so a modest bound is enough. When full, [`RevalidationQueue::revalidate_view`]
/// falls back to inline revalidation so the finish protocol cannot hang.
const VIEW_REVALIDATION_QUEUE_CAPACITY: usize = 64;

/// Request to revalidate a [`View`].
///
/// Communication channels with the maintain thread are also provided.
struct RevalidateViewPayload<Api>
where
	Api: ChainApi + 'static,
{
	view: Arc<View<Api>>,
	worker_channels: FinishRevalidationWorkerChannels<Api>,
}

/// Request to revalidate a [`TxMemPool`] at the provided block hash.
struct RevalidateMempoolPayload<Api, Block>
where
	Block: BlockT,
	Api: ChainApi<Block = Block> + 'static,
{
	mempool: Arc<TxMemPool<Api, Block>>,
	view_store: Arc<ViewStore<Api, Block>>,
	finalized_hash: HashAndNumber<Block>,
}

/// Latest-wins mempool revalidation slot.
///
/// Producers overwrite `pending` and wake the worker. Only one payload is retained
/// while the worker is busy, so finalization bursts cannot grow a queue of
/// `Arc<TxMemPool>` / `Arc<ViewStore>` jobs.
struct MempoolPendingSlot<Api, Block>
where
	Block: BlockT,
	Api: ChainApi<Block = Block> + 'static,
{
	pending: Mutex<Option<RevalidateMempoolPayload<Api, Block>>>,
}

/// The background revalidation worker.
struct RevalidationWorker<Block: BlockT> {
	_phantom: PhantomData<Block>,
}

impl<Block> RevalidationWorker<Block>
where
	Block: BlockT,
	<Block as BlockT>::Hash: Unpin,
{
	/// Create a new instance of the background worker.
	fn new() -> Self {
		Self { _phantom: Default::default() }
	}

	/// Worker loop for view revalidation payloads.
	async fn run_view<Api: ChainApi<Block = Block> + 'static>(
		self,
		from_queue: Receiver<RevalidateViewPayload<Api>>,
	) {
		let mut from_queue = from_queue.fuse();

		loop {
			let Some(payload) = from_queue.next().await else {
				break;
			};
			payload.view.revalidate(payload.worker_channels).await;
		}
	}

	/// Worker loop for mempool revalidation: always process the latest pending job.
	async fn run_mempool<Api: ChainApi<Block = Block> + 'static>(
		self,
		slot: Arc<MempoolPendingSlot<Api, Block>>,
		wake_rx: Receiver<()>,
	) {
		let mut wake_rx = wake_rx.fuse();

		loop {
			let Some(()) = wake_rx.next().await else {
				break;
			};
			// Drain whatever is pending; producers may overwrite while we work, so
			// loop until the slot stays empty after a job completes.
			loop {
				let payload = { slot.pending.lock().take() };
				let Some(payload) = payload else {
					break;
				};
				payload.mempool.revalidate(payload.view_store, payload.finalized_hash).await;
			}
		}
	}
}

/// A Revalidation queue.
///
/// Allows to send the revalidation requests to the background workers.
///
/// View and mempool jobs use independent channels so maintain's
/// [`View::finish_revalidation`] wait is not coupled to mempool batch work.
pub struct RevalidationQueue<Api, Block>
where
	Api: ChainApi<Block = Block> + 'static,
	Block: BlockT,
{
	view_background: Option<Mutex<Sender<RevalidateViewPayload<Api>>>>,
	mempool_pending: Option<Arc<MempoolPendingSlot<Api, Block>>>,
	/// Capacity-1 wake signal owned by the queue; drop shuts the mempool worker down.
	mempool_wake: Option<Mutex<Sender<()>>>,
}

impl<Api, Block> RevalidationQueue<Api, Block>
where
	Api: ChainApi<Block = Block> + 'static,
	Block: BlockT,
	<Block as BlockT>::Hash: Unpin,
{
	/// New revalidation queue without background worker.
	///
	/// All validation requests will be blocking.
	pub fn new() -> Self {
		Self { view_background: None, mempool_pending: None, mempool_wake: None }
	}

	/// New revalidation queue with background workers.
	///
	/// View and mempool revalidation each run on their own worker loop so that a long
	/// mempool batch cannot delay cancellation / completion of view revalidation.
	///
	/// Mempool jobs coalesce to a latest-wins slot (no unbounded FIFO backlog).
	pub fn new_with_worker() -> (Self, Pin<Box<dyn Future<Output = ()> + Send>>) {
		let (to_view_worker, from_view_queue) = channel(VIEW_REVALIDATION_QUEUE_CAPACITY);
		let (wake_tx, wake_rx) = channel(1);
		let mempool_slot = Arc::new(MempoolPendingSlot { pending: Mutex::new(None) });
		let mempool_slot_worker = mempool_slot.clone();

		let worker = async move {
			futures::future::join(
				RevalidationWorker::new().run_view(from_view_queue),
				RevalidationWorker::new().run_mempool(mempool_slot_worker, wake_rx),
			)
			.await;
		};

		(
			Self {
				view_background: Some(Mutex::new(to_view_worker)),
				mempool_pending: Some(mempool_slot),
				mempool_wake: Some(Mutex::new(wake_tx)),
			},
			worker.boxed(),
		)
	}

	/// Queue the view for later revalidation.
	///
	/// If the queue is configured with background worker, this will return immediately
	/// unless the bounded queue is full, in which case revalidation runs inline so
	/// maintain's finish protocol cannot hang.
	/// If the queue is configured without background worker, this will resolve after
	/// revalidation is actually done.
	///
	/// Schedules execution of the [`View::revalidate`].
	pub async fn revalidate_view(
		&self,
		view: Arc<View<Api>>,
		finish_revalidation_worker_channels: FinishRevalidationWorkerChannels<Api>,
	) {
		debug!(
			target: LOG_TARGET,
			view_at_hash = ?view.at.hash,
			"revalidation_queue::revalidate_view: Sending view to revalidation queue"
		);

		if let Some(ref to_worker) = self.view_background {
			let payload = RevalidateViewPayload {
				view: view.clone(),
				worker_channels: finish_revalidation_worker_channels,
			};
			// Drop the lock before any `.await` (parking_lot guards are not `Send`).
			let send_result = { to_worker.lock().try_send(payload) };
			match send_result {
				Ok(()) => {},
				Err(error) => {
					let is_full = error.is_full();
					let payload = error.into_inner();
					if is_full {
						warn!(
							target: LOG_TARGET,
							view_at_hash = ?view.at.hash,
							"revalidation_queue::revalidate_view: queue full, running inline"
						);
					} else {
						warn!(
							target: LOG_TARGET,
							view_at_hash = ?view.at.hash,
							"revalidation_queue::revalidate_view: worker gone, running inline"
						);
					}
					payload.view.revalidate(payload.worker_channels).await;
				},
			}
		} else {
			view.revalidate(finish_revalidation_worker_channels).await
		}
	}

	/// Revalidates the given mempool instance.
	///
	/// If queue configured with background worker, this schedules a latest-wins job
	/// (overwriting any not-yet-started mempool revalidation) and returns immediately.
	/// If queue configured without background worker, this will resolve after
	/// revalidation is actually done.
	///
	/// Schedules execution of the [`TxMemPool::revalidate`].
	pub async fn revalidate_mempool(
		&self,
		mempool: Arc<TxMemPool<Api, Block>>,
		view_store: Arc<ViewStore<Api, Block>>,
		finalized_hash: HashAndNumber<Block>,
	) {
		debug!(
			target: LOG_TARGET,
			?finalized_hash,
			"Sent mempool to revalidation queue"
		);

		if let (Some(slot), Some(wake)) = (&self.mempool_pending, &self.mempool_wake) {
			*slot.pending.lock() = Some(RevalidateMempoolPayload {
				mempool,
				view_store,
				finalized_hash,
			});
			// Capacity-1 wake: ignore full (worker already notified).
			let wake_result = { wake.lock().try_send(()) };
			if let Err(error) = wake_result {
				if error.is_disconnected() {
					warn!(
						target: LOG_TARGET,
						"mempool revalidation worker gone; dropping job"
					);
				}
			}
		} else {
			mempool.revalidate(view_store, finalized_hash).await
		}
	}

	/// Number of mempool revalidation jobs currently waiting in the latest-wins slot.
	///
	/// Test-only helper (0 or 1).
	#[cfg(test)]
	pub(super) fn mempool_pending_jobs(&self) -> usize {
		self.mempool_pending
			.as_ref()
			.map(|slot| usize::from(slot.pending.lock().is_some()))
			.unwrap_or(0)
	}
}

#[cfg(all(test, feature = "test-helpers"))]
//todo: add more tests [#5480]
mod tests {
	use super::*;
	use crate::{
		common::tests::{uxt, TestApi},
		fork_aware_txpool::view::FinishRevalidationLocalChannels,
		TimedTransactionSource, ValidateTransactionPriority,
	};
	use futures::executor::block_on;
	use substrate_test_runtime::{AccountId, Transfer, H256};
	use substrate_test_runtime_client::Sr25519Keyring::Alice;
	#[test]
	fn revalidation_queue_works() {
		let api = Arc::new(TestApi::default());
		let block0 = api.expect_hash_and_number(0);

		let view = Arc::new(
			View::new(api.clone(), block0, Default::default(), Default::default(), false.into()).0,
		);
		let queue = Arc::new(RevalidationQueue::new());

		let uxt = uxt(Transfer {
			from: Alice.into(),
			to: AccountId::from_h256(H256::from_low_u64_be(2)),
			amount: 5,
			nonce: 0,
		});

		let _ = block_on(view.submit_many(
			std::iter::once((TimedTransactionSource::new_external(false), uxt.clone().into())),
			ValidateTransactionPriority::Submitted,
		));
		assert_eq!(api.validation_requests().len(), 1);

		let (finish_revalidation_request_tx, finish_revalidation_request_rx) =
			tokio::sync::mpsc::channel(1);
		let (revalidation_result_tx, revalidation_result_rx) = tokio::sync::mpsc::channel(1);

		let finish_revalidation_worker_channels = FinishRevalidationWorkerChannels::new(
			finish_revalidation_request_rx,
			revalidation_result_tx,
		);

		let _finish_revalidation_local_channels = FinishRevalidationLocalChannels::new(
			finish_revalidation_request_tx,
			revalidation_result_rx,
		);

		block_on(queue.revalidate_view(view.clone(), finish_revalidation_worker_channels));

		assert_eq!(api.validation_requests().len(), 2);
		// number of ready
		assert_eq!(view.status().ready, 1);
	}
}

#[cfg(test)]
mod concurrency_tests {
	use super::super::{
		dropped_watcher::MultiViewDroppedWatcherController,
		import_notification_sink::MultiViewImportNotificationSink,
		multi_view_listener::MultiViewListener,
		tx_mem_pool::{TxMemPool, TXMEMPOOL_REVALIDATION_PERIOD},
		view::View,
		view_store::ViewStore,
	};
	use super::*;
	use crate::common::mock_api::{xt, MockChainApi, TestBlock};
	use sp_core::H256;
	use sp_runtime::transaction_validity::TransactionSource;
	use std::time::Duration;

	fn setup_mempool_and_view_store(
		api: Arc<MockChainApi>,
	) -> (
		Arc<TxMemPool<MockChainApi, TestBlock>>,
		Arc<ViewStore<MockChainApi, TestBlock>>,
		impl Future<Output = ()> + Send,
		impl Future<Output = ()> + Send,
	) {
		let (listener, listener_task) = MultiViewListener::new_with_worker(Default::default());
		let listener = Arc::new(listener);
		let (import_notification_sink, import_notification_sink_task) =
			MultiViewImportNotificationSink::new_with_worker();
		let (dropped_stream_controller, _dropped_stream) =
			MultiViewDroppedWatcherController::<MockChainApi>::new();

		let view_store = Arc::new(ViewStore::new(
			api.clone(),
			listener.clone(),
			dropped_stream_controller,
			import_notification_sink,
		));
		// Only async mempool APIs are used below; the sync-bridge task is unused.
		let (mempool, _mempool_task) =
			TxMemPool::new(api, listener, Default::default(), 1024, usize::MAX);
		let mempool = Arc::new(mempool);

		(mempool, view_store, listener_task, import_notification_sink_task)
	}

	/// `finish_revalidation` (maintain critical path) must not stall behind an uncancellable
	/// mempool revalidation batch that was enqueued earlier on the shared worker infrastructure.
	#[tokio::test]
	async fn finish_view_revalidation_not_blocked_by_mempool_revalidation() {
		let mempool_validation_delay = Duration::from_millis(500);
		let finish_budget = Duration::from_millis(100);

		let api = Arc::new(MockChainApi::with_validation_delay(mempool_validation_delay));
		let (mempool, view_store, listener_task, import_notification_sink_task) =
			setup_mempool_and_view_store(api.clone());

		let (queue, worker_task) = RevalidationQueue::<MockChainApi, TestBlock>::new_with_worker();
		let queue = Arc::new(queue);

		tokio::spawn(listener_task);
		tokio::spawn(import_notification_sink_task);
		tokio::spawn(worker_task);

		// Mempool txs are due for revalidation at finalized height > PERIOD.
		let xts = vec![Arc::from(xt(1)), Arc::from(xt(2))];
		let _ = mempool.extend_unwatched(TransactionSource::External, 0, &xts).await;

		let finalized = HashAndNumber {
			hash: H256::from_low_u64_be(TXMEMPOOL_REVALIDATION_PERIOD + 1),
			number: TXMEMPOOL_REVALIDATION_PERIOD + 1,
		};
		queue
			.revalidate_mempool(mempool.clone(), view_store.clone(), finalized)
			.await;

		// Ensure the mempool job is dequeued before the view job is enqueued.
		tokio::time::sleep(Duration::from_millis(20)).await;

		let view_at = HashAndNumber { hash: H256::from_low_u64_be(1), number: 1 };
		let view = Arc::new(
			View::new(api, view_at, Default::default(), Default::default(), false.into()).0,
		);
		View::start_background_revalidation(view.clone(), queue).await;

		let finish = tokio::time::timeout(finish_budget, view.finish_revalidation()).await;
		assert!(
			finish.is_ok(),
			"finish_revalidation stalled behind mempool revalidation \
			 (budget {:?}, mempool validation delay {:?})",
			finish_budget,
			mempool_validation_delay
		);
	}

	/// Finalization bursts must not enqueue unbounded mempool revalidation work: the slot
	/// holds at most one pending job, and only the latest finalized tip is applied.
	#[tokio::test]
	async fn mempool_revalidation_latest_wins_under_finalization_burst() {
		let api = Arc::new(MockChainApi::with_validation_delay(Duration::from_millis(50)));
		let (mempool, view_store, listener_task, import_notification_sink_task) =
			setup_mempool_and_view_store(api.clone());

		let (queue, worker_task) = RevalidationQueue::<MockChainApi, TestBlock>::new_with_worker();

		tokio::spawn(listener_task);
		tokio::spawn(import_notification_sink_task);

		let xts = vec![Arc::from(xt(1)), Arc::from(xt(2))];
		let _ = mempool.extend_unwatched(TransactionSource::External, 0, &xts).await;

		const BURST: u64 = 40;
		// Enqueue before the worker runs so a FIFO would retain all jobs.
		for i in 1..=BURST {
			let number = TXMEMPOOL_REVALIDATION_PERIOD + i;
			queue
				.revalidate_mempool(
					mempool.clone(),
					view_store.clone(),
					HashAndNumber { hash: H256::from_low_u64_be(number), number },
				)
				.await;
			assert_eq!(
				queue.mempool_pending_jobs(),
				1,
				"pending mempool revalidation jobs must stay latest-wins (at most one)"
			);
		}

		tokio::spawn(worker_task);

		tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				if queue.mempool_pending_jobs() == 0 && api.validation_count() > 0 {
					break;
				}
				tokio::time::sleep(Duration::from_millis(20)).await;
			}
		})
		.await
		.expect("mempool revalidation did not complete");

		// All BURST enqueues coalesced into a single job before the worker started, so only
		// one batch runs (one validation per mempool tx).
		assert_eq!(
			api.validation_count(),
			xts.len(),
			"expected a single coalesced revalidation batch, got {} validations for burst of {}",
			api.validation_count(),
			BURST
		);
	}
}
