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
use parking_lot::Mutex;
use sc_transaction_pool_api::error::Error as TxPoolApiError;
use sp_blockchain::{HashAndNumber, TreeRoute};
use sp_core::H256;
use sp_runtime::{
	generic::BlockId,
	testing::MockCallU64,
	traits::{BlakeTwo256, Block as BlockT, Hash as _, Header as _},
	transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
	},
};
use std::{
	collections::{HashMap, HashSet},
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};

/// Extrinsic type used with [`MockChainApi`].
pub(crate) type Extrinsic = sp_runtime::testing::TestXt<MockCallU64, ()>;

/// Block type used with [`MockChainApi`].
pub(crate) type TestBlock = sp_runtime::testing::Block<Extrinsic>;

type Header = <TestBlock as BlockT>::Header;

/// Transactions with a call value at or above this threshold are reported as invalid by
/// [`MockChainApi`].
pub(crate) const INVALID_CALL_THRESHOLD: u64 = 1000;

/// Creates a test extrinsic with the given call value.
pub(crate) fn xt(value: u64) -> Extrinsic {
	Extrinsic::new_bare(MockCallU64(value))
}

/// Creates a distinct extrinsic for the same call value, so two of them share a `provides` tag.
pub(crate) fn signed_xt(value: u64, signer: u64) -> Extrinsic {
	Extrinsic::new_signed(MockCallU64(value), signer, (), ())
}

/// Minimal `ChainApi` mock: treats every block id as existing and validates transactions
/// based on their call value only.
#[derive(Default)]
pub(crate) struct MockChainApi {
	/// Number of performed `validate_transaction` calls.
	validation_count: AtomicUsize,
	/// Optional artificial delay applied inside `validate_transaction`.
	validation_delay: Option<Duration>,
	/// Transactions that fail validation as `Stale`.
	invalidated: Mutex<HashSet<H256>>,
	/// Known blocks: hash to parent hash and body.
	blocks: Mutex<HashMap<H256, (H256, Vec<Extrinsic>)>>,
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

	/// Makes every later validation of the transaction fail as `Stale`.
	pub(crate) fn invalidate(&self, hash: H256) {
		self.invalidated.lock().insert(hash);
	}

	/// Registers a block so `block_body` and `block_header` know it.
	pub(crate) fn add_block(&self, block: H256, parent: H256, body: Vec<Extrinsic>) {
		self.blocks.lock().insert(block, (parent, body));
	}

	/// Registers the fork `G - R1[txs[0]] - R2[txs[1]] - R3[txs[2]] - R4[txs[3]]` and the empty
	/// competing fork `G - E1 - E2 - E3 - E4`. Returns the retracted transactions in execution
	/// order together with the tips `R4` and `E4`.
	pub(crate) fn interleaved_reorg(&self) -> ([Extrinsic; 4], H256, H256) {
		let txs = [xt(1), xt(2), xt(3), xt(4)];
		let add_fork = |numbers: std::ops::RangeInclusive<u64>, bodies: Vec<Vec<Extrinsic>>| {
			numbers.map(H256::from_low_u64_be).zip(bodies).fold(
				H256::from_low_u64_be(0),
				|parent, (block, body)| {
					self.add_block(block, parent, body);
					block
				},
			)
		};
		let old_tip = add_fork(1..=4, txs.iter().map(|tx| vec![tx.clone()]).collect());
		let new_tip = add_fork(11..=14, vec![vec![]; 4]);
		(txs, old_tip, new_tip)
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
		if self.invalidated.lock().contains(&self.hash_and_length(&uxt).0) {
			return Ok(Err(InvalidTransaction::Stale.into()))
		}
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
		at: <Self::Block as BlockT>::Hash,
	) -> Result<Option<Vec<<Self::Block as BlockT>::Extrinsic>>, Self::Error> {
		Ok(self.blocks.lock().get(&at).map(|(_, body)| body.clone()))
	}

	fn block_header(
		&self,
		at: <Self::Block as BlockT>::Hash,
	) -> Result<Option<<Self::Block as BlockT>::Header>, Self::Error> {
		Ok(self.blocks.lock().get(&at).map(|(parent, _)| {
			Header::new(
				at.to_low_u64_be(),
				Default::default(),
				Default::default(),
				*parent,
				Default::default(),
			)
		}))
	}

	fn tree_route(
		&self,
		from: <Self::Block as BlockT>::Hash,
		to: <Self::Block as BlockT>::Hash,
	) -> Result<TreeRoute<Self::Block>, Self::Error> {
		let blocks = self.blocks.lock();
		let ancestry = |mut hash: H256| {
			let mut path = vec![hash];
			while let Some((parent, _)) = blocks.get(&hash) {
				hash = *parent;
				path.push(hash);
			}
			path
		};
		let (from_path, to_path) = (ancestry(from), ancestry(to));
		let pivot = from_path.iter().position(|hash| to_path.contains(hash)).ok_or_else(|| {
			TxPoolApiError::InvalidBlockId(format!("{from:?} and {to:?} have no common ancestor"))
		})?;
		let enacted = to_path.iter().take_while(|hash| **hash != from_path[pivot]).count();
		let route = from_path[..=pivot]
			.iter()
			.chain(to_path[..enacted].iter().rev())
			.map(|hash| HashAndNumber { hash: *hash, number: hash.to_low_u64_be() })
			.collect();
		TreeRoute::new(route, pivot).map_err(TxPoolApiError::InvalidBlockId)
	}
}
