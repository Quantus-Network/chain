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
//! The [*Background tasks*](../index.html#background-tasks) section provides some extra details on
//! revalidation process.

use std::{marker::PhantomData, pin::Pin, sync::Arc};

use crate::{graph::ChainApi, LOG_TARGET};
use sc_utils::mpsc::{tracing_unbounded, TracingUnboundedReceiver, TracingUnboundedSender};
use sp_blockchain::HashAndNumber;
use sp_runtime::traits::Block as BlockT;

use super::{tx_mem_pool::TxMemPool, view_store::ViewStore};
use futures::prelude::*;
use tracing::{debug, warn};

use super::view::{FinishRevalidationWorkerChannels, View};

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
		from_queue: TracingUnboundedReceiver<RevalidateViewPayload<Api>>,
	) {
		let mut from_queue = from_queue.fuse();

		loop {
			let Some(payload) = from_queue.next().await else {
				break;
			};
			payload.view.revalidate(payload.worker_channels).await;
		}
	}

	/// Worker loop for mempool revalidation payloads.
	async fn run_mempool<Api: ChainApi<Block = Block> + 'static>(
		self,
		from_queue: TracingUnboundedReceiver<RevalidateMempoolPayload<Api, Block>>,
	) {
		let mut from_queue = from_queue.fuse();

		loop {
			let Some(payload) = from_queue.next().await else {
				break;
			};
			payload.mempool.revalidate(payload.view_store, payload.finalized_hash).await;
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
	view_background: Option<TracingUnboundedSender<RevalidateViewPayload<Api>>>,
	mempool_background: Option<TracingUnboundedSender<RevalidateMempoolPayload<Api, Block>>>,
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
		Self { view_background: None, mempool_background: None }
	}

	/// New revalidation queue with background workers.
	///
	/// View and mempool revalidation each run on their own worker loop so that a long
	/// mempool batch cannot delay cancellation / completion of view revalidation.
	pub fn new_with_worker() -> (Self, Pin<Box<dyn Future<Output = ()> + Send>>) {
		let (to_view_worker, from_view_queue) =
			tracing_unbounded("mpsc_revalidation_view_queue", 100_000);
		let (to_mempool_worker, from_mempool_queue) =
			tracing_unbounded("mpsc_revalidation_mempool_queue", 100_000);

		let worker = async move {
			futures::future::join(
				RevalidationWorker::new().run_view(from_view_queue),
				RevalidationWorker::new().run_mempool(from_mempool_queue),
			)
			.await;
		};

		(
			Self {
				view_background: Some(to_view_worker),
				mempool_background: Some(to_mempool_worker),
			},
			worker.boxed(),
		)
	}

	/// Queue the view for later revalidation.
	///
	/// If the queue is configured with background worker, this will return immediately.
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
			if let Err(error) = to_worker.unbounded_send(RevalidateViewPayload {
				view,
				worker_channels: finish_revalidation_worker_channels,
			}) {
				warn!(
					target: LOG_TARGET,
					?error,
					"revalidation_queue::revalidate_view: Failed to update background worker"
				);
			}
		} else {
			view.revalidate(finish_revalidation_worker_channels).await
		}
	}

	/// Revalidates the given mempool instance.
	///
	/// If queue configured with background worker, this will return immediately.
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

		if let Some(ref to_worker) = self.mempool_background {
			if let Err(error) = to_worker.unbounded_send(RevalidateMempoolPayload {
				mempool,
				view_store,
				finalized_hash,
			}) {
				warn!(
					target: LOG_TARGET,
					?error,
					"Failed to update background worker"
				);
			}
		} else {
			mempool.revalidate(view_store, finalized_hash).await
		}
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

	/// `finish_revalidation` (maintain critical path) must not stall behind an uncancellable
	/// mempool revalidation batch that was enqueued earlier on the shared worker infrastructure.
	#[tokio::test]
	async fn finish_view_revalidation_not_blocked_by_mempool_revalidation() {
		let mempool_validation_delay = Duration::from_millis(500);
		let finish_budget = Duration::from_millis(100);

		let api = Arc::new(MockChainApi::with_validation_delay(mempool_validation_delay));
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
			TxMemPool::new(api.clone(), listener, Default::default(), 1024, usize::MAX);
		let mempool = Arc::new(mempool);

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
}
