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
use sp_blockchain::HeaderBackend;
use sp_consensus::{BlockOrigin, Proposal};
use sp_consensus_qpow::{QPoWApi, Seal, POW_ENGINE_ID};
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use std::{
	pin::Pin,
	sync::{
		atomic::{AtomicBool, AtomicUsize, Ordering},
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
	/// Rewards preimage (32 bytes) - stored in block headers, hashed to derive wormhole address.
	pub rewards_preimage: [u8; 32],
	/// Mining target difficulty.
	pub difficulty: D,
}

/// A block template: a fully built block proposal awaiting a seal, together with
/// the metadata miners need to search for that seal.
pub struct BlockTemplate<Block: BlockT, Proof> {
	/// Mining metadata.
	pub metadata: MiningMetadata<Block::Hash, U512>,
	/// Mining proposal.
	pub proposal: Proposal<Block, Proof>,
}

/// Identifies a generation of the block template. Bumped whenever the template is
/// replaced, cleared, or consumed, so holders can detect that a snapshot went stale.
#[derive(Eq, PartialEq, Clone, Copy)]
pub struct TemplateVersion(usize);

/// Retains the latest version so changes cannot be lost before a waiter subscribes.
#[derive(Clone)]
struct TemplateVersionSignal {
	value: Arc<AtomicUsize>,
	changed: tokio::sync::watch::Sender<()>,
}

impl TemplateVersionSignal {
	fn new() -> Self {
		Self { value: Arc::new(AtomicUsize::new(0)), changed: tokio::sync::watch::channel(()).0 }
	}

	fn current(&self) -> TemplateVersion {
		TemplateVersion(self.value.load(Ordering::SeqCst))
	}

	fn increment(&self) {
		self.value.fetch_add(1, Ordering::SeqCst);
		self.changed.send_replace(());
	}

	async fn changed_since(&self, version: TemplateVersion) {
		let mut receiver = self.changed.subscribe();
		loop {
			receiver.borrow_and_update();
			if self.current() != version {
				return;
			}
			// The sender in self remains alive for the duration of this wait.
			if receiver.changed().await.is_err() {
				return;
			}
		}
	}
}

#[derive(Clone, Default)]
struct AuthoringGate {
	enabled: Arc<AtomicBool>,
}

impl AuthoringGate {
	fn is_enabled(&self) -> bool {
		self.enabled.load(Ordering::SeqCst)
	}

	fn set_enabled(&self, enabled: bool) -> bool {
		self.enabled.swap(enabled, Ordering::SeqCst) != enabled
	}
}

/// Mining worker that exposes structs to query the current block template and submit mined blocks.
pub struct MiningHandle<Block: BlockT, AC, L: sc_consensus::JustificationSyncLink<Block>, Proof> {
	version: TemplateVersionSignal,
	authoring_gate: AuthoringGate,
	client: Arc<AC>,
	justification_sync_link: Arc<L>,
	template: Arc<Mutex<Option<BlockTemplate<Block, Proof>>>>,
	block_import: Arc<BoxBlockImport<Block>>,
	// Rebuild-request channel shared with the template-building task, so mining can be
	// resumed (post-sync, or after a failed import) without an external trigger.
	pending_rebuild: Arc<Mutex<Option<Block::Hash>>>,
	rebuild_notify: futures::channel::mpsc::Sender<()>,
}

impl<Block, AC, L, Proof> MiningHandle<Block, AC, L, Proof>
where
	Block: BlockT<Hash = H256>,
	AC: ProvideRuntimeApi<Block> + HeaderBackend<Block>,
	AC::Api: QPoWApi<Block>,
	L: sc_consensus::JustificationSyncLink<Block>,
{
	fn increment_version(&self) {
		self.version.increment();
	}

	pub(crate) fn new(
		client: Arc<AC>,
		block_import: BoxBlockImport<Block>,
		justification_sync_link: L,
		pending_rebuild: Arc<Mutex<Option<Block::Hash>>>,
		rebuild_notify: futures::channel::mpsc::Sender<()>,
	) -> Self {
		Self {
			version: TemplateVersionSignal::new(),
			authoring_gate: AuthoringGate::default(),
			client,
			justification_sync_link: Arc::new(justification_sync_link),
			template: Arc::new(Mutex::new(None)),
			block_import: Arc::new(block_import),
			pending_rebuild,
			rebuild_notify,
		}
	}

	/// Enable or pause proposal building, mining, and seal submission together.
	pub fn set_authoring_enabled(&self, enabled: bool) {
		if !self.authoring_gate.set_enabled(enabled) {
			return;
		}

		*self.pending_rebuild.lock() = None;
		if enabled {
			self.request_rebuild();
		} else {
			self.template.lock().take();
			self.increment_version();
		}
	}

	/// Whether proposal building, mining, and seal submission are enabled.
	pub fn is_authoring_enabled(&self) -> bool {
		self.authoring_gate.is_enabled()
	}

	/// Request a rebuild of the block template on top of the current best block.
	/// Used to resume mining after the template was cleared (post-sync) or a submitted
	/// block failed to import, leaving no template.
	pub fn request_rebuild(&self) {
		if !self.is_authoring_enabled() {
			return;
		}
		let best_hash = self.client.info().best_hash;
		*self.pending_rebuild.lock() = Some(best_hash);
		let _ = self.rebuild_notify.clone().try_send(());
	}

	pub(crate) fn on_new_template(&self, value: BlockTemplate<Block, Proof>) {
		let mut template = self.template.lock();
		if !self.is_authoring_enabled() {
			return;
		}
		*template = Some(value);
		self.increment_version();
	}

	/// Wait until the template is replaced, cleared, or consumed.
	/// Changes since the supplied version are observed even before this future is polled.
	pub async fn wait_for_template_change(&self, version: TemplateVersion) {
		self.version.changed_since(version).await;
	}

	/// Get the version of the current block template.
	///
	/// This returns type `TemplateVersion` which can only compare equality. If it is unchanged,
	/// then it can be certain that `best_hash` and `metadata` were not changed.
	pub fn template_version(&self) -> TemplateVersion {
		self.version.current()
	}

	/// Get the current best hash. `None` if the worker has just started or the last
	/// template was consumed.
	pub fn best_hash(&self) -> Option<Block::Hash> {
		if !self.is_authoring_enabled() {
			return None;
		}
		self.template.lock().as_ref().map(|t| t.metadata.best_hash)
	}

	/// Get a copy of the current mining metadata, if available.
	pub fn metadata(&self) -> Option<MiningMetadata<Block::Hash, U512>> {
		if !self.is_authoring_enabled() {
			return None;
		}
		self.template.lock().as_ref().map(|t| t.metadata.clone())
	}

	/// Submit a mined seal. The seal will be validated before consuming the template.
	/// Returns true if the submission is successful.
	pub async fn submit(&self, seal: Seal) -> bool {
		// Atomically verify and take the template in a single lock acquisition.
		// This prevents TOCTOU issues where a rebuild could land between verify and consume.
		let template = {
			let mut template_guard = self.template.lock();

			if !self.is_authoring_enabled() {
				debug!(target: LOG_TARGET, "Ignoring mined seal while authoring is paused");
				return false;
			}

			// Extract metadata for verification while keeping the template in place
			let (pre_hash, best_hash) = match template_guard.as_ref() {
				Some(t) => (t.metadata.pre_hash.0, t.metadata.best_hash),
				None => {
					warn!(target: LOG_TARGET, "Unable to import mined block: no block template exists");
					return false;
				},
			};

			// Verify seal before consuming the template
			let nonce: [u8; 64] = match seal.as_slice().try_into() {
				Ok(arr) => arr,
				Err(_) => {
					warn!(target: LOG_TARGET, "Seal does not have exactly 64 bytes, got {}", seal.len());
					return false;
				},
			};

			match self.client.runtime_api().verify_nonce_local_mining(best_hash, pre_hash, nonce) {
				Ok(true) => {
					// Seal is valid, take the template. This cannot be None because:
					// - We hold the lock continuously since checking as_ref() above
					// - No other code path modifies template_guard between check and take
					let template = template_guard.take();
					self.increment_version();
					match template {
						Some(t) => t,
						None => {
							// This branch is unreachable given the lock invariants, but we handle
							// it explicitly rather than using unwrap() to satisfy safety
							// guidelines.
							warn!(target: LOG_TARGET, "Template disappeared while holding lock (should be unreachable)");
							return false;
						},
					}
				},
				Ok(false) => {
					warn!(
						target: LOG_TARGET,
						"Seal verification failed: pre_hash={:?}, best_hash={:?}",
						pre_hash, best_hash
					);
					return false;
				},
				Err(e) => {
					warn!(target: LOG_TARGET, "Runtime API error verifying seal: {:?}", e);
					return false;
				},
			}
		};

		let seal = DigestItem::Seal(POW_ENGINE_ID, seal);
		let (header, body) = template.proposal.block.deconstruct();

		let mut import_block: BlockImportParams<Block> =
			BlockImportParams::new(BlockOrigin::Own, header);
		import_block.post_digests.push(seal);
		import_block.body = Some(body);
		import_block.state_action =
			StateAction::ApplyChanges(StorageChanges::Changes(template.proposal.storage_changes));

		let block_number = *import_block.header.number();
		let post_hash = import_block.post_header().hash();
		let import_result = self.block_import.import_block(import_block).await;

		match import_result {
			Ok(res) => {
				res.handle_justification(&post_hash, block_number, &self.justification_sync_link);

				true
			},
			Err(err) => {
				warn!(target: LOG_TARGET, "Unable to import mined block: {}", err,);
				// The template was consumed above; request a fresh one so mining
				// resumes without waiting for an external trigger.
				self.request_rebuild();
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
			authoring_gate: self.authoring_gate.clone(),
			client: self.client.clone(),
			justification_sync_link: self.justification_sync_link.clone(),
			template: self.template.clone(),
			block_import: self.block_import.clone(),
			pending_rebuild: self.pending_rebuild.clone(),
			rebuild_notify: self.rebuild_notify.clone(),
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
/// Rate limiting prevents excessive rebuilds via `min_rebuild_interval`.
pub struct UntilImportedOrTransaction<Block: BlockT, TxHash> {
	/// Block import notifications stream.
	import_notifications: ImportNotifications<Block>,
	/// Transaction pool import notifications stream.
	tx_notifications: Pin<Box<dyn Stream<Item = TxHash> + Send>>,
	/// Minimum interval between transaction-triggered rebuilds.
	min_rebuild_interval: Duration,
	/// Rate limit delay - if set, we're waiting before we can fire again.
	rate_limit_delay: Option<Delay>,
	/// Whether we've fired the initial trigger yet.
	initial_fired: bool,
	/// Whether we have pending transactions waiting to trigger a rebuild.
	has_pending_tx: bool,
}

impl<Block: BlockT, TxHash> UntilImportedOrTransaction<Block, TxHash> {
	/// Create a new stream.
	///
	/// # Arguments
	/// * `import_notifications` - Stream of block import notifications
	/// * `tx_notifications` - Stream of transaction import notifications
	/// * `min_rebuild_interval` - Minimum time between transaction-triggered rebuilds
	pub fn new(
		import_notifications: ImportNotifications<Block>,
		tx_notifications: impl Stream<Item = TxHash> + Send + 'static,
		min_rebuild_interval: Duration,
	) -> Self {
		Self {
			import_notifications,
			tx_notifications: Box::pin(tx_notifications),
			min_rebuild_interval,
			rate_limit_delay: None,
			initial_fired: false,
			has_pending_tx: false,
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
		if let Poll::Ready(notification) =
			Stream::poll_next(Pin::new(&mut self.import_notifications), cx)
		{
			match notification {
				Some(_) => {
					// Block import resets pending state since we'll build fresh
					self.has_pending_tx = false;
					self.rate_limit_delay = None;
					debug!(target: LOG_TARGET, "Block imported, triggering rebuild");
					return Poll::Ready(Some(RebuildTrigger::BlockImported));
				},
				None => return Poll::Ready(None),
			}
		}

		// Drain all pending transaction notifications
		while let Poll::Ready(Some(_)) = Stream::poll_next(Pin::new(&mut self.tx_notifications), cx)
		{
			self.has_pending_tx = true;
		}

		// If we have pending transactions, check rate limit
		if self.has_pending_tx {
			// Check if rate limit allows firing (no delay or delay expired)
			let can_fire = match self.rate_limit_delay.as_mut() {
				None => true,
				Some(delay) => Future::poll(Pin::new(delay), cx).is_ready(),
			};

			if can_fire {
				self.has_pending_tx = false;
				self.rate_limit_delay = Some(Delay::new(self.min_rebuild_interval));
				debug!(target: LOG_TARGET, "New transaction(s), triggering rebuild");
				return Poll::Ready(Some(RebuildTrigger::NewTransactions));
			}
		}

		Poll::Pending
	}
}

#[cfg(test)]
mod tests {
	use super::AuthoringGate;

	#[test]
	fn authoring_gate_is_shared_and_starts_disabled() {
		let gate = AuthoringGate::default();
		let clone = gate.clone();

		assert!(!gate.is_enabled());
		assert!(clone.set_enabled(true));
		assert!(gate.is_enabled());
		assert!(gate.set_enabled(false));
		assert!(!clone.is_enabled());
	}
}

#[cfg(test)]
mod template_version_signal_tests {
	use super::TemplateVersionSignal;
	use futures::{executor::block_on, task::ArcWake, Future, FutureExt};
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	};

	#[derive(Default)]
	struct WakeCounter(AtomicUsize);
	impl ArcWake for WakeCounter {
		fn wake_by_ref(this: &Arc<Self>) {
			this.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn change_before_subscription_is_retained() {
		let signal = TemplateVersionSignal::new();
		let version = signal.current();
		signal.increment();
		assert!(signal.changed_since(version).now_or_never().is_some());
	}

	#[test]
	fn change_wakes_all_waiters_without_a_timer() {
		let signal = TemplateVersionSignal::new();
		let version = signal.current();
		let mut first = Box::pin(signal.changed_since(version));
		let mut second = Box::pin(signal.changed_since(version));
		let counter = Arc::new(WakeCounter::default());
		let waker = futures::task::waker(counter.clone());
		let mut context = std::task::Context::from_waker(&waker);
		assert!(first.as_mut().poll(&mut context).is_pending());
		assert!(second.as_mut().poll(&mut context).is_pending());
		signal.clone().increment();
		assert_eq!(counter.0.load(Ordering::SeqCst), 2);
		assert!(first.as_mut().poll(&mut context).is_ready());
		assert!(second.as_mut().poll(&mut context).is_ready());
	}

	#[test]
	fn cancelled_wait_does_not_consume_next_change() {
		let signal = TemplateVersionSignal::new();
		let version = signal.current();
		assert!(signal.changed_since(version).now_or_never().is_none());
		signal.increment();
		block_on(signal.changed_since(version));
		assert!(signal.changed_since(signal.current()).now_or_never().is_none());
	}
}
