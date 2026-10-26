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

//! Transaction pool wrapper. Provides a type for wrapping object providing actual implementation of
//! transaction pool.

use crate::{
	builder::FullClientTransactionPool,
	graph::{base_pool::Transaction, ExtrinsicFor, ExtrinsicHash},
	interceptor::{Interception, SubmissionInterceptor},
	ChainApi, FullChainApi, ReadyIteratorFor,
};
use async_trait::async_trait;
use sc_transaction_pool_api::{
	ChainEvent, ImportNotificationStream, LocalTransactionFor, LocalTransactionPool,
	MaintainedTransactionPool, PoolStatus, ReadyTransactions, TransactionFor, TransactionPool,
	TransactionSource, TransactionStatusStreamFor, TxHash, TxInvalidityReportMap,
};
use sp_runtime::traits::Block as BlockT;
use std::{collections::HashMap, pin::Pin, sync::Arc};

/// The wrapper for actual object providing implementation of TransactionPool.
///
/// This wraps actual implementation of the TransactionPool, e.g. fork-aware or single-state.
pub struct TransactionPoolWrapper<Block, Client>
where
	Block: BlockT,
	Client: sp_api::ProvideRuntimeApi<Block>
		+ sc_client_api::BlockBackend<Block>
		+ sc_client_api::blockchain::HeaderBackend<Block>
		+ sp_runtime::traits::BlockIdTo<Block>
		+ sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
		+ 'static,
	Client::Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>,
{
	/// The actual pool implementation.
	pub inner: Box<dyn FullClientTransactionPool<Block, Client>>,
	/// Optional node-local hook consulted before admission (see [`SubmissionInterceptor`]).
	interceptor: Option<Arc<dyn SubmissionInterceptor<Block>>>,
}

impl<Block, Client> TransactionPoolWrapper<Block, Client>
where
	Block: BlockT,
	Client: sp_api::ProvideRuntimeApi<Block>
		+ sc_client_api::BlockBackend<Block>
		+ sc_client_api::blockchain::HeaderBackend<Block>
		+ sp_runtime::traits::BlockIdTo<Block>
		+ sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
		+ 'static,
	Client::Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>,
{
	/// Wrap `inner`, optionally fronting it with `interceptor`.
	pub fn new(
		inner: Box<dyn FullClientTransactionPool<Block, Client>>,
		interceptor: Option<Arc<dyn SubmissionInterceptor<Block>>>,
	) -> Self {
		Self { inner, interceptor }
	}

	async fn intercept(
		&self,
		at: Block::Hash,
		source: TransactionSource,
		xt: &Block::Extrinsic,
	) -> Interception {
		match &self.interceptor {
			Some(interceptor) => interceptor.intercept(at, source, xt).await,
			None => Interception::Pass,
		}
	}
}

#[async_trait]
impl<Block, Client> TransactionPool for TransactionPoolWrapper<Block, Client>
where
	Block: BlockT,
	Client: sp_api::ProvideRuntimeApi<Block>
		+ sc_client_api::BlockBackend<Block>
		+ sc_client_api::blockchain::HeaderBackend<Block>
		+ sp_runtime::traits::BlockIdTo<Block>
		+ sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
		+ 'static,
	Client::Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>,
{
	type Block = Block;
	type Hash = ExtrinsicHash<FullChainApi<Client, Block>>;
	type InPoolTransaction = Transaction<
		ExtrinsicHash<FullChainApi<Client, Block>>,
		ExtrinsicFor<FullChainApi<Client, Block>>,
	>;
	type Error = <FullChainApi<Client, Block> as ChainApi>::Error;

	async fn submit_at(
		&self,
		at: <Self::Block as BlockT>::Hash,
		source: TransactionSource,
		xts: Vec<TransactionFor<Self>>,
	) -> Result<Vec<Result<TxHash<Self>, Self::Error>>, Self::Error> {
		if self.interceptor.is_none() {
			return self.inner.submit_at(at, source, xts).await;
		}

		// Claimed extrinsics are answered with their hash in place; the rest go to
		// the pool as one batch and their results are spliced back in order.
		let mut slots = Vec::with_capacity(xts.len());
		let mut to_submit = Vec::new();
		for xt in xts {
			match self.intercept(at, source, &xt).await {
				Interception::Claimed => slots.push(Some(self.inner.hash_of(&xt))),
				Interception::Pass => {
					slots.push(None);
					to_submit.push(xt);
				},
			}
		}
		let mut submitted = if to_submit.is_empty() {
			Vec::new()
		} else {
			self.inner.submit_at(at, source, to_submit).await?
		}
		.into_iter();
		Ok(slots
			.into_iter()
			.map(|claimed| match claimed {
				Some(hash) => Ok(hash),
				None => submitted.next().unwrap_or_else(|| {
					Err(crate::common::error::Error::RuntimeApi(
						"transaction pool returned fewer results than submitted".into(),
					))
				}),
			})
			.collect())
	}

	async fn submit_one(
		&self,
		at: <Self::Block as BlockT>::Hash,
		source: TransactionSource,
		xt: TransactionFor<Self>,
	) -> Result<TxHash<Self>, Self::Error> {
		if let Interception::Claimed = self.intercept(at, source, &xt).await {
			return Ok(self.inner.hash_of(&xt));
		}
		self.inner.submit_one(at, source, xt).await
	}

	async fn submit_and_watch(
		&self,
		at: <Self::Block as BlockT>::Hash,
		source: TransactionSource,
		xt: TransactionFor<Self>,
	) -> Result<Pin<Box<TransactionStatusStreamFor<Self>>>, Self::Error> {
		self.inner.submit_and_watch(at, source, xt).await
	}

	async fn ready_at(
		&self,
		at: <Self::Block as BlockT>::Hash,
	) -> ReadyIteratorFor<FullChainApi<Client, Block>> {
		self.inner.ready_at(at).await
	}

	fn ready(&self) -> Box<dyn ReadyTransactions<Item = Arc<Self::InPoolTransaction>> + Send> {
		self.inner.ready()
	}

	async fn report_invalid(
		&self,
		at: Option<<Self::Block as BlockT>::Hash>,
		invalid_tx_errors: TxInvalidityReportMap<TxHash<Self>>,
	) -> Vec<Arc<Self::InPoolTransaction>> {
		self.inner.report_invalid(at, invalid_tx_errors).await
	}

	fn futures(&self) -> Vec<Self::InPoolTransaction> {
		self.inner.futures()
	}

	fn status(&self) -> PoolStatus {
		self.inner.status()
	}

	fn import_notification_stream(&self) -> ImportNotificationStream<TxHash<Self>> {
		self.inner.import_notification_stream()
	}

	fn on_broadcasted(&self, propagations: HashMap<TxHash<Self>, Vec<String>>) {
		self.inner.on_broadcasted(propagations)
	}

	fn hash_of(&self, xt: &TransactionFor<Self>) -> TxHash<Self> {
		self.inner.hash_of(xt)
	}

	fn ready_transaction(&self, hash: &TxHash<Self>) -> Option<Arc<Self::InPoolTransaction>> {
		self.inner.ready_transaction(hash)
	}

	async fn ready_at_with_timeout(
		&self,
		at: <Self::Block as BlockT>::Hash,
		timeout: std::time::Duration,
	) -> ReadyIteratorFor<FullChainApi<Client, Block>> {
		self.inner.ready_at_with_timeout(at, timeout).await
	}
}

#[async_trait]
impl<Block, Client> MaintainedTransactionPool for TransactionPoolWrapper<Block, Client>
where
	Block: BlockT,
	Client: sp_api::ProvideRuntimeApi<Block>
		+ sc_client_api::BlockBackend<Block>
		+ sc_client_api::blockchain::HeaderBackend<Block>
		+ sp_runtime::traits::BlockIdTo<Block>
		+ sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
		+ 'static,
	Client::Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>,
{
	async fn maintain(&self, event: ChainEvent<Self::Block>) {
		self.inner.maintain(event).await;
	}
}

impl<Block, Client> LocalTransactionPool for TransactionPoolWrapper<Block, Client>
where
	Block: BlockT,
	Client: sp_api::ProvideRuntimeApi<Block>
		+ sc_client_api::BlockBackend<Block>
		+ sc_client_api::blockchain::HeaderBackend<Block>
		+ sp_runtime::traits::BlockIdTo<Block>
		+ sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
		+ 'static,
	Client::Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>,
{
	type Block = Block;
	type Hash = ExtrinsicHash<FullChainApi<Client, Block>>;
	type Error = <FullChainApi<Client, Block> as ChainApi>::Error;

	fn submit_local(
		&self,
		at: <Self::Block as BlockT>::Hash,
		xt: LocalTransactionFor<Self>,
	) -> Result<Self::Hash, Self::Error> {
		self.inner.submit_local(at, xt)
	}
}
