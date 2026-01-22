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

// use client directly; QPowAlgorithm removed
use crate::LOG_TARGET;
use futures::{
	prelude::*,
	task::{Context, Poll},
};
use futures_timer::Delay;
use log::*;
use parking_lot::Mutex;
use primitive_types::{H256, U512};
use sc_client_api::ImportNotifications;
use sc_consensus::{BlockImportParams, BoxBlockImport, StateAction, StorageChanges};
use sp_api::ProvideRuntimeApi;
use sp_consensus::{BlockOrigin, Proposal};
use sp_consensus_pow::{Seal, POW_ENGINE_ID};
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	AccountId32, DigestItem,
};
use std::time::Instant;
use std::{
	pin::Pin,
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	},
	time::Duration,
};

/// Mining metadata. This is the information needed to start an actual mining loop.
#[derive(Clone, Eq, PartialEq)]
pub struct MiningMetadata<H, D> {
	/// Currently known best hash which the pre-hash is built on.
	pub best_hash: H,
	/// Mining pre-hash.
	pub pre_hash: H,
	/// Rewards address.
	pub rewards_address: AccountId32,
	/// Mining target difficulty.
	pub difficulty: D,
}

/// A build of mining, containing the metadata and the block proposal.
pub struct MiningBuild<Block: BlockT, Proof> {
	/// Mining metadata.
	pub metadata: MiningMetadata<Block::Hash, U512>,
	/// Mining proposal.
	pub proposal: Proposal<Block, Proof>,
}

/// Version of the mining worker.
#[derive(Eq, PartialEq, Clone, Copy)]
pub struct Version(usize);

/// Mining worker that exposes structs to query the current mining build and submit mined blocks.
pub struct MiningHandle<Block: BlockT, AC, L: sc_consensus::JustificationSyncLink<Block>, Proof> {
	version: Arc<AtomicUsize>,
	client: Arc<AC>,
	justification_sync_link: Arc<L>,
	build: Arc<Mutex<Option<MiningBuild<Block, Proof>>>>,
	block_import: Arc<Mutex<BoxBlockImport<Block>>>,
}

impl<Block, AC, L, Proof> MiningHandle<Block, AC, L, Proof>
where
	Block: BlockT<Hash = H256>,
	AC: ProvideRuntimeApi<Block>,
	L: sc_consensus::JustificationSyncLink<Block>,
{
	fn increment_version(&self) {
		self.version.fetch_add(1, Ordering::SeqCst);
	}

	pub(crate) fn new(
		client: Arc<AC>,
		block_import: BoxBlockImport<Block>,
		justification_sync_link: L,
	) -> Self {
		Self {
			version: Arc::new(AtomicUsize::new(0)),
			client,
			justification_sync_link: Arc::new(justification_sync_link),
			build: Arc::new(Mutex::new(None)),
			block_import: Arc::new(Mutex::new(block_import)),
		}
	}

	pub(crate) fn on_major_syncing(&self) {
		let mut build = self.build.lock();
		*build = None;
		self.increment_version();
	}

	pub(crate) fn on_build(&self, value: MiningBuild<Block, Proof>) {
		let mut build = self.build.lock();
		*build = Some(value);
		self.increment_version();
	}

	/// Get the version of the mining worker.
	///
	/// This returns type `Version` which can only compare equality. If `Version` is unchanged, then
	/// it can be certain that `best_hash` and `metadata` were not changed.
	pub fn version(&self) -> Version {
		Version(self.version.load(Ordering::SeqCst))
	}

	/// Get the current best hash. `None` if the worker has just started or the client is doing
	/// major syncing.
	pub fn best_hash(&self) -> Option<Block::Hash> {
		self.build.lock().as_ref().map(|b| b.metadata.best_hash)
	}

	/// Get a copy of the current mining metadata, if available.
	pub fn metadata(&self) -> Option<MiningMetadata<Block::Hash, U512>> {
		self.build.lock().as_ref().map(|b| b.metadata.clone())
	}

	/// Submit a mined seal. The seal will be validated again. Returns true if the submission is
	/// successful.
	#[allow(clippy::await_holding_lock)]
	pub async fn submit(&self, seal: Seal) -> bool {
		let build = if let Some(build) = {
			let mut build = self.build.lock();
			let value = build.take();
			if value.is_some() {
				self.increment_version();
			}
			value
		} {
			build
		} else {
			warn!(target: LOG_TARGET, "Unable to import mined block: build does not exist",);
			return false;
		};

		let seal = DigestItem::Seal(POW_ENGINE_ID, seal);
		let (header, body) = build.proposal.block.deconstruct();

		let mut import_block = BlockImportParams::new(BlockOrigin::Own, header);
		import_block.post_digests.push(seal);
		import_block.body = Some(body);
		import_block.state_action =
			StateAction::ApplyChanges(StorageChanges::Changes(build.proposal.storage_changes));

		let header = import_block.post_header();
		let import_result = self.block_import.lock().import_block(import_block).await;

		match import_result {
			Ok(res) => {
				res.handle_justification(
					&header.hash(),
					*header.number(),
					&self.justification_sync_link,
				);

				true
			},
			Err(err) => {
				warn!(target: LOG_TARGET, "Unable to import mined block: {}", err,);
				false
			},
		}
	}
}

impl<Block, AC, L, Proof> Clone for MiningHandle<Block, AC, L, Proof>
where
	Block: BlockT<Hash = H256>,
	AC: ProvideRuntimeApi<Block>,
	L: sc_consensus::JustificationSyncLink<Block>,
{
	fn clone(&self) -> Self {
		Self {
			version: self.version.clone(),
			client: self.client.clone(),
			justification_sync_link: self.justification_sync_link.clone(),
			build: self.build.clone(),
			block_import: self.block_import.clone(),
		}
	}
}

/// Reason why the stream fired - either a block was imported or enough transactions arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildTrigger {
	/// Initial trigger to bootstrap mining (fires once on first poll).
	Initial,
	/// A new block was imported from the network.
	BlockImported,
	/// Enough new transactions arrived to trigger a rebuild.
	NewTransactions,
}

/// A stream that waits for a block import or new transactions (with rate limiting).
///
/// This enables block producers to include new transactions faster by rebuilding
/// the block being mined when transactions arrive, rather than waiting for the
/// next block import or timeout.
///
/// Rate limiting prevents excessive rebuilds - we limit to `max_rebuilds_per_sec`
/// and require at least `min_txs_for_rebuild` transactions before triggering.
pub struct UntilImportedOrTransaction<Block: BlockT, TxHash> {
	/// Block import notifications stream.
	import_notifications: ImportNotifications<Block>,
	/// Transaction pool import notifications stream.
	tx_notifications: Pin<Box<dyn Stream<Item = TxHash> + Send>>,
	/// Minimum interval between transaction-triggered rebuilds.
	min_rebuild_interval: Duration,
	/// Last time we triggered a rebuild due to transactions.
	last_tx_rebuild: Option<Instant>,
	/// Number of transactions accumulated since last rebuild.
	pending_tx_count: usize,
	/// Minimum number of transactions required to trigger a rebuild.
	min_txs_for_rebuild: usize,
	/// Rate limit delay - if set, we're waiting before we can fire again.
	rate_limit_delay: Option<Delay>,
	/// Whether we've fired the initial trigger yet.
	initial_fired: bool,
}

impl<Block: BlockT, TxHash> UntilImportedOrTransaction<Block, TxHash> {
	/// Create a new stream.
	///
	/// # Arguments
	/// * `import_notifications` - Stream of block import notifications
	/// * `tx_notifications` - Stream of transaction import notifications
	/// * `max_rebuilds_per_sec` - Maximum transaction-triggered rebuilds per second
	/// * `min_txs_for_rebuild` - Minimum transactions needed to trigger a rebuild
	pub fn new(
		import_notifications: ImportNotifications<Block>,
		tx_notifications: impl Stream<Item = TxHash> + Send + 'static,
		max_rebuilds_per_sec: u32,
		min_txs_for_rebuild: usize,
	) -> Self {
		let min_rebuild_interval = if max_rebuilds_per_sec > 0 {
			Duration::from_millis(1000 / max_rebuilds_per_sec as u64)
		} else {
			Duration::from_secs(u64::MAX) // Effectively disable tx-triggered rebuilds
		};

		Self {
			import_notifications,
			tx_notifications: Box::pin(tx_notifications),
			min_rebuild_interval,
			last_tx_rebuild: None,
			pending_tx_count: 0,
			min_txs_for_rebuild,
			rate_limit_delay: None,
			initial_fired: false,
		}
	}
}

impl<Block: BlockT, TxHash> Stream for UntilImportedOrTransaction<Block, TxHash> {
	type Item = RebuildTrigger;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<RebuildTrigger>> {
		// Fire immediately on first poll to bootstrap mining at genesis
		if !self.initial_fired {
			self.initial_fired = true;
			debug!(target: LOG_TARGET, "Initial trigger, bootstrapping block production");
			return Poll::Ready(Some(RebuildTrigger::Initial));
		}

		// Check for block imports first - these always trigger immediately
		loop {
			match Stream::poll_next(Pin::new(&mut self.import_notifications), cx) {
				Poll::Pending => break,
				Poll::Ready(Some(_)) => {
					// Block import resets the transaction counter since we'll build fresh
					self.pending_tx_count = 0;
					self.rate_limit_delay = None;
					debug!(target: LOG_TARGET, "Block imported, triggering rebuild");
					return Poll::Ready(Some(RebuildTrigger::BlockImported));
				},
				Poll::Ready(None) => return Poll::Ready(None),
			}
		}

		// Drain all pending transaction notifications and count them
		loop {
			match Stream::poll_next(Pin::new(&mut self.tx_notifications), cx) {
				Poll::Pending => break,
				Poll::Ready(Some(_)) => {
					self.pending_tx_count += 1;
				},
				Poll::Ready(None) => {
					// Transaction stream closed, but we can still listen for block imports
					break;
				},
			}
		}

		// Check if we have enough transactions and rate limiting allows us to fire
		if self.pending_tx_count >= self.min_txs_for_rebuild {
			let now = Instant::now();
			let can_fire = match self.last_tx_rebuild {
				None => true,
				Some(last) => now.duration_since(last) >= self.min_rebuild_interval,
			};

			if can_fire {
				self.last_tx_rebuild = Some(now);
				let tx_count = self.pending_tx_count;
				self.pending_tx_count = 0;
				self.rate_limit_delay = None;
				debug!(
					target: LOG_TARGET,
					"New transactions ({} txs), triggering rebuild",
					tx_count
				);
				return Poll::Ready(Some(RebuildTrigger::NewTransactions));
			} else {
				// We have enough txs but need to wait for rate limit
				// Set up a delay to wake us when we can fire
				let time_since_last = now.duration_since(self.last_tx_rebuild.unwrap());
				let wait_time = self.min_rebuild_interval.saturating_sub(time_since_last);

				if self.rate_limit_delay.is_none() {
					self.rate_limit_delay = Some(Delay::new(wait_time));
				}

				if let Some(ref mut delay) = self.rate_limit_delay {
					match Future::poll(Pin::new(delay), cx) {
						Poll::Ready(()) => {
							self.last_tx_rebuild = Some(Instant::now());
							let tx_count = self.pending_tx_count;
							self.pending_tx_count = 0;
							self.rate_limit_delay = None;
							debug!(
								target: LOG_TARGET,
								"Rate limit expired, triggering rebuild for {} txs",
								tx_count
							);
							return Poll::Ready(Some(RebuildTrigger::NewTransactions));
						},
						Poll::Pending => {},
					}
				}
			}
		}

		Poll::Pending
	}
}
