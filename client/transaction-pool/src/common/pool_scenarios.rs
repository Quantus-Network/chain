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

//! End-to-end scenarios shared by the single-state and fork-aware pool tests.

use super::mock_api::{signed_xt, xt, Extrinsic, MockChainApi, TestBlock};
use crate::fork_aware_txpool::TXMEMPOOL_REVALIDATION_PERIOD;
use futures::{Stream, StreamExt};
use sc_transaction_pool_api::{
	ChainEvent, InPoolTransaction, MaintainedTransactionPool, TransactionSource, TransactionStatus,
	TransactionStatusStreamFor,
};
use sp_core::H256;
use std::{pin::Pin, sync::Arc, time::Duration};

/// A pool implementation driven by [`MockChainApi`].
pub(crate) trait TestPool:
	MaintainedTransactionPool<Block = TestBlock, Hash = H256>
{
}

impl<P: MaintainedTransactionPool<Block = TestBlock, Hash = H256>> TestPool for P {}

type Watcher<P> = Pin<Box<TransactionStatusStreamFor<P>>>;

type Status = TransactionStatus<H256, H256>;

const GENESIS: H256 = H256::zero();

fn new_best_block(hash: H256) -> ChainEvent<TestBlock> {
	ChainEvent::NewBestBlock { hash, tree_route: None }
}

fn finalized(hash: H256) -> ChainEvent<TestBlock> {
	ChainEvent::Finalized { hash, tree_route: Arc::from(vec![]) }
}

async fn next_event(watcher: &mut (impl Stream<Item = Status> + Unpin)) -> Option<Status> {
	tokio::time::timeout(Duration::from_secs(10), watcher.next())
		.await
		.expect("watcher must emit an event")
}

async fn remaining_events(watcher: impl Stream<Item = Status>) -> Vec<Status> {
	tokio::time::timeout(Duration::from_secs(10), watcher.collect())
		.await
		.expect("watcher must terminate")
}

/// Submits a watched transaction at genesis and consumes its `Ready` event.
async fn watch<P: TestPool>(pool: &P, xt: Extrinsic) -> Watcher<P> {
	let mut watcher = pool
		.submit_and_watch(GENESIS, TransactionSource::External, xt)
		.await
		.expect("submit_and_watch succeeds");
	assert_eq!(next_event(&mut watcher).await, Some(TransactionStatus::Ready));
	watcher
}

/// A watched transaction pruned by a block holding a different transaction with the same tag was
/// never included: it must not be reported `InBlock` and ends with `Invalid` instead of staying at
/// `Ready`. The finalized block lies beyond the mempool revalidation period so the fork-aware pool
/// revalidates its mempool.
pub(crate) async fn replaced_transaction_ends_invalid<P: TestPool>(pool: &P, api: &MockChainApi) {
	let block = H256::from_low_u64_be(1);
	let finalized_block = H256::from_low_u64_be(TXMEMPOOL_REVALIDATION_PERIOD + 2);
	let replaced = signed_xt(1, 1);
	api.add_block(block, GENESIS, vec![signed_xt(1, 2)]);
	api.add_block(finalized_block, block, vec![]);

	let watcher = watch(pool, replaced.clone()).await;
	api.invalidate(pool.hash_of(&replaced));

	pool.maintain(new_best_block(block)).await;
	pool.maintain(finalized(finalized_block)).await;

	assert_eq!(remaining_events(watcher).await, vec![TransactionStatus::Invalid]);
}

/// Transactions from retracted blocks are resubmitted in execution order (common ancestor towards
/// the old tip), so equal-priority transactions keep their on-chain order.
pub(crate) async fn retracted_transactions_keep_execution_order<P: TestPool>(
	pool: &P,
	api: &MockChainApi,
) {
	let (txs, old_tip, new_tip) = api.interleaved_reorg();

	pool.maintain(new_best_block(old_tip)).await;
	pool.maintain(new_best_block(new_tip)).await;

	let ready: Vec<_> = pool.ready_at(new_tip).await.map(|tx| *tx.hash()).collect();
	assert_eq!(ready, txs.iter().map(|tx| pool.hash_of(tx)).collect::<Vec<_>>());
}

/// Transactions included in a block report their position in the block body with both `InBlock`
/// and `Finalized`.
pub(crate) async fn block_transactions_report_body_positions<P: TestPool>(
	pool: &P,
	api: &MockChainApi,
) {
	let block = H256::from_low_u64_be(1);
	let txs: Vec<_> = (1..=4).map(xt).collect();
	api.add_block(block, GENESIS, txs.clone());
	let mut watchers = Vec::new();
	for tx in txs {
		watchers.push(watch(pool, tx).await);
	}

	pool.maintain(new_best_block(block)).await;
	for (index, watcher) in watchers.iter_mut().enumerate() {
		assert_eq!(next_event(watcher).await, Some(TransactionStatus::InBlock((block, index))));
	}

	pool.maintain(finalized(block)).await;
	for (index, watcher) in watchers.into_iter().enumerate() {
		assert_eq!(
			remaining_events(watcher).await,
			vec![TransactionStatus::Finalized((block, index))]
		);
	}
}
