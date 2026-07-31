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

//! Minimal mock of [`ChainApi`] for unit tests that cannot rely on the substrate test
//! runtime (not available in this vendored crate).

use crate::{
	graph::{self, ExtrinsicFor, ExtrinsicHash, RawExtrinsicFor},
	ValidateTransactionPriority,
};
use async_trait::async_trait;
use codec::Encode;
use sc_transaction_pool_api::error::Error as TxPoolApiError;
use sp_blockchain::TreeRoute;
use sp_core::H256;
use sp_runtime::{
	generic::BlockId,
	traits::{BlakeTwo256, Block as BlockT, Hash as _},
	transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
	},
};
use std::{
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};

/// Extrinsic type used with [`MockChainApi`].
pub(crate) type Extrinsic = sp_runtime::testing::TestXt<sp_runtime::testing::MockCallU64, ()>;

/// Block type used with [`MockChainApi`].
pub(crate) type TestBlock = sp_runtime::testing::Block<Extrinsic>;

/// Transactions with a call value at or above this threshold are reported as invalid by
/// [`MockChainApi`].
pub(crate) const INVALID_CALL_THRESHOLD: u64 = 1000;

/// Creates a test extrinsic with the given call value.
pub(crate) fn xt(value: u64) -> Extrinsic {
	Extrinsic::new_bare(sp_runtime::testing::MockCallU64(value))
}

/// Minimal `ChainApi` mock: treats every block id as existing and validates transactions
/// based on their call value only.
#[derive(Default)]
pub(crate) struct MockChainApi {
	/// Number of performed `validate_transaction` calls.
	validation_count: AtomicUsize,
	/// Optional artificial delay applied inside `validate_transaction`.
	validation_delay: Option<Duration>,
}

impl MockChainApi {
	/// Creates a mock that sleeps for `delay` on every `validate_transaction` call.
	pub(crate) fn with_validation_delay(delay: Duration) -> Self {
		Self { validation_delay: Some(delay), ..Default::default() }
	}

	/// Returns the number of performed `validate_transaction` calls.
	pub(crate) fn validation_count(&self) -> usize {
		self.validation_count.load(Ordering::Relaxed)
	}
}

#[async_trait]
impl graph::ChainApi for MockChainApi {
	type Block = TestBlock;
	type Error = TxPoolApiError;

	async fn validate_transaction(
		&self,
		_at: <Self::Block as BlockT>::Hash,
		_source: TransactionSource,
		uxt: ExtrinsicFor<Self>,
		_priority: ValidateTransactionPriority,
	) -> Result<TransactionValidity, Self::Error> {
		if let Some(delay) = self.validation_delay {
			tokio::time::sleep(delay).await;
		}
		self.validation_count.fetch_add(1, Ordering::Relaxed);
		let value = uxt.function.0;
		Ok(if value >= INVALID_CALL_THRESHOLD {
			Err(InvalidTransaction::Custom(0).into())
		} else {
			Ok(ValidTransaction {
				priority: 4,
				requires: vec![],
				provides: vec![value.encode()],
				longevity: 64,
				propagate: true,
			})
		})
	}

	fn validate_transaction_blocking(
		&self,
		_at: <Self::Block as BlockT>::Hash,
		_source: TransactionSource,
		_uxt: ExtrinsicFor<Self>,
	) -> Result<TransactionValidity, Self::Error> {
		unimplemented!()
	}

	fn block_id_to_number(
		&self,
		at: &BlockId<Self::Block>,
	) -> Result<Option<graph::NumberFor<Self>>, Self::Error> {
		Ok(match at {
			BlockId::Number(num) => Some(*num),
			BlockId::Hash(hash) => Some(hash.to_low_u64_be()),
		})
	}

	fn block_id_to_hash(
		&self,
		at: &BlockId<Self::Block>,
	) -> Result<Option<<Self::Block as BlockT>::Hash>, Self::Error> {
		Ok(match at {
			BlockId::Number(num) => Some(H256::from_low_u64_be(*num)),
			BlockId::Hash(hash) => Some(*hash),
		})
	}

	fn hash_and_length(&self, uxt: &RawExtrinsicFor<Self>) -> (ExtrinsicHash<Self>, usize) {
		let encoded = uxt.encode();
		(BlakeTwo256::hash(&encoded), encoded.len())
	}

	async fn block_body(
		&self,
		_at: <Self::Block as BlockT>::Hash,
	) -> Result<Option<Vec<<Self::Block as BlockT>::Extrinsic>>, Self::Error> {
		Ok(None)
	}

	fn block_header(
		&self,
		_at: <Self::Block as BlockT>::Hash,
	) -> Result<Option<<Self::Block as BlockT>::Header>, Self::Error> {
		Ok(None)
	}

	fn tree_route(
		&self,
		_from: <Self::Block as BlockT>::Hash,
		_to: <Self::Block as BlockT>::Hash,
	) -> Result<TreeRoute<Self::Block>, Self::Error> {
		unimplemented!()
	}
}
