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

//! Contains the state of the chain synchronization process
//!
//! At any given point in time, a running node tries as much as possible to be at the head of the
//! chain. This module handles the logic of which blocks to request from remotes, and processing
//! responses. It yields blocks to check and potentially move to the database.
//!
//! # Usage
//!
//! The `ChainSync` struct maintains the state of the block requests. Whenever something happens on
//! the network, or whenever a block has been successfully verified, call the appropriate method in
//! order to update it.

use crate::{
	block_relay_protocol::{BlockDownloader, BlockResponseError},
	blocks::BlockCollection,
	justification_requests::ExtraRequests,
	schema::v1::{StateRequest, StateResponse},
	service::network::NetworkServiceHandle,
	strategy::{
		disconnected_peers::DisconnectedPeers,
		future_blocks::FutureBlockPeers,
		state_sync::{ImportResult, StateSync, StateSyncProvider},
		warp::{WarpSyncPhase, WarpSyncProgress},
		StrategyKey, SyncingAction, SyncingStrategy,
	},
	types::{BadPeer, SyncState, SyncStatus},
	LOG_TARGET,
};

use codec::Encode;
use futures::{channel::oneshot, FutureExt};
use log::{debug, error, info, trace, warn};
use prometheus_endpoint::{register, Gauge, PrometheusError, Registry, U64};
use prost::Message;
use sc_client_api::{blockchain::BlockGap, BlockBackend, ProofProvider};
use sc_consensus::{BlockImportError, BlockImportStatus, IncomingBlock};
use sc_network::{IfDisconnected, ProtocolName};
use sc_network_common::sync::message::{
	BlockAnnounce, BlockAttributes, BlockData, BlockRequest, BlockResponse, Direction, FromBlock,
};
use sc_network_types::PeerId;
use sp_arithmetic::traits::Saturating;
use sp_blockchain::{Error as ClientError, HeaderBackend, HeaderMetadata};
use sp_consensus::{BlockOrigin, BlockStatus};
use sp_consensus_qpow::{FutureBlock, InvalidBlock};
use sp_core::storage::StateVersion;
use sp_runtime::{
	traits::{
		Block as BlockT, CheckedAdd, CheckedSub, Hash as HashT, Header as HeaderT, NumberFor, One,
		SaturatedConversion, Zero,
	},
	EncodedJustification, Justifications,
};

use std::{
	any::Any,
	collections::{HashMap, HashSet},
	ops::Range,
	sync::Arc,
	time::Instant,
};

#[cfg(all(test, feature = "substrate-test"))]
mod test;

/// Maximum blocks to store in the import queue.
const MAX_IMPORTING_BLOCKS: usize = 2048;

/// `true` when block import failed with the marker error `T`.
///
/// Block import uses [`InvalidBlock`] and [`FutureBlock`] to say that the block caused the
/// failure. Any other error is a failure of this node.
fn import_failed_with<T: std::error::Error + 'static>(error: &sp_consensus::Error) -> bool {
	matches!(error, sp_consensus::Error::Other(inner) if inner.is::<T>())
}

/// Maximum blocks to download ahead of any gap.
const MAX_DOWNLOAD_AHEAD: u32 = 2048;

/// Maximum blocks to look backwards. The gap is the difference between the highest block and the
/// common block of a node.
const MAX_BLOCKS_TO_LOOK_BACKWARDS: u32 = MAX_DOWNLOAD_AHEAD / 2;

/// Pick the state to sync as the latest finalized number minus this.
const STATE_SYNC_FINALITY_THRESHOLD: u32 = 8;

/// We use a heuristic that with a high likelihood, by the time
/// `MAJOR_SYNC_BLOCKS` have been imported we'll be on the same
/// chain as (or at least closer to) the peer so we want to delay
/// the ancestor search to not waste time doing that when we are
/// so far behind.
const MAJOR_SYNC_BLOCKS: u8 = 5;

fn catch_up_request_is_gated<N: Ord>(peer_best: N, local_best: N, request_in_flight: bool) -> bool {
	request_in_flight && peer_best > local_best
}

/// Mirrors Bitcoin's IBD philosophy: only locally-held blocks (the import
/// backlog) can label the node as major-syncing; peer height claims never do.
/// Authoring does not depend on this state at all, so a forged claim (or an
/// unverified queue burst) can at worst tweak notification batching and
/// download scheduling for a few seconds.
fn block_import_origin(gap: bool, normal_sync: bool, major_syncing: bool) -> BlockOrigin {
	if gap || (normal_sync && major_syncing) {
		BlockOrigin::NetworkInitialSync
	} else {
		BlockOrigin::NetworkBroadcast
	}
}

mod rep {
	use sc_network::ReputationChange as Rep;
	/// Reputation change when a peer sent us a message that led to a
	/// database read error.
	pub const BLOCKCHAIN_READ_ERROR: Rep = Rep::new(-(1 << 16), "DB Error");

	/// Reputation change when a peer sent us a status message with a different
	/// genesis than us.
	pub const GENESIS_MISMATCH: Rep = Rep::new(i32::MIN, "Genesis mismatch");

	/// Reputation change for peers which send us a block with an incomplete header.
	pub const INCOMPLETE_HEADER: Rep = Rep::new(-(1 << 20), "Incomplete header");

	/// Reputation change for peers which send us a block which we fail to verify.
	pub const VERIFICATION_FAIL: Rep = Rep::new(-(1 << 29), "Block verification failed");

	/// Reputation change for peers which send us a known bad block.
	pub const BAD_BLOCK: Rep = Rep::new(-(1 << 29), "Bad block");

	/// Peer did not provide us with advertised block data.
	pub const NO_BLOCK: Rep = Rep::new(-(1 << 29), "No requested block data");

	/// Reputation change for peers which send us non-requested block data.
	pub const NOT_REQUESTED: Rep = Rep::new(-(1 << 29), "Not requested block data");

	/// Reputation change for peers which send us a block with bad justifications.
	pub const BAD_JUSTIFICATION: Rep = Rep::new(-(1 << 16), "Bad justification");

	/// Reputation change when a peer sent us invalid ancestry result.
	pub const UNKNOWN_ANCESTOR: Rep = Rep::new(-(1 << 16), "DB Error");

	/// Peer response data does not have requested bits.
	pub const BAD_RESPONSE: Rep = Rep::new(-(1 << 12), "Incomplete response");

	/// We received a message that failed to decode.
	pub const BAD_MESSAGE: Rep = Rep::new(-(1 << 12), "Bad message");
}

struct Metrics {
	queued_blocks: Gauge<U64>,
	fork_targets: Gauge<U64>,
}

impl Metrics {
	fn register(r: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			queued_blocks: {
				let g =
					Gauge::new("substrate_sync_queued_blocks", "Number of blocks in import queue")?;
				register(g, r)?
			},
			fork_targets: {
				let g = Gauge::new("substrate_sync_fork_targets", "Number of fork sync targets")?;
				register(g, r)?
			},
		})
	}
}

#[derive(Debug, Clone)]
enum AllowedRequests {
	Some(HashSet<PeerId>),
	All,
}

impl AllowedRequests {
	fn add(&mut self, id: &PeerId) {
		if let Self::Some(ref mut set) = self {
			set.insert(*id);
		}
	}

	fn take(&mut self) -> Self {
		std::mem::take(self)
	}

	fn set_all(&mut self) {
		*self = Self::All;
	}

	fn contains(&self, id: &PeerId) -> bool {
		match self {
			Self::Some(set) => set.contains(id),
			Self::All => true,
		}
	}

	fn is_empty(&self) -> bool {
		match self {
			Self::Some(set) => set.is_empty(),
			Self::All => false,
		}
	}

	fn clear(&mut self) {
		std::mem::take(self);
	}
}

impl Default for AllowedRequests {
	fn default() -> Self {
		Self::Some(HashSet::default())
	}
}

struct GapSync<B: BlockT> {
	blocks: BlockCollection<B>,
	best_queued_number: NumberFor<B>,
	target: NumberFor<B>,
}

/// Sync operation mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ChainSyncMode {
	/// Full block download and verification.
	Full,
	/// Download blocks and the latest state.
	LightState {
		/// Skip state proof download and verification.
		skip_proofs: bool,
		/// Download indexed transactions for recent blocks.
		storage_chain_mode: bool,
	},
}

/// All the data we have about a Peer that we are trying to sync with
#[derive(Debug, Clone)]
pub(crate) struct PeerSync<B: BlockT> {
	/// Peer id of this peer.
	pub peer_id: PeerId,
	/// The common number is the block number that is a common point of
	/// ancestry for both our chains (as far as we know).
	pub common_number: NumberFor<B>,
	/// The hash of the best block that we've seen for this peer.
	pub best_hash: B::Hash,
	/// The number of the best block that we've seen for this peer.
	pub best_number: NumberFor<B>,
	/// The state of syncing this peer is in for us, generally categories
	/// into `Available` or "busy" with something as defined by `PeerSyncState`.
	pub state: PeerSyncState<B>,
}

impl<B: BlockT> PeerSync<B> {
	/// Update the `common_number` iff `new_common > common_number`.
	fn update_common_number(&mut self, new_common: NumberFor<B>) {
		if self.common_number < new_common {
			trace!(
				target: LOG_TARGET,
				"Updating peer {} common number from={} => to={}.",
				self.peer_id,
				self.common_number,
				new_common,
			);
			self.common_number = new_common;
		}
	}
}

struct ForkTarget<B: BlockT> {
	number: NumberFor<B>,
	parent_hash: Option<B::Hash>,
	peers: HashSet<PeerId>,
}

/// What sync keeps about a block while the import queue has it.
struct QueuedBlock<B: BlockT> {
	/// The peer that supplied the block. `BlockImportError::Other` does not carry a peer id.
	origin: Option<PeerId>,
	/// The fork target that queueing the block completed. It is the only record that the
	/// block was wanted, so it is put back when the block has to be requested again.
	fork_target: Option<ForkTarget<B>>,
}

/// The state of syncing between a Peer and ourselves.
///
/// Generally two categories, "busy" or `Available`. If busy, the enum
/// defines what we are busy with.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum PeerSyncState<B: BlockT> {
	/// Available for sync requests.
	Available,
	/// Searching for ancestors the Peer has in common with us.
	AncestorSearch { start: NumberFor<B>, current: NumberFor<B>, state: AncestorSearchState<B> },
	/// Actively downloading new blocks, starting from the given Number.
	DownloadingNew(NumberFor<B>),
	/// Downloading a stale block with given Hash. Stale means that it is a
	/// block with a number that is lower than our best number. It might be
	/// from a fork and not necessarily already imported.
	DownloadingStale(B::Hash),
	/// Downloading justification for given block hash.
	DownloadingJustification(B::Hash),
	/// Downloading state.
	DownloadingState,
	/// Actively downloading block history after warp sync.
	DownloadingGap(NumberFor<B>),
}

impl<B: BlockT> PeerSyncState<B> {
	pub fn is_available(&self) -> bool {
		matches!(self, Self::Available)
	}
}

/// The main data structure which contains all the state for a chains
/// active syncing strategy.
pub struct ChainSync<B: BlockT, Client> {
	/// Chain client.
	client: Arc<Client>,
	/// The active peers that we are using to sync and their PeerSync status
	peers: HashMap<PeerId, PeerSync<B>>,
	disconnected_peers: DisconnectedPeers,
	future_block_peers: FutureBlockPeers,
	/// A `BlockCollection` of blocks that are being downloaded from peers
	blocks: BlockCollection<B>,
	/// The best block number in our queue of blocks to import
	best_queued_number: NumberFor<B>,
	/// The best block hash in our queue of blocks to import
	best_queued_hash: B::Hash,
	/// Current mode (full/light)
	mode: ChainSyncMode,
	/// Any extra justification requests.
	extra_justifications: ExtraRequests<B>,
	/// Blocks that have been downloaded and are queued for import, by hash.
	queue_blocks: HashMap<B::Hash, QueuedBlock<B>>,
	/// A pending attempt to start the state sync.
	///
	/// The initiation of state sync may be deferred in cases where other conditions
	/// are not yet met when the finalized block notification is received, such as
	/// when `queue_blocks` is not empty or there are no peers. This field holds the
	/// necessary information to attempt the state sync at a later point when
	/// conditions are satisfied.
	pending_state_sync_attempt: Option<(B::Hash, NumberFor<B>, bool)>,
	/// Fork sync targets.
	fork_targets: HashMap<B::Hash, ForkTarget<B>>,
	/// A set of peers for which there might be potential block requests
	allowed_requests: AllowedRequests,
	/// Maximum number of peers to ask the same blocks in parallel.
	max_parallel_downloads: u32,
	/// Maximum blocks per request.
	max_blocks_per_request: u32,
	/// Protocol name used to send out state requests
	state_request_protocol_name: ProtocolName,
	/// Total number of downloaded blocks.
	downloaded_blocks: usize,
	/// State sync in progress, if any.
	state_sync: Option<StateSync<B, Client>>,
	/// Enable importing existing blocks. This is used after the state download to
	/// catch up to the latest state while re-importing blocks.
	import_existing: bool,
	/// Block downloader
	block_downloader: Arc<dyn BlockDownloader<B>>,
	/// Gap download process.
	gap_sync: Option<GapSync<B>>,
	/// Pending actions.
	actions: Vec<SyncingAction<B>>,
	/// Prometheus metrics.
	metrics: Option<Metrics>,
}

impl<B, Client> SyncingStrategy<B> for ChainSync<B, Client>
where
	B: BlockT,
	Client: HeaderBackend<B>
		+ BlockBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ ProofProvider<B>
		+ Send
		+ Sync
		+ 'static,
{
	fn add_peer(&mut self, peer_id: PeerId, best_hash: B::Hash, best_number: NumberFor<B>) {
		match self.add_peer_inner(peer_id, best_hash, best_number) {
			Ok(Some(request)) => {
				let action = self.create_block_request_action(peer_id, request);
				self.actions.push(action);
			},
			Ok(None) => {},
			Err(bad_peer) => self.actions.push(SyncingAction::DropPeer(bad_peer)),
		}
	}

	fn remove_peer(&mut self, peer_id: &PeerId) {
		self.blocks.clear_peer_download(peer_id);
		if let Some(gap_sync) = &mut self.gap_sync {
			gap_sync.blocks.clear_peer_download(peer_id)
		}

		if let Some(state) = self.peers.remove(peer_id) {
			if !state.state.is_available() {
				if let Some(bad_peer) =
					self.disconnected_peers.on_disconnect_during_request(*peer_id)
				{
					self.actions.push(SyncingAction::DropPeer(bad_peer));
				}
			}
		}

		self.extra_justifications.peer_disconnected(peer_id);
		self.allowed_requests.set_all();
		self.fork_targets.retain(|_, target| {
			target.peers.remove(peer_id);
			!target.peers.is_empty()
		});
		if let Some(metrics) = &self.metrics {
			metrics.fork_targets.set(self.fork_targets.len().try_into().unwrap_or(u64::MAX));
		}

		let blocks = self.ready_blocks();

		if !blocks.is_empty() {
			self.validate_and_queue_blocks(blocks, false, true);
		}
	}

	fn on_validated_block_announce(
		&mut self,
		is_best: bool,
		peer_id: PeerId,
		announce: &BlockAnnounce<B::Header>,
	) -> Option<(B::Hash, NumberFor<B>)> {
		let number = *announce.header.number();
		let hash = announce.header.hash();
		let parent_status =
			self.block_status(announce.header.parent_hash()).unwrap_or(BlockStatus::Unknown);
		let known_parent = parent_status != BlockStatus::Unknown;
		let ancient_parent = parent_status == BlockStatus::InChainPruned;

		let known = self.is_known(&hash);
		let is_major_syncing = self.is_major_syncing();
		let peer = if let Some(peer) = self.peers.get_mut(&peer_id) {
			peer
		} else {
			error!(target: LOG_TARGET, "💔 Called `on_validated_block_announce` with a bad peer ID {peer_id}");
			return Some((hash, number));
		};

		if let PeerSyncState::AncestorSearch { .. } = peer.state {
			trace!(target: LOG_TARGET, "Peer {} is in the ancestor search state.", peer_id);
			return None;
		}

		let continues_known_fork =
			known || known_parent || announce.header.parent_hash() == &peer.best_hash;

		let peer_info = is_best.then(|| {
			// update their best block
			peer.best_number = number;
			peer.best_hash = hash;

			(hash, number)
		});

		// If the announced block is the best they have and is not ahead of us, our common number
		// is either one further ahead or it's the one they just announced, if we know about it.
		if is_best {
			let best_queued_number = self.best_queued_number;

			if known && best_queued_number >= number {
				peer.update_common_number(number);
			} else if announce.header.parent_hash() == &self.best_queued_hash ||
				known_parent && best_queued_number >= number
			{
				peer.update_common_number(number.saturating_sub(One::one()));
			}

			if !continues_known_fork && !is_major_syncing {
				let current = number.min(best_queued_number);
				peer.common_number = peer.common_number.min(self.client.info().finalized_number);
				let old_state = std::mem::replace(
					&mut peer.state,
					PeerSyncState::AncestorSearch {
						current,
						start: best_queued_number,
						state: AncestorSearchState::ExponentialBackoff(One::one()),
					},
				);
				self.cancel_peer_request(peer_id, old_state);

				let request = ancestry_request::<B>(current);
				let action = self.create_block_request_action(peer_id, request);
				self.actions.push(action);

				return peer_info;
			}
		}
		self.allowed_requests.add(&peer_id);

		// known block case
		if known || self.is_already_downloading(&hash) {
			trace!(target: LOG_TARGET, "Known block announce from {}: {}", peer_id, hash);
			if let Some(target) = self.fork_targets.get_mut(&hash) {
				target.peers.insert(peer_id);
			}
			return peer_info
		}

		if ancient_parent {
			trace!(
				target: LOG_TARGET,
				"Ignored ancient block announced from {}: {} {:?}",
				peer_id,
				hash,
				announce.header,
			);
			return peer_info
		}

		if self.status().state == SyncState::Idle {
			trace!(
				target: LOG_TARGET,
				"Added sync target for block announced from {}: {} {:?}",
				peer_id,
				hash,
				announce.summary(),
			);
			self.fork_targets
				.entry(hash)
				.or_insert_with(|| {
					if let Some(metrics) = &self.metrics {
						metrics.fork_targets.inc();
					}

					ForkTarget {
						number,
						parent_hash: Some(*announce.header.parent_hash()),
						peers: Default::default(),
					}
				})
				.peers
				.insert(peer_id);
		}

		peer_info
	}

	// The implementation is similar to `on_validated_block_announce` with unknown parent hash.
	fn set_sync_fork_request(
		&mut self,
		mut peers: Vec<PeerId>,
		hash: &B::Hash,
		number: NumberFor<B>,
	) {
		if peers.is_empty() {
			peers = self
				.peers
				.iter()
				// Only request blocks from peers who are ahead or on a par.
				.filter(|(_, peer)| peer.best_number >= number)
				.map(|(id, _)| *id)
				.collect();

			debug!(
				target: LOG_TARGET,
				"Explicit sync request for block {hash:?} with no peers specified. \
				Syncing from these peers {peers:?} instead.",
			);
		} else {
			debug!(
				target: LOG_TARGET,
				"Explicit sync request for block {hash:?} with {peers:?}",
			);
		}

		if self.is_known(hash) {
			debug!(target: LOG_TARGET, "Refusing to sync known hash {hash:?}");
			return
		}

		trace!(target: LOG_TARGET, "Downloading requested old fork {hash:?}");
		for peer_id in &peers {
			if let Some(peer) = self.peers.get_mut(peer_id) {
				if let PeerSyncState::AncestorSearch { .. } = peer.state {
					continue
				}

				if number > peer.best_number {
					peer.best_number = number;
					peer.best_hash = *hash;
				}
				self.allowed_requests.add(peer_id);
			}
		}

		self.fork_targets
			.entry(*hash)
			.or_insert_with(|| {
				if let Some(metrics) = &self.metrics {
					metrics.fork_targets.inc();
				}

				ForkTarget { number, peers: Default::default(), parent_hash: None }
			})
			.peers
			.extend(peers);
	}

	fn request_justification(&mut self, hash: &B::Hash, number: NumberFor<B>) {
		let client = &self.client;
		self.extra_justifications
			.schedule((*hash, number), |base, block| is_descendent_of(&**client, base, block))
	}

	fn clear_justification_requests(&mut self) {
		self.extra_justifications.reset();
	}

	fn on_justification_import(&mut self, hash: B::Hash, number: NumberFor<B>, success: bool) {
		let finalization_result = if success { Ok((hash, number)) } else { Err(()) };
		self.extra_justifications
			.try_finalize_root((hash, number), finalization_result, true);
		self.allowed_requests.set_all();
	}

	fn on_generic_response(
		&mut self,
		peer_id: &PeerId,
		key: StrategyKey,
		protocol_name: ProtocolName,
		response: Box<dyn Any + Send>,
	) {
		if Self::STRATEGY_KEY != key {
			warn!(
				target: LOG_TARGET,
				"Unexpected generic response strategy key {key:?}, protocol {protocol_name}",
			);
			debug_assert!(false);
			return;
		}

		if protocol_name == self.state_request_protocol_name {
			let Ok(response) = response.downcast::<Vec<u8>>() else {
				warn!(target: LOG_TARGET, "Failed to downcast state response");
				debug_assert!(false);
				return;
			};

			if let Err(bad_peer) = self.on_state_data(&peer_id, &response) {
				self.actions.push(SyncingAction::DropPeer(bad_peer));
			}
		} else if &protocol_name == self.block_downloader.protocol_name() {
			let Ok(response) = response
				.downcast::<(BlockRequest<B>, Result<Vec<BlockData<B>>, BlockResponseError>)>()
			else {
				warn!(target: LOG_TARGET, "Failed to downcast block response");
				debug_assert!(false);
				return;
			};

			let (request, response) = *response;
			let blocks = match response {
				Ok(blocks) => blocks,
				Err(BlockResponseError::DecodeFailed(e)) => {
					debug!(
						target: LOG_TARGET,
						"Failed to decode block response from peer {:?}: {:?}.",
						peer_id,
						e
					);
					self.actions.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::BAD_MESSAGE)));
					return;
				},
				Err(BlockResponseError::ExtractionFailed(e)) => {
					debug!(
						target: LOG_TARGET,
						"Failed to extract blocks from peer response {:?}: {:?}.",
						peer_id,
						e
					);
					self.actions.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::BAD_MESSAGE)));
					return;
				},
			};

			if let Err(bad_peer) = self.on_block_response(peer_id, key, request, blocks) {
				self.actions.push(SyncingAction::DropPeer(bad_peer));
			}
		} else {
			warn!(
				target: LOG_TARGET,
				"Unexpected generic response protocol {protocol_name}, strategy key \
				{key:?}",
			);
			debug_assert!(false);
		}
	}

	fn on_blocks_processed(
		&mut self,
		imported: usize,
		count: usize,
		results: Vec<(Result<BlockImportStatus<NumberFor<B>>, BlockImportError>, B::Hash)>,
	) {
		trace!(target: LOG_TARGET, "Imported {imported} of {count}");

		let mut has_error = false;
		let mut from_the_future = false;
		let mut queued_blocks = Vec::with_capacity(results.len());
		for (_, hash) in &results {
			let queued = self.queue_blocks.remove(hash);
			if queued.is_some() {
				if let Some(metrics) = &self.metrics {
					metrics.queued_blocks.dec();
				}
			}
			queued_blocks.push(queued);
			self.blocks.clear_queued(hash);
			if let Some(gap_sync) = &mut self.gap_sync {
				gap_sync.blocks.clear_queued(hash);
			}
		}
		for ((result, hash), queued) in results.into_iter().zip(queued_blocks) {
			let (origin, fork_target) =
				queued.map_or((None, None), |queued| (queued.origin, queued.fork_target));

			// A block from the future passes later, and the queue does not try the blocks
			// behind it. Sync restarts from what the peers advertise as their best block,
			// which does not lead back to a block on another fork.
			if !has_error {
				from_the_future = matches!(
					&result,
					Err(BlockImportError::Other(error)) if import_failed_with::<FutureBlock>(error)
				);
			}
			if let Some(fork_target) = fork_target.filter(|_| from_the_future) {
				if self.fork_targets.insert(hash, fork_target).is_none() {
					if let Some(metrics) = &self.metrics {
						metrics.fork_targets.inc();
					}
				}
			}

			if has_error {
				continue
			}

			has_error |= result.is_err();

			match result {
				Ok(BlockImportStatus::ImportedKnown(number, peer_id)) => {
					if let Some(peer) = peer_id {
						self.update_peer_common_number(&peer, number);
					}
					self.complete_gap_if_target(number);
				},
				Ok(BlockImportStatus::ImportedUnknown(number, aux, peer_id)) => {
					if aux.clear_justification_requests {
						trace!(
							target: LOG_TARGET,
							"Block imported clears all pending justification requests {number}: {hash:?}",
						);
						self.clear_justification_requests();
					}

					if aux.needs_justification {
						trace!(
							target: LOG_TARGET,
							"Block imported but requires justification {number}: {hash:?}",
						);
						self.request_justification(&hash, number);
					}

					if aux.bad_justification {
						if let Some(ref peer) = peer_id {
							warn!("💔 Sent block with bad justification to import");
							self.actions.push(SyncingAction::DropPeer(BadPeer(
								*peer,
								rep::BAD_JUSTIFICATION,
							)));
						}
					}

					if let Some(peer) = peer_id {
						self.update_peer_common_number(&peer, number);
					}
					let state_sync_complete =
						self.state_sync.as_ref().map_or(false, |s| s.target_hash() == hash);
					if state_sync_complete {
						info!(
							target: LOG_TARGET,
							"State sync is complete ({} MiB), restarting block sync.",
							self.state_sync.as_ref().map_or(0, |s| s.progress().size / (1024 * 1024)),
						);
						self.state_sync = None;
						self.mode = ChainSyncMode::Full;
						self.restart();
					}

					self.complete_gap_if_target(number);
				},
				Err(BlockImportError::IncompleteHeader(peer_id)) =>
					if let Some(peer) = peer_id {
						warn!(
							target: LOG_TARGET,
							"💔 Peer sent block with incomplete header to import",
						);
						self.actions
							.push(SyncingAction::DropPeer(BadPeer(peer, rep::INCOMPLETE_HEADER)));
						self.restart();
					},
				Err(BlockImportError::VerificationFailed(peer_id, e)) => {
					let extra_message = peer_id
						.map_or_else(|| "".into(), |peer| format!(" received from ({peer})"));

					warn!(
						target: LOG_TARGET,
						"💔 Verification failed for block {hash:?}{extra_message}: {e:?}",
					);

					if let Some(peer) = peer_id {
						self.actions
							.push(SyncingAction::DropPeer(BadPeer(peer, rep::VERIFICATION_FAIL)));
					}

					self.restart();
				},
				Err(BlockImportError::BadBlock(peer_id)) => {
					if let Some(peer) = peer_id {
						warn!(
							target: LOG_TARGET,
							"💔 Block {hash:?} received from peer {peer} has been blacklisted",
						);
						self.actions.push(SyncingAction::DropPeer(BadPeer(peer, rep::BAD_BLOCK)));
					}
					self.restart();
				},
				Err(BlockImportError::MissingState) => {
					// This may happen if the chain we were requesting upon has been discarded
					// in the meantime because other chain has been finalized.
					// Don't mark it as bad as it still may be synced if explicitly requested.
					trace!(target: LOG_TARGET, "Obsolete block {hash:?}");
				},
				// A missing parent is a normal race: the parent may still be in flight. Do not
				// penalise the peer.
				Err(BlockImportError::UnknownParent) => {
					warn!(target: LOG_TARGET, "💔 Error importing block {hash:?}: unknown parent");
					self.state_sync = None;
					self.restart();
				},
				// `Other` carries no peer. Drop the sender only when import reported the block
				// itself as invalid. A block from the future passes later, so its sender stays
				// connected and is asked again after a pause. A failure of this node is retried
				// at once.
				Err(BlockImportError::Other(error)) => {
					warn!(target: LOG_TARGET, "💔 Error importing block {hash:?}: {error}");
					if let Some(peer) = origin {
						if import_failed_with::<InvalidBlock>(&error) {
							self.actions
								.push(SyncingAction::DropPeer(BadPeer(peer, rep::BAD_BLOCK)));
						} else if import_failed_with::<FutureBlock>(&error) {
							let pause = self.future_block_peers.pause(peer, Instant::now());
							debug!(
								target: LOG_TARGET,
								"Not requesting blocks from {peer} for {pause:?}",
							);
						}
					}
					self.state_sync = None;
					self.restart();
				},
				Err(BlockImportError::Cancelled) => {},
			};
		}

		self.allowed_requests.set_all();
	}

	fn on_block_finalized(&mut self, hash: &B::Hash, number: NumberFor<B>) {
		let client = &self.client;
		let r = self.extra_justifications.on_block_finalized(hash, number, |base, block| {
			is_descendent_of(&**client, base, block)
		});

		if let ChainSyncMode::LightState { skip_proofs, .. } = &self.mode {
			if self.state_sync.is_none() {
				if !self.peers.is_empty() && self.queue_blocks.is_empty() {
					self.attempt_state_sync(*hash, number, *skip_proofs);
				} else {
					self.pending_state_sync_attempt.replace((*hash, number, *skip_proofs));
				}
			}
		}

		if let Err(err) = r {
			warn!(
				target: LOG_TARGET,
				"💔 Error cleaning up pending extra justification data requests: {err}",
			);
		}
	}

	fn update_chain_info(&mut self, best_hash: &B::Hash, best_number: NumberFor<B>) {
		self.on_block_queued(best_hash, best_number);
	}

	fn is_major_syncing(&self) -> bool {
		self.status().state.is_major_syncing()
	}

	fn is_peer_drop_gated(&self, peer_id: &PeerId) -> bool {
		let best_block = self.client.info().best_number;
		self.peers.get(peer_id).is_some_and(|peer| {
			catch_up_request_is_gated(peer.best_number, best_block, !peer.state.is_available())
		})
	}

	fn num_peers(&self) -> usize {
		self.peers.len()
	}

	fn status(&self) -> SyncStatus<B> {
		// Only the local import backlog (blocks we actually hold) can put the
		// node into a major-sync state; peer height claims never do.
		let sync_state = if self.has_download_catch_up_work() {
			SyncState::Importing { target: self.best_queued_number }
		} else {
			SyncState::Idle
		};
		// `best_seen_block` is display/monitoring only (informant, RPC
		// `system_syncState`, `sync_target` metric): the median of
		// peer-claimed heights, clamped to our own best. A forged claim can
		// at worst skew a dashboard; it never feeds authoring, import-origin,
		// or peer-drop decisions.
		let best_seen_block = self.median_claimed_height().map(|claimed| {
			claimed.max(match sync_state {
				SyncState::Downloading { target } | SyncState::Importing { target } => target,
				SyncState::Idle => self.best_queued_number,
			})
		});

		let warp_sync_progress = self.gap_sync.as_ref().map(|gap_sync| WarpSyncProgress {
			phase: WarpSyncPhase::DownloadingBlocks(gap_sync.best_queued_number),
			total_bytes: 0,
		});

		SyncStatus {
			state: sync_state,
			best_seen_block,
			num_peers: self.peers.len() as u32,
			queued_blocks: self.queue_blocks.len() as u32,
			state_sync: self.state_sync.as_ref().map(|s| s.progress()),
			warp_sync: warp_sync_progress,
		}
	}

	fn on_request_failed(&mut self, peer_id: &PeerId) {
		debug!(target: LOG_TARGET, "received on_request_failed for {:?} has peer: {:?}", peer_id, self.peers.get_mut(peer_id).is_some());

		if let Some(peer) = self.peers.get_mut(peer_id) {
			debug!(target: LOG_TARGET, "Request failed for peer {:?}, previous state: {:?}", peer_id, peer.state);
			match peer.state {
				PeerSyncState::DownloadingNew(_) => {
					self.blocks.clear_peer_download(peer_id);
					peer.state = PeerSyncState::Available;
				},
				PeerSyncState::DownloadingGap(_) => {
					if let Some(gap) = &mut self.gap_sync {
						gap.blocks.clear_peer_download(peer_id);
					}
					peer.state = PeerSyncState::Available;
				},
				PeerSyncState::DownloadingStale(_) => {
					peer.state = PeerSyncState::Available;
				},
				PeerSyncState::AncestorSearch { .. } => {
					peer.state = PeerSyncState::Available;
				},
				PeerSyncState::DownloadingJustification(_) |
				PeerSyncState::DownloadingState |
				PeerSyncState::Available => {},
			}
			debug!(target: LOG_TARGET, "on_request_failed: now_available={}", matches!(peer.state, PeerSyncState::Available));
		}
		self.allowed_requests.set_all();
	}

	fn num_downloaded_blocks(&self) -> usize {
		self.downloaded_blocks
	}

	fn num_sync_requests(&self) -> usize {
		self.fork_targets
			.values()
			.filter(|f| f.number <= self.best_queued_number)
			.count()
	}

	fn actions(
		&mut self,
		network_service: &NetworkServiceHandle,
	) -> Result<Vec<SyncingAction<B>>, ClientError> {
		if !self.peers.is_empty() && self.queue_blocks.is_empty() {
			if let Some((hash, number, skip_proofs)) = self.pending_state_sync_attempt.take() {
				self.attempt_state_sync(hash, number, skip_proofs);
			}
		}

		let block_requests = self
			.block_requests()
			.into_iter()
			.map(|(peer_id, request)| {
				debug!(
					target: LOG_TARGET,
					"Scheduling block request to {:?}: fields={:?}, from={:?}, dir={:?}, max={:?}",
					peer_id, request.fields, request.from, request.direction, request.max,
				);
				self.create_block_request_action(peer_id, request)
			})
			.collect::<Vec<_>>();
		self.actions.extend(block_requests);

		let justification_requests = self
			.justification_requests()
			.into_iter()
			.map(|(peer_id, request)| self.create_block_request_action(peer_id, request))
			.collect::<Vec<_>>();
		self.actions.extend(justification_requests);

		let state_request = self.state_request().into_iter().map(|(peer_id, request)| {
			trace!(
				target: LOG_TARGET,
				"Created `StrategyRequest` to {peer_id}.",
			);

			let (tx, rx) = oneshot::channel();

			network_service.start_request(
				peer_id,
				self.state_request_protocol_name.clone(),
				request.encode_to_vec(),
				tx,
				IfDisconnected::ImmediateError,
			);

			SyncingAction::StartRequest {
				peer_id,
				key: Self::STRATEGY_KEY,
				request: async move {
					Ok(rx.await?.and_then(|(response, protocol_name)| {
						Ok((Box::new(response) as Box<dyn Any + Send>, protocol_name))
					}))
				}
				.boxed(),
				remove_obsolete: false,
			}
		});
		self.actions.extend(state_request);

		Ok(std::mem::take(&mut self.actions))
	}
}

impl<B, Client> ChainSync<B, Client>
where
	B: BlockT,
	Client: HeaderBackend<B>
		+ BlockBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ ProofProvider<B>
		+ Send
		+ Sync
		+ 'static,
{
	/// Strategy key used by chain sync.
	pub const STRATEGY_KEY: StrategyKey = StrategyKey::new("ChainSync");

	/// Create a new instance.
	pub fn new(
		mode: ChainSyncMode,
		client: Arc<Client>,
		max_parallel_downloads: u32,
		max_blocks_per_request: u32,
		state_request_protocol_name: ProtocolName,
		block_downloader: Arc<dyn BlockDownloader<B>>,
		metrics_registry: Option<&Registry>,
		initial_peers: impl Iterator<Item = (PeerId, B::Hash, NumberFor<B>)>,
	) -> Result<Self, ClientError> {
		let mut sync = Self {
			client,
			peers: HashMap::new(),
			disconnected_peers: DisconnectedPeers::new(),
			future_block_peers: FutureBlockPeers::new(),
			blocks: BlockCollection::new(),
			best_queued_hash: Default::default(),
			best_queued_number: Zero::zero(),
			extra_justifications: ExtraRequests::new("justification", metrics_registry),
			mode,
			queue_blocks: Default::default(),
			pending_state_sync_attempt: None,
			fork_targets: Default::default(),
			allowed_requests: Default::default(),
			max_parallel_downloads,
			max_blocks_per_request,
			state_request_protocol_name,
			downloaded_blocks: 0,
			state_sync: None,
			import_existing: false,
			block_downloader,
			gap_sync: None,
			actions: Vec::new(),
			metrics: metrics_registry.and_then(|r| match Metrics::register(r) {
				Ok(metrics) => Some(metrics),
				Err(err) => {
					log::error!(
						target: LOG_TARGET,
						"Failed to register `ChainSync` metrics {err:?}",
					);
					None
				},
			}),
		};

		sync.reset_sync_start_point()?;
		initial_peers.for_each(|(peer_id, best_hash, best_number)| {
			sync.add_peer(peer_id, best_hash, best_number);
		});

		Ok(sync)
	}

	fn has_download_catch_up_work(&self) -> bool {
		self.best_queued_number.saturating_sub(self.client.info().best_number) >
			MAJOR_SYNC_BLOCKS.into()
	}

	/// Median of the heights our peers claim in their handshakes and
	/// announcements. Unverified by construction — for display and monitoring
	/// only, never for gating.
	fn median_claimed_height(&self) -> Option<NumberFor<B>> {
		let mut claims: Vec<_> = self.peers.values().map(|peer| peer.best_number).collect();
		if claims.is_empty() {
			return None;
		}
		let mid = claims.len() / 2;
		let (_, median, _) = claims.select_nth_unstable(mid);
		Some(*median)
	}

	/// Complete the gap sync if the target number is reached and there is a gap.
	fn complete_gap_if_target(&mut self, number: NumberFor<B>) {
		let gap_sync_complete = self.gap_sync.as_ref().map_or(false, |s| s.target == number);
		if gap_sync_complete {
			info!(
				target: LOG_TARGET,
				"Block history download is complete."
			);
			self.gap_sync = None;
		}
	}

	#[must_use]
	fn add_peer_inner(
		&mut self,
		peer_id: PeerId,
		best_hash: B::Hash,
		best_number: NumberFor<B>,
	) -> Result<Option<BlockRequest<B>>, BadPeer> {
		// There is nothing sync can get from the node that has no blockchain data.
		match self.block_status(&best_hash) {
			Err(e) => {
				debug!(target: LOG_TARGET, "Error reading blockchain: {e}");
				Err(BadPeer(peer_id, rep::BLOCKCHAIN_READ_ERROR))
			},
			Ok(BlockStatus::KnownBad) => {
				info!(
					"💔 New peer {peer_id} with known bad best block {best_hash} ({best_number})."
				);
				Err(BadPeer(peer_id, rep::BAD_BLOCK))
			},
			Ok(BlockStatus::Unknown) => {
				if best_number.is_zero() {
					info!(
						"💔 New peer {} with unknown genesis hash {} ({}).",
						peer_id, best_hash, best_number,
					);
					return Err(BadPeer(peer_id, rep::GENESIS_MISMATCH));
				}

				// If there are more than `MAJOR_SYNC_BLOCKS` in the import queue then we have
				// enough to do in the import queue that it's not worth kicking off
				// an ancestor search, which is what we do in the next match case below.
				if self.queue_blocks.len() > MAJOR_SYNC_BLOCKS as usize {
					debug!(
						target: LOG_TARGET,
						"New peer {} with unknown best hash {} ({}), assuming common block.",
						peer_id,
						self.best_queued_hash,
						self.best_queued_number
					);
					self.peers.insert(
						peer_id,
						PeerSync {
							peer_id,
							common_number: self.best_queued_number,
							best_hash,
							best_number,
							state: PeerSyncState::Available,
						},
					);
					return Ok(None);
				}

				// If we are at genesis, just start downloading.
				let (state, req) = if self.best_queued_number.is_zero() {
					debug!(
						target: LOG_TARGET,
						"New peer {peer_id} with best hash {best_hash} ({best_number}).",
					);

					(PeerSyncState::Available, None)
				} else {
					let common_best = std::cmp::min(self.best_queued_number, best_number);

					debug!(
						target: LOG_TARGET,
						"New peer {} with unknown best hash {} ({}), searching for common ancestor.",
						peer_id,
						best_hash,
						best_number
					);

					(
						PeerSyncState::AncestorSearch {
							current: common_best,
							start: self.best_queued_number,
							state: AncestorSearchState::ExponentialBackoff(One::one()),
						},
						Some(ancestry_request::<B>(common_best)),
					)
				};

				self.allowed_requests.add(&peer_id);
				self.peers.insert(
					peer_id,
					PeerSync {
						peer_id,
						common_number: Zero::zero(),
						best_hash,
						best_number,
						state,
					},
				);

				Ok(req)
			},
			Ok(BlockStatus::Queued) |
			Ok(BlockStatus::InChainWithState) |
			Ok(BlockStatus::InChainPruned) => {
				debug!(
					target: LOG_TARGET,
					"New peer {peer_id} with known best hash {best_hash} ({best_number}).",
				);
				self.peers.insert(
					peer_id,
					PeerSync {
						peer_id,
						common_number: std::cmp::min(self.best_queued_number, best_number),
						best_hash,
						best_number,
						state: PeerSyncState::Available,
					},
				);
				self.allowed_requests.add(&peer_id);
				Ok(None)
			},
		}
	}

	fn cancel_peer_request(&mut self, peer_id: PeerId, old_state: PeerSyncState<B>) {
		match old_state {
			PeerSyncState::Available => return,
			PeerSyncState::DownloadingNew(_) => self.blocks.clear_peer_download(&peer_id),
			PeerSyncState::DownloadingGap(_) =>
				if let Some(gap) = &mut self.gap_sync {
					gap.blocks.clear_peer_download(&peer_id);
				},
			PeerSyncState::DownloadingJustification(_) =>
				self.extra_justifications.peer_disconnected(&peer_id),
			PeerSyncState::DownloadingState |
			PeerSyncState::DownloadingStale(_) |
			PeerSyncState::AncestorSearch { .. } => {},
		}
		self.actions
			.push(SyncingAction::CancelRequest { peer_id, key: Self::STRATEGY_KEY });
		self.allowed_requests.set_all();
	}

	fn create_block_request_action(
		&mut self,
		peer_id: PeerId,
		request: BlockRequest<B>,
	) -> SyncingAction<B> {
		let downloader = self.block_downloader.clone();
		if let Some((low, high)) = self.compute_request_range_u64(&request) {
			debug!(target: LOG_TARGET, "➡️ Sent block request to {}: {}..{}", peer_id, low, high);
		}

		SyncingAction::StartRequest {
			peer_id,
			key: Self::STRATEGY_KEY,
			request: async move {
				Ok(downloader.download_blocks(peer_id, request.clone()).await?.and_then(
					|(response, protocol_name)| {
						let decoded_response =
							downloader.block_response_into_blocks(&request, response);
						let result = Box::new((request, decoded_response)) as Box<dyn Any + Send>;
						Ok((result, protocol_name))
					},
				))
			}
			.boxed(),
			// Sending block request implies dropping obsolete pending response as we are not
			// interested in it anymore.
			remove_obsolete: true,
		}
	}

	/// Submit a block response for processing.
	#[must_use]
	fn on_block_data(
		&mut self,
		peer_id: &PeerId,
		request: Option<BlockRequest<B>>,
		response: BlockResponse<B>,
	) -> Result<(), BadPeer> {
		self.downloaded_blocks += response.blocks.len();
		let mut gap = false;
		let mut normal_sync = false;
		let mut blocks = response.blocks;

		if log::log_enabled!(target: LOG_TARGET, log::Level::Debug) {
			if let Some((low, high)) = self.compute_blocks_range_u64(&blocks) {
				debug!(target: LOG_TARGET, "⬅️ Received blocks from {}: {}..{}", peer_id, low, high);
			} else if let Some(req) = &request {
				if let Some((low, high)) = self.compute_request_range_u64(req) {
					debug!(target: LOG_TARGET, "⬅️ Received blocks from {} (range estimated from request): {}..{}", peer_id, low, high);
				}
			}
		}
		let new_blocks: Vec<IncomingBlock<B>> = if let Some(peer) = self.peers.get_mut(peer_id) {
			if request.as_ref().map_or(false, |r| r.direction == Direction::Descending) {
				trace!(target: LOG_TARGET, "Reversing incoming block list");
				blocks.reverse()
			}
			self.allowed_requests.add(peer_id);
			if let Some(request) = request {
				match &mut peer.state {
					PeerSyncState::DownloadingNew(range_start) => {
						let range_start = *range_start;
						let max_blocks = request.max;
						normal_sync = true;
						self.blocks.clear_peer_download(peer_id);
						peer.state = PeerSyncState::Available;
						if let Some(start_block) =
							validate_blocks::<B>(&blocks, peer_id, Some(request))?
						{
							ensure_response_within_request::<B>(
								start_block,
								blocks.len(),
								range_start,
								max_blocks,
								peer_id,
							)?;
							self.blocks.insert(start_block, blocks, *peer_id);
						}
						self.ready_blocks()
					},
					PeerSyncState::DownloadingGap(range_start) => {
						let range_start = *range_start;
						let max_blocks = request.max;
						peer.state = PeerSyncState::Available;
						if let Some(gap_sync) = &mut self.gap_sync {
							gap_sync.blocks.clear_peer_download(peer_id);
							if let Some(start_block) =
								validate_blocks::<B>(&blocks, peer_id, Some(request))?
							{
								ensure_response_within_request::<B>(
									start_block,
									blocks.len(),
									range_start,
									max_blocks,
									peer_id,
								)?;
								gap_sync.blocks.insert(start_block, blocks, *peer_id);
							}
							gap = true;
							let blocks: Vec<_> = gap_sync
								.blocks
								.ready_blocks(gap_sync.best_queued_number + One::one())
								.into_iter()
								.map(|block_data| {
									let justifications =
										block_data.block.justifications.or_else(|| {
											legacy_justification_mapping(
												block_data.block.justification,
											)
										});
									IncomingBlock {
										hash: block_data.block.hash,
										header: block_data.block.header,
										body: block_data.block.body,
										indexed_body: block_data.block.indexed_body,
										justifications,
										origin: block_data.origin,
										allow_missing_state: true,
										import_existing: self.import_existing,
										skip_execution: true,
										state: None,
									}
								})
								.collect();
							debug!(
								target: LOG_TARGET,
								"Drained {} gap blocks from {}",
								blocks.len(),
								gap_sync.best_queued_number,
							);
							blocks
						} else {
							debug!(target: LOG_TARGET, "Unexpected gap block response from {peer_id}");
							return Err(BadPeer(*peer_id, rep::NO_BLOCK));
						}
					},
					PeerSyncState::DownloadingStale(_) => {
						peer.state = PeerSyncState::Available;
						if blocks.is_empty() {
							debug!(target: LOG_TARGET, "Empty block response from {peer_id}");
							return Err(BadPeer(*peer_id, rep::NO_BLOCK));
						}
						validate_blocks::<B>(&blocks, peer_id, Some(request))?;
						blocks
							.into_iter()
							.map(|b| {
								let justifications = b
									.justifications
									.or_else(|| legacy_justification_mapping(b.justification));
								IncomingBlock {
									hash: b.hash,
									header: b.header,
									body: b.body,
									indexed_body: None,
									justifications,
									origin: Some(*peer_id),
									allow_missing_state: true,
									import_existing: self.import_existing,
									skip_execution: self.skip_execution(),
									state: None,
								}
							})
							.collect()
					},
					PeerSyncState::AncestorSearch { current, start, state } => {
						let matching_hash = match (blocks.get(0), self.client.hash(*current)) {
							(Some(block), Ok(maybe_our_block_hash)) => {
								trace!(
									target: LOG_TARGET,
									"Got ancestry block #{} ({}) from peer {}",
									current,
									block.hash,
									peer_id,
								);
								maybe_our_block_hash.filter(|x| x == &block.hash)
							},
							(None, _) => {
								debug!(
									target: LOG_TARGET,
									"Invalid response when searching for ancestor from {peer_id}",
								);
								return Err(BadPeer(*peer_id, rep::UNKNOWN_ANCESTOR));
							},
							(_, Err(e)) => {
								info!(
									target: LOG_TARGET,
									"❌ Error answering legitimate blockchain query: {e}",
								);
								return Err(BadPeer(*peer_id, rep::BLOCKCHAIN_READ_ERROR));
							},
						};
						if matching_hash.is_some() {
							if *start < self.best_queued_number &&
								self.best_queued_number <= peer.best_number
							{
								// We've made progress on this chain since the search was started.
								// Opportunistically set common number to updated number
								// instead of the one that started the search.
								trace!(
									target: LOG_TARGET,
									"Ancestry search: opportunistically updating peer {} common number from={} => to={}.",
									*peer_id,
									peer.common_number,
									self.best_queued_number,
								);
								peer.common_number = self.best_queued_number;
							} else if peer.common_number < *current {
								trace!(
									target: LOG_TARGET,
									"Ancestry search: updating peer {} common number from={} => to={}.",
									*peer_id,
									peer.common_number,
									*current,
								);
								peer.common_number = *current;
							}
						}
						if matching_hash.is_none() && current.is_zero() {
							trace!(
								target: LOG_TARGET,
								"Ancestry search: genesis mismatch for peer {peer_id}",
							);
							return Err(BadPeer(*peer_id, rep::GENESIS_MISMATCH));
						}
						if let Some((next_state, next_num)) =
							handle_ancestor_search_state(state, *current, matching_hash.is_some())
						{
							peer.state = PeerSyncState::AncestorSearch {
								current: next_num,
								start: *start,
								state: next_state,
							};
							let request = ancestry_request::<B>(next_num);
							let action = self.create_block_request_action(*peer_id, request);
							self.actions.push(action);
							return Ok(());
						} else {
							// Ancestry search is complete. Check if peer is on a stale fork unknown
							// to us and add it to sync targets if necessary.
							trace!(
								target: LOG_TARGET,
								"Ancestry search complete. Ours={} ({}), Theirs={} ({}), Common={:?} ({})",
								self.best_queued_hash,
								self.best_queued_number,
								peer.best_hash,
								peer.best_number,
								matching_hash,
								peer.common_number,
							);
							if peer.common_number < peer.best_number &&
								peer.best_number < self.best_queued_number
							{
								trace!(
									target: LOG_TARGET,
									"Added fork target {} for {}",
									peer.best_hash,
									peer_id,
								);
								self.fork_targets
									.entry(peer.best_hash)
									.or_insert_with(|| {
										if let Some(metrics) = &self.metrics {
											metrics.fork_targets.inc();
										}

										ForkTarget {
											number: peer.best_number,
											parent_hash: None,
											peers: Default::default(),
										}
									})
									.peers
									.insert(*peer_id);
							}
							peer.state = PeerSyncState::Available;
							return Ok(());
						}
					},
					PeerSyncState::Available |
					PeerSyncState::DownloadingJustification(..) |
					PeerSyncState::DownloadingState => Vec::new(),
				}
			} else {
				// When request.is_none() this is a block announcement. Just accept blocks.
				validate_blocks::<B>(&blocks, peer_id, None)?;
				blocks
					.into_iter()
					.map(|b| {
						let justifications = b
							.justifications
							.or_else(|| legacy_justification_mapping(b.justification));
						IncomingBlock {
							hash: b.hash,
							header: b.header,
							body: b.body,
							indexed_body: None,
							justifications,
							origin: Some(*peer_id),
							allow_missing_state: true,
							import_existing: false,
							skip_execution: true,
							state: None,
						}
					})
					.collect()
			}
		} else {
			// We don't know of this peer, so we also did not request anything from it.
			return Err(BadPeer(*peer_id, rep::NOT_REQUESTED));
		};

		self.validate_and_queue_blocks(new_blocks, gap, normal_sync);

		Ok(())
	}

	fn on_block_response(
		&mut self,
		peer_id: &PeerId,
		key: StrategyKey,
		request: BlockRequest<B>,
		blocks: Vec<BlockData<B>>,
	) -> Result<(), BadPeer> {
		if key != Self::STRATEGY_KEY {
			error!(
				target: LOG_TARGET,
				"`on_block_response()` called with unexpected key {key:?} for chain sync",
			);
			debug_assert!(false);
		}
		let block_response = BlockResponse::<B> { id: request.id, blocks };

		let blocks_range = || match (
			block_response
				.blocks
				.first()
				.and_then(|b| b.header.as_ref().map(|h| h.number())),
			block_response.blocks.last().and_then(|b| b.header.as_ref().map(|h| h.number())),
		) {
			(Some(first), Some(last)) if first != last => format!(" ({}..{})", first, last),
			(Some(first), Some(_)) => format!(" ({})", first),
			_ => Default::default(),
		};
		trace!(
			target: LOG_TARGET,
			"BlockResponse {} from {} with {} blocks {}",
			block_response.id,
			peer_id,
			block_response.blocks.len(),
			blocks_range(),
		);

		if request.fields == BlockAttributes::JUSTIFICATION {
			self.on_block_justification(*peer_id, block_response)
		} else {
			self.on_block_data(peer_id, Some(request), block_response)
		}
	}

	/// Submit a justification response for processing.
	#[must_use]
	fn on_block_justification(
		&mut self,
		peer_id: PeerId,
		response: BlockResponse<B>,
	) -> Result<(), BadPeer> {
		let peer = if let Some(peer) = self.peers.get_mut(&peer_id) {
			peer
		} else {
			error!(
				target: LOG_TARGET,
				"💔 Called on_block_justification with a peer ID of an unknown peer",
			);
			return Ok(());
		};

		self.allowed_requests.add(&peer_id);
		if let PeerSyncState::DownloadingJustification(hash) = peer.state {
			peer.state = PeerSyncState::Available;

			// We only request one justification at a time
			let justification = if let Some(block) = response.blocks.into_iter().next() {
				if hash != block.hash {
					warn!(
						target: LOG_TARGET,
						"💔 Invalid block justification provided by {}: requested: {:?} got: {:?}",
						peer_id,
						hash,
						block.hash,
					);
					return Err(BadPeer(peer_id, rep::BAD_JUSTIFICATION));
				}

				block
					.justifications
					.or_else(|| legacy_justification_mapping(block.justification))
			} else {
				// we might have asked the peer for a justification on a block that we assumed it
				// had but didn't (regardless of whether it had a justification for it or not).
				trace!(
					target: LOG_TARGET,
					"Peer {peer_id:?} provided empty response for justification request {hash:?}",
				);

				None
			};

			if let Some((peer_id, hash, number, justifications)) =
				self.extra_justifications.on_response(peer_id, justification)
			{
				self.actions.push(SyncingAction::ImportJustifications {
					peer_id,
					hash,
					number,
					justifications,
				});
				return Ok(());
			}
		}

		Ok(())
	}

	fn required_block_attributes(&self) -> BlockAttributes {
		match self.mode {
			ChainSyncMode::Full =>
				BlockAttributes::HEADER | BlockAttributes::JUSTIFICATION | BlockAttributes::BODY,
			ChainSyncMode::LightState { storage_chain_mode: false, .. } =>
				BlockAttributes::HEADER | BlockAttributes::JUSTIFICATION | BlockAttributes::BODY,
			ChainSyncMode::LightState { storage_chain_mode: true, .. } =>
				BlockAttributes::HEADER |
					BlockAttributes::JUSTIFICATION |
					BlockAttributes::INDEXED_BODY,
		}
	}

	fn skip_execution(&self) -> bool {
		match self.mode {
			ChainSyncMode::Full => false,
			ChainSyncMode::LightState { .. } => true,
		}
	}

	fn validate_and_queue_blocks(
		&mut self,
		mut new_blocks: Vec<IncomingBlock<B>>,
		gap: bool,
		normal_sync: bool,
	) {
		let orig_len = new_blocks.len();
		new_blocks.retain(|b| !self.queue_blocks.contains_key(&b.hash));
		if new_blocks.len() != orig_len {
			debug!(
				target: LOG_TARGET,
				"Ignoring {} blocks that are already queued",
				orig_len - new_blocks.len(),
			);
		}

		let last = new_blocks.last().and_then(|b| b.header.as_ref().map(|h| (b.hash, *h.number())));
		let fork_target = last.and_then(|(hash, number)| {
			self.on_block_queued(&hash, number).map(|target| (hash, target))
		});

		// Computed after `on_block_queued` so the batch itself counts toward
		// the import backlog; otherwise every batch of a smooth catch-up
		// (where imports keep pace with downloads) would be labeled
		// `NetworkBroadcast` and emit per-block import notifications.
		let origin = block_import_origin(gap, normal_sync, self.status().state.is_major_syncing());
		trace!(
			target: LOG_TARGET,
			"Accepted {} blocks ({:?}) with origin {:?}",
			new_blocks.len(),
			new_blocks.last().map(|b| b.hash),
			origin,
		);
		self.queue_blocks.extend(
			new_blocks
				.iter()
				.map(|b| (b.hash, QueuedBlock { origin: b.origin, fork_target: None })),
		);
		if let Some((hash, target)) = fork_target {
			if let Some(queued) = self.queue_blocks.get_mut(&hash) {
				queued.fork_target = Some(target);
			}
		}
		if let Some(metrics) = &self.metrics {
			metrics
				.queued_blocks
				.set(self.queue_blocks.len().try_into().unwrap_or(u64::MAX));
		}

		self.actions.push(SyncingAction::ImportBlocks { origin, blocks: new_blocks })
	}

	fn update_peer_common_number(&mut self, peer_id: &PeerId, new_common: NumberFor<B>) {
		if let Some(peer) = self.peers.get_mut(peer_id) {
			peer.update_common_number(new_common);
		}
	}

	/// Called when a block has been queued for import.
	///
	/// Updates our internal state for best queued block and then goes
	/// through all peers to update our view of their state as well.
	///
	/// Returns the fork target the block completed, if it was one.
	fn on_block_queued(&mut self, hash: &B::Hash, number: NumberFor<B>) -> Option<ForkTarget<B>> {
		let fork_target = self.fork_targets.remove(hash);
		if fork_target.is_some() {
			if let Some(metrics) = &self.metrics {
				metrics.fork_targets.dec();
			}
			trace!(target: LOG_TARGET, "Completed fork sync {hash:?}");
		}
		if let Some(gap_sync) = &mut self.gap_sync {
			if number > gap_sync.best_queued_number && number <= gap_sync.target {
				gap_sync.best_queued_number = number;
			}
		}
		if number > self.best_queued_number {
			self.best_queued_number = number;
			self.best_queued_hash = *hash;
			// Update common blocks
			for (n, peer) in self.peers.iter_mut() {
				if let PeerSyncState::AncestorSearch { .. } = peer.state {
					// Wait for ancestry search to complete first.
					continue;
				}
				let new_common_number =
					if peer.best_number >= number { number } else { peer.best_number };
				trace!(
					target: LOG_TARGET,
					"Updating peer {} info, ours={}, common={}->{}, their best={}",
					n,
					number,
					peer.common_number,
					new_common_number,
					peer.best_number,
				);
				peer.common_number = new_common_number;
			}
		}
		self.allowed_requests.set_all();
		fork_target
	}

	/// Restart the sync process. This will reset all pending block requests and return an iterator
	/// of new block requests to make to peers. Peers that were downloading finality data (i.e.
	/// their state was `DownloadingJustification`) are unaffected and will stay in the same state.
	fn restart(&mut self) {
		self.blocks.clear();
		if let Err(e) = self.reset_sync_start_point() {
			warn!(target: LOG_TARGET, "💔  Unable to restart sync: {e}");
		}
		self.allowed_requests.set_all();
		debug!(
			target: LOG_TARGET,
			"Restarted with {} ({})",
			self.best_queued_number,
			self.best_queued_hash,
		);
		let old_peers = std::mem::take(&mut self.peers);

		old_peers.into_iter().for_each(|(peer_id, mut peer_sync)| {
			match peer_sync.state {
				PeerSyncState::Available => {
					self.add_peer(peer_id, peer_sync.best_hash, peer_sync.best_number);
				},
				PeerSyncState::AncestorSearch { .. } |
				PeerSyncState::DownloadingNew(_) |
				PeerSyncState::DownloadingStale(_) |
				PeerSyncState::DownloadingGap(_) |
				PeerSyncState::DownloadingState => {
					// Cancel a request first, as `add_peer` may generate a new request.
					self.actions
						.push(SyncingAction::CancelRequest { peer_id, key: Self::STRATEGY_KEY });
					self.add_peer(peer_id, peer_sync.best_hash, peer_sync.best_number);
				},
				PeerSyncState::DownloadingJustification(_) => {
					// Peers that were downloading justifications
					// should be kept in that state.
					// We make sure our common number is at least something we have.
					trace!(
						target: LOG_TARGET,
						"Keeping peer {} after restart, updating common number from={} => to={} (our best).",
						peer_id,
						peer_sync.common_number,
						self.best_queued_number,
					);
					peer_sync.common_number = self.best_queued_number;
					self.peers.insert(peer_id, peer_sync);
				},
			}
		});
	}

	/// Find a block to start sync from. If we sync with state, that's the latest block we have
	/// state for.
	fn reset_sync_start_point(&mut self) -> Result<(), ClientError> {
		let info = self.client.info();
		debug!(target: LOG_TARGET, "Restarting sync with client info {info:?}");

		if matches!(self.mode, ChainSyncMode::LightState { .. }) && info.finalized_state.is_some() {
			warn!(
				target: LOG_TARGET,
				"Can't use fast sync mode with a partially synced database. Reverting to full sync mode."
			);
			self.mode = ChainSyncMode::Full;
		}

		self.import_existing = false;
		self.best_queued_hash = info.best_hash;
		self.best_queued_number = info.best_number;

		if self.mode == ChainSyncMode::Full &&
			self.client.block_status(info.best_hash)? != BlockStatus::InChainWithState
		{
			self.import_existing = true;
			// Latest state is missing, start with the last finalized state or genesis instead.
			if let Some((hash, number)) = info.finalized_state {
				debug!(target: LOG_TARGET, "Starting from finalized state #{number}");
				self.best_queued_hash = hash;
				self.best_queued_number = number;
			} else {
				debug!(target: LOG_TARGET, "Restarting from genesis");
				self.best_queued_hash = Default::default();
				self.best_queued_number = Zero::zero();
			}
		}

		let old_gap = self.gap_sync.take().map(|g| (g.best_queued_number, g.target));
		if let Some(BlockGap { start, end, .. }) = info.block_gap {
			debug!(target: LOG_TARGET, "Starting gap sync #{start} - #{end} (old gap best and target: {old_gap:?})");
			self.gap_sync = Some(GapSync {
				best_queued_number: start - One::one(),
				target: end,
				blocks: BlockCollection::new(),
			});
		}
		trace!(
			target: LOG_TARGET,
			"Restarted sync at #{} ({:?})",
			self.best_queued_number,
			self.best_queued_hash,
		);
		Ok(())
	}

	/// What is the status of the block corresponding to the given hash?
	fn block_status(&self, hash: &B::Hash) -> Result<BlockStatus, ClientError> {
		if self.queue_blocks.contains_key(hash) {
			return Ok(BlockStatus::Queued);
		}
		self.client.block_status(*hash)
	}

	/// Is the block corresponding to the given hash known?
	fn is_known(&self, hash: &B::Hash) -> bool {
		self.block_status(hash).ok().map_or(false, |s| s != BlockStatus::Unknown)
	}

	/// Is any peer downloading the given hash?
	fn is_already_downloading(&self, hash: &B::Hash) -> bool {
		self.peers
			.iter()
			.any(|(_, p)| p.state == PeerSyncState::DownloadingStale(*hash))
	}

	/// Get the set of downloaded blocks that are ready to be queued for import.
	fn ready_blocks(&mut self) -> Vec<IncomingBlock<B>> {
		self.blocks
			.ready_blocks(self.best_queued_number + One::one())
			.into_iter()
			.map(|block_data| {
				let justifications = block_data
					.block
					.justifications
					.or_else(|| legacy_justification_mapping(block_data.block.justification));
				IncomingBlock {
					hash: block_data.block.hash,
					header: block_data.block.header,
					body: block_data.block.body,
					indexed_body: block_data.block.indexed_body,
					justifications,
					origin: block_data.origin,
					allow_missing_state: true,
					import_existing: self.import_existing,
					skip_execution: self.skip_execution(),
					state: None,
				}
			})
			.collect()
	}

	/// Get justification requests scheduled by sync to be sent out.
	fn justification_requests(&mut self) -> Vec<(PeerId, BlockRequest<B>)> {
		let peers = &mut self.peers;
		let mut matcher = self.extra_justifications.matcher();
		std::iter::from_fn(move || {
			if let Some((peer, request)) = matcher.next(peers) {
				peers
					.get_mut(&peer)
					.expect(
						"`Matcher::next` guarantees the `PeerId` comes from the given peers; qed",
					)
					.state = PeerSyncState::DownloadingJustification(request.0);
				let req = BlockRequest::<B> {
					id: 0,
					fields: BlockAttributes::JUSTIFICATION,
					from: FromBlock::Hash(request.0),
					direction: Direction::Ascending,
					max: Some(1),
				};
				Some((peer, req))
			} else {
				None
			}
		})
		.collect()
	}

	/// Get block requests scheduled by sync to be sent out.
	fn block_requests(&mut self) -> Vec<(PeerId, BlockRequest<B>)> {
		if self.allowed_requests.is_empty() || self.state_sync.is_some() {
			return Vec::new();
		}

		if self.queue_blocks.len() > MAX_IMPORTING_BLOCKS {
			trace!(target: LOG_TARGET, "Too many blocks in the queue.");
			return Vec::new();
		}
		let transport_catch_up = self.has_download_catch_up_work();
		let attrs = self.required_block_attributes();
		let blocks = &mut self.blocks;
		let fork_targets = &mut self.fork_targets;
		let last_finalized =
			std::cmp::min(self.best_queued_number, self.client.info().finalized_number);
		let best_queued = self.best_queued_number;
		let client = &self.client;
		let queue_blocks = &self.queue_blocks;
		let allowed_requests = self.allowed_requests.clone();
		let max_parallel = if transport_catch_up { 1 } else { self.max_parallel_downloads };
		let max_blocks_per_request = self.max_blocks_per_request;
		let gap_sync = &mut self.gap_sync;
		let disconnected_peers = &mut self.disconnected_peers;
		let future_block_peers = &self.future_block_peers;
		let now = Instant::now();
		let metrics = self.metrics.as_ref();
		let requests = self
			.peers
			.iter_mut()
			.filter_map(move |(&id, peer)| {
				if !peer.state.is_available() {
					debug!(target: LOG_TARGET, "Skipping {:?}: state not available: {:?}", id, peer.state);
					return None;
				}
				if !allowed_requests.contains(&id) {
					debug!(target: LOG_TARGET, "Skipping {:?}: not in allowed_requests", id);
					return None;
				}
				if !disconnected_peers.is_peer_available(&id) {
					debug!(target: LOG_TARGET, "Skipping {:?}: backed off due to recent disconnect", id);
					return None;
				}
				if !peer.state.is_available() ||
					!allowed_requests.contains(&id) ||
					!disconnected_peers.is_peer_available(&id)
				{
					return None;
				}
				if future_block_peers.is_paused(&id, now) {
					debug!(target: LOG_TARGET, "Skipping {:?}: it sent a block from the future", id);
					return None;
				}

				// If our best queued is more than `MAX_BLOCKS_TO_LOOK_BACKWARDS` blocks away from
				// the common number, the peer best number is higher than our best queued and the
				// common number is smaller than the last finalized block number, we should do an
				// ancestor search to find a better common block. If the queue is full we wait till
				// all blocks are imported though.
				if best_queued.saturating_sub(peer.common_number) >
					MAX_BLOCKS_TO_LOOK_BACKWARDS.into() &&
					best_queued < peer.best_number &&
					peer.common_number < last_finalized &&
					queue_blocks.len() <= MAJOR_SYNC_BLOCKS as usize
				{
					debug!(
						target: LOG_TARGET,
						"Peer {:?} common block {} too far behind of our best {}. Starting ancestry search.",
						id,
						peer.common_number,
						best_queued,
					);
					let current = std::cmp::min(peer.best_number, best_queued);
					peer.state = PeerSyncState::AncestorSearch {
						current,
						start: best_queued,
						state: AncestorSearchState::ExponentialBackoff(One::one()),
					};
					Some((id, ancestry_request::<B>(current)))
				} else if let Some((range, req)) = peer_block_request(
					&id,
					peer,
					blocks,
					attrs,
					max_parallel,
					max_blocks_per_request,
					last_finalized,
					best_queued,
				) {
					peer.state = PeerSyncState::DownloadingNew(range.start);
					debug!(
						target: LOG_TARGET,
						"New block request for {}, (best:{}, common:{}) {:?}",
						id,
						peer.best_number,
						peer.common_number,
						req,
					);
					Some((id, req))
				} else if let Some((hash, req)) = fork_sync_request(
					&id,
					fork_targets,
					best_queued,
					last_finalized,
					attrs,
					|hash| {
						if queue_blocks.contains_key(hash) {
							BlockStatus::Queued
						} else {
							client.block_status(*hash).unwrap_or(BlockStatus::Unknown)
						}
					},
					max_blocks_per_request,
					metrics,
				) {
					trace!(target: LOG_TARGET, "Downloading fork {hash:?} from {id}");
					peer.state = PeerSyncState::DownloadingStale(hash);
					Some((id, req))
				} else if let Some((range, req)) = gap_sync.as_mut().and_then(|sync| {
					peer_gap_block_request(
						&id,
						peer,
						&mut sync.blocks,
						attrs,
						sync.target,
						sync.best_queued_number,
						max_blocks_per_request,
					)
				}) {
					peer.state = PeerSyncState::DownloadingGap(range.start);
					debug!(
						target: LOG_TARGET,
						"New gap block request for {}, (best:{}, common:{}) {:?}",
						id,
						peer.best_number,
						peer.common_number,
						req,
					);
					Some((id, req))
				} else {
					debug!(target: LOG_TARGET, "No request produced for {:?}", id);
					None
				}
			})
			.collect::<Vec<_>>();

		// Clear the allowed_requests state when sending new block requests
		// to prevent multiple inflight block requests from being issued.
		if !requests.is_empty() {
			self.allowed_requests.take();
		}

		requests
	}

	/// Get a state request scheduled by sync to be sent out (if any).
	fn state_request(&mut self) -> Option<(PeerId, StateRequest)> {
		if self.allowed_requests.is_empty() {
			return None;
		}
		if self.state_sync.is_some() &&
			self.peers.iter().any(|(_, peer)| peer.state == PeerSyncState::DownloadingState)
		{
			// Only one pending state request is allowed.
			return None;
		}
		if let Some(sync) = &self.state_sync {
			if sync.is_complete() {
				return None;
			}

			for (id, peer) in self.peers.iter_mut() {
				if peer.state.is_available() &&
					peer.common_number >= sync.target_number() &&
					self.disconnected_peers.is_peer_available(&id)
				{
					peer.state = PeerSyncState::DownloadingState;
					let request = sync.next_request();
					trace!(target: LOG_TARGET, "New StateRequest for {}: {:?}", id, request);
					self.allowed_requests.clear();
					return Some((*id, request));
				}
			}
		}
		None
	}

	#[must_use]
	fn on_state_data(&mut self, peer_id: &PeerId, response: &[u8]) -> Result<(), BadPeer> {
		let response = match StateResponse::decode(response) {
			Ok(response) => response,
			Err(error) => {
				debug!(
					target: LOG_TARGET,
					"Failed to decode state response from peer {peer_id:?}: {error:?}.",
				);

				return Err(BadPeer(*peer_id, rep::BAD_RESPONSE));
			},
		};

		if let Some(peer) = self.peers.get_mut(peer_id) {
			if let PeerSyncState::DownloadingState = peer.state {
				peer.state = PeerSyncState::Available;
				self.allowed_requests.set_all();
			}
		}
		let import_result = if let Some(sync) = &mut self.state_sync {
			debug!(
				target: LOG_TARGET,
				"Importing state data from {} with {} keys, {} proof nodes.",
				peer_id,
				response.entries.len(),
				response.proof.len(),
			);
			sync.import(response)
		} else {
			debug!(target: LOG_TARGET, "Ignored obsolete state response from {peer_id}");
			return Err(BadPeer(*peer_id, rep::NOT_REQUESTED));
		};

		match import_result {
			ImportResult::Import(hash, header, state, body, justifications) => {
				let origin = BlockOrigin::NetworkInitialSync;
				let block = IncomingBlock {
					hash,
					header: Some(header),
					body,
					indexed_body: None,
					justifications,
					origin: None,
					allow_missing_state: true,
					import_existing: true,
					skip_execution: self.skip_execution(),
					state: Some(state),
				};
				debug!(target: LOG_TARGET, "State download is complete. Import is queued");
				self.actions.push(SyncingAction::ImportBlocks { origin, blocks: vec![block] });
				Ok(())
			},
			ImportResult::Continue => Ok(()),
			ImportResult::BadResponse => {
				debug!(target: LOG_TARGET, "Bad state data received from {peer_id}");
				Err(BadPeer(*peer_id, rep::BAD_BLOCK))
			},
		}
	}

	fn attempt_state_sync(
		&mut self,
		finalized_hash: B::Hash,
		finalized_number: NumberFor<B>,
		skip_proofs: bool,
	) {
		let mut heads: Vec<_> = self.peers.values().map(|peer| peer.best_number).collect();
		heads.sort();
		let median = heads[heads.len() / 2];
		if finalized_number + STATE_SYNC_FINALITY_THRESHOLD.saturated_into() >= median {
			if let Ok(Some(header)) = self.client.header(finalized_hash) {
				log::debug!(
					target: LOG_TARGET,
					"Starting state sync for #{finalized_number} ({finalized_hash})",
				);
				self.state_sync =
					Some(StateSync::new(self.client.clone(), header, None, None, skip_proofs));
				self.allowed_requests.set_all();
			} else {
				log::error!(
					target: LOG_TARGET,
					"Failed to start state sync: header for finalized block \
					  #{finalized_number} ({finalized_hash}) is not available",
				);
				debug_assert!(false);
			}
		}
	}

	fn compute_request_range_u64(&self, req: &BlockRequest<B>) -> Option<(u64, u64)> {
		let max = req.max?;
		let start_num = match req.from {
			FromBlock::Number(n) => n.saturated_into::<u64>(),
			FromBlock::Hash(h) =>
				self.client.number(h).ok().flatten().map(|n| n.saturated_into::<u64>())?,
		};
		let span = max.saturating_sub(1) as u64;
		match req.direction {
			Direction::Descending => Some((start_num.saturating_sub(span), start_num)),
			Direction::Ascending => Some((start_num, start_num.saturating_add(span))),
		}
	}

	fn compute_blocks_range_u64(&self, blocks: &Vec<BlockData<B>>) -> Option<(u64, u64)> {
		let mut min_n: Option<u64> = None;
		let mut max_n: Option<u64> = None;
		for b in blocks.iter() {
			if let Some(h) = &b.header {
				let n = (*h.number()).saturated_into::<u64>();
				min_n = Some(min_n.map_or(n, |x| x.min(n)));
				max_n = Some(max_n.map_or(n, |x| x.max(n)));
			}
		}
		match (min_n, max_n) {
			(Some(lo), Some(hi)) => Some((lo, hi)),
			_ => None,
		}
	}

	#[cfg(all(test, feature = "substrate-test"))]
	#[must_use]
	fn take_actions(&mut self) -> impl Iterator<Item = SyncingAction<B>> {
		std::mem::take(&mut self.actions).into_iter()
	}
}

// This is purely during a backwards compatible transitionary period and should be removed
// once we can assume all nodes can send and receive multiple Justifications
// The ID tag is hardcoded here to avoid depending on the GRANDPA crate.
// See: https://github.com/paritytech/substrate/issues/8172
fn legacy_justification_mapping(
	justification: Option<EncodedJustification>,
) -> Option<Justifications> {
	justification.map(|just| (*b"FRNK", just).into())
}

/// Request the ancestry for a block. Sends a request for header and justification for the given
/// block number. Used during ancestry search.
fn ancestry_request<B: BlockT>(block: NumberFor<B>) -> BlockRequest<B> {
	BlockRequest::<B> {
		id: 0,
		fields: BlockAttributes::HEADER | BlockAttributes::JUSTIFICATION,
		from: FromBlock::Number(block),
		direction: Direction::Ascending,
		max: Some(1),
	}
}

/// The ancestor search state expresses which algorithm, and its stateful parameters, we are using
/// to try to find an ancestor block
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum AncestorSearchState<B: BlockT> {
	/// Use exponential backoff to find an ancestor, then switch to binary search.
	/// We keep track of the exponent.
	ExponentialBackoff(NumberFor<B>),
	/// Using binary search to find the best ancestor.
	/// We keep track of left and right bounds.
	BinarySearch(NumberFor<B>, NumberFor<B>),
}

/// This function handles the ancestor search strategy used. The goal is to find a common point
/// that both our chains agree on that is as close to the tip as possible.
/// The way this works is we first have an exponential backoff strategy, where we try to step
/// forward until we find a block hash mismatch. The size of the step doubles each step we take.
///
/// When we've found a block hash mismatch we then fall back to a binary search between the two
/// last known points to find the common block closest to the tip.
fn handle_ancestor_search_state<B: BlockT>(
	state: &AncestorSearchState<B>,
	curr_block_num: NumberFor<B>,
	block_hash_match: bool,
) -> Option<(AncestorSearchState<B>, NumberFor<B>)> {
	let two = <NumberFor<B>>::one() + <NumberFor<B>>::one();
	match state {
		AncestorSearchState::ExponentialBackoff(next_distance_to_tip) => {
			let next_distance_to_tip = *next_distance_to_tip;
			if block_hash_match && next_distance_to_tip == One::one() {
				// We found the ancestor in the first step so there is no need to execute binary
				// search.
				return None;
			}
			if block_hash_match {
				let left = curr_block_num;
				let right = left + next_distance_to_tip / two;
				let middle = left + (right - left) / two;
				Some((AncestorSearchState::BinarySearch(left, right), middle))
			} else {
				let next_block_num =
					curr_block_num.checked_sub(&next_distance_to_tip).unwrap_or_else(Zero::zero);
				let next_distance_to_tip = next_distance_to_tip * two;
				Some((
					AncestorSearchState::ExponentialBackoff(next_distance_to_tip),
					next_block_num,
				))
			}
		},
		AncestorSearchState::BinarySearch(mut left, mut right) => {
			if left >= curr_block_num {
				return None;
			}
			if block_hash_match {
				left = curr_block_num;
			} else {
				right = curr_block_num;
			}
			assert!(right >= left);
			let middle = left + (right - left) / two;
			if middle == curr_block_num {
				None
			} else {
				Some((AncestorSearchState::BinarySearch(left, right), middle))
			}
		},
	}
}

/// Get a new block request for the peer if any.
fn peer_block_request<B: BlockT>(
	id: &PeerId,
	peer: &PeerSync<B>,
	blocks: &mut BlockCollection<B>,
	attrs: BlockAttributes,
	max_parallel_downloads: u32,
	max_blocks_per_request: u32,
	finalized: NumberFor<B>,
	best_num: NumberFor<B>,
) -> Option<(Range<NumberFor<B>>, BlockRequest<B>)> {
	if best_num >= peer.best_number {
		// Will be downloaded as alternative fork instead.
		return None;
	} else if peer.common_number < finalized {
		trace!(
			target: LOG_TARGET,
			"Requesting pre-finalized chain from {:?}, common={}, finalized={}, peer best={}, our best={}",
			id, peer.common_number, finalized, peer.best_number, best_num,
		);
	}
	let range = blocks.needed_blocks(
		*id,
		max_blocks_per_request,
		peer.best_number,
		peer.common_number,
		max_parallel_downloads,
		MAX_DOWNLOAD_AHEAD,
	)?;

	// The end is not part of the range.
	let last = range.end.saturating_sub(One::one());

	let from = if peer.best_number == last {
		FromBlock::Hash(peer.best_hash)
	} else {
		FromBlock::Number(last)
	};

	let request = BlockRequest::<B> {
		id: 0,
		fields: attrs,
		from,
		direction: Direction::Descending,
		max: Some((range.end - range.start).saturated_into::<u32>()),
	};

	Some((range, request))
}

/// Get a new block request for the peer if any.
fn peer_gap_block_request<B: BlockT>(
	id: &PeerId,
	peer: &PeerSync<B>,
	blocks: &mut BlockCollection<B>,
	attrs: BlockAttributes,
	target: NumberFor<B>,
	common_number: NumberFor<B>,
	max_blocks_per_request: u32,
) -> Option<(Range<NumberFor<B>>, BlockRequest<B>)> {
	let range = blocks.needed_blocks(
		*id,
		max_blocks_per_request,
		std::cmp::min(peer.best_number, target),
		common_number,
		1,
		MAX_DOWNLOAD_AHEAD,
	)?;

	// The end is not part of the range.
	let last = range.end.saturating_sub(One::one());
	let from = FromBlock::Number(last);

	let request = BlockRequest::<B> {
		id: 0,
		fields: attrs,
		from,
		direction: Direction::Descending,
		max: Some((range.end - range.start).saturated_into::<u32>()),
	};
	Some((range, request))
}

/// Get pending fork sync targets for a peer.
fn fork_sync_request<B: BlockT>(
	id: &PeerId,
	fork_targets: &mut HashMap<B::Hash, ForkTarget<B>>,
	best_num: NumberFor<B>,
	finalized: NumberFor<B>,
	attributes: BlockAttributes,
	check_block: impl Fn(&B::Hash) -> BlockStatus,
	max_blocks_per_request: u32,
	metrics: Option<&Metrics>,
) -> Option<(B::Hash, BlockRequest<B>)> {
	fork_targets.retain(|hash, r| {
		if r.number <= finalized {
			trace!(
				target: LOG_TARGET,
				"Removed expired fork sync request {:?} (#{})",
				hash,
				r.number,
			);
			return false;
		}
		if check_block(hash) != BlockStatus::Unknown {
			trace!(
				target: LOG_TARGET,
				"Removed obsolete fork sync request {:?} (#{})",
				hash,
				r.number,
			);
			return false;
		}
		true
	});
	if let Some(metrics) = metrics {
		metrics.fork_targets.set(fork_targets.len().try_into().unwrap_or(u64::MAX));
	}
	for (hash, r) in fork_targets {
		if !r.peers.contains(&id) {
			continue;
		}
		// Download the fork only if it is behind or not too far ahead our tip of the chain
		// Otherwise it should be downloaded in full sync mode.
		if r.number <= best_num ||
			(r.number - best_num).saturated_into::<u32>() < max_blocks_per_request as u32
		{
			let parent_status = r.parent_hash.as_ref().map_or(BlockStatus::Unknown, check_block);
			let count = if parent_status == BlockStatus::Unknown {
				(r.number - finalized).saturated_into::<u32>() // up to the last finalized block
			} else {
				// request only single block
				1
			};
			trace!(
				target: LOG_TARGET,
				"Downloading requested fork {hash:?} from {id}, {count} blocks",
			);
			return Some((
				*hash,
				BlockRequest::<B> {
					id: 0,
					fields: attributes,
					from: FromBlock::Hash(*hash),
					direction: Direction::Descending,
					max: Some(count),
				},
			));
		} else {
			trace!(target: LOG_TARGET, "Fork too far in the future: {:?} (#{})", hash, r.number);
		}
	}
	None
}

/// Returns `true` if the given `block` is a descendent of `base`.
fn is_descendent_of<Block, T>(
	client: &T,
	base: &Block::Hash,
	block: &Block::Hash,
) -> sp_blockchain::Result<bool>
where
	Block: BlockT,
	T: HeaderMetadata<Block, Error = sp_blockchain::Error> + ?Sized,
{
	if base == block {
		return Ok(false);
	}

	let ancestor = sp_blockchain::lowest_common_ancestor(client, *block, *base)?;

	Ok(ancestor.hash == *base)
}

/// `true` when `body` is the extrinsic list committed by `header.extrinsics_root`.
///
/// The runtime that built the block chose the trie layout of the root from its
/// `system_version` (`RuntimeVersion::extrinsics_root_state_version`), which sync does not
/// know. A body is accepted under either layout, so a runtime upgrade that changes the layout
/// does not make every node refuse every block. A body that matches only under the layout the
/// runtime does not use fails when the block is executed.
pub(crate) fn extrinsics_match_header<Block: BlockT>(
	header: &Block::Header,
	body: &[Block::Extrinsic],
) -> bool {
	[StateVersion::V0, StateVersion::V1].into_iter().any(|layout| {
		let root = <<Block::Header as HeaderT>::Hashing as HashT>::ordered_trie_root(
			body.iter().map(Encode::encode).collect(),
			layout,
		);
		header.extrinsics_root() == &root
	})
}

/// Validate that the given `blocks` are correct.
/// Returns the number of the first block in the sequence.
///
/// It is expected that `blocks` are in ascending order.
pub fn validate_blocks<Block: BlockT>(
	blocks: &Vec<BlockData<Block>>,
	peer_id: &PeerId,
	request: Option<BlockRequest<Block>>,
) -> Result<Option<NumberFor<Block>>, BadPeer> {
	if let Some(request) = request {
		if Some(blocks.len() as _) > request.max {
			debug!(
				target: LOG_TARGET,
				"Received more blocks than requested from {}. Expected in maximum {:?}, got {}.",
				peer_id,
				request.max,
				blocks.len(),
			);

			return Err(BadPeer(*peer_id, rep::NOT_REQUESTED));
		}

		let block_header =
			if request.direction == Direction::Descending { blocks.last() } else { blocks.first() }
				.and_then(|b| b.header.as_ref());

		let expected_block = block_header.as_ref().map_or(false, |h| match request.from {
			FromBlock::Hash(hash) => h.hash() == hash,
			FromBlock::Number(n) => h.number() == &n,
		});

		if !expected_block {
			debug!(
				target: LOG_TARGET,
				"Received block that was not requested. Requested {:?}, got {:?}.",
				request.from,
				block_header,
			);

			return Err(BadPeer(*peer_id, rep::NOT_REQUESTED));
		}

		if request.fields.contains(BlockAttributes::HEADER) &&
			blocks.iter().any(|b| b.header.is_none())
		{
			trace!(
				target: LOG_TARGET,
				"Missing requested header for a block in response from {peer_id}.",
			);

			return Err(BadPeer(*peer_id, rep::BAD_RESPONSE));
		}

		if request.fields.contains(BlockAttributes::BODY) && blocks.iter().any(|b| b.body.is_none())
		{
			trace!(
				target: LOG_TARGET,
				"Missing requested body for a block in response from {peer_id}.",
			);

			return Err(BadPeer(*peer_id, rep::BAD_RESPONSE));
		}

		// Callers put `blocks` in ascending order before validating (descending responses are
		// reversed), so every block must directly extend the one before it. Without this the
		// peer could attach arbitrary blocks below the one block checked above.
		for pair in blocks.windows(2) {
			let (Some(prev), Some(next)) = (pair[0].header.as_ref(), pair[1].header.as_ref())
			else {
				continue
			};

			if prev.number().checked_add(&One::one()) != Some(*next.number()) ||
				*next.parent_hash() != prev.hash()
			{
				debug!(
					target: LOG_TARGET,
					"Non-contiguous blocks in response from {}: block {} ({:?}) does not extend block {} ({:?}).",
					peer_id,
					next.number(),
					next.hash(),
					prev.number(),
					prev.hash(),
				);

				return Err(BadPeer(*peer_id, rep::BAD_BLOCK));
			}
		}
	}

	for b in blocks {
		if let Some(header) = &b.header {
			let hash = header.hash();
			if hash != b.hash {
				debug!(
					target: LOG_TARGET,
					"Bad header received from {}. Expected hash {:?}, got {:?}",
					peer_id,
					b.hash,
					hash,
				);
				return Err(BadPeer(*peer_id, rep::BAD_BLOCK));
			}
			if let Some(body) = &b.body {
				if !extrinsics_match_header::<Block>(header, body) {
					debug!(
						target: LOG_TARGET,
						"Body from {peer_id} does not match the header extrinsics root of {hash:?}.",
					);
					return Err(BadPeer(*peer_id, rep::BAD_BLOCK));
				}
			}
		}
	}

	Ok(blocks.first().and_then(|b| b.header.as_ref()).map(|h| *h.number()))
}

/// Ensure a validated response is filed inside the block range that was requested from the peer.
///
/// `validate_blocks` pins the block at `request.from`, but when the request is by hash the number
/// of that block comes from the peer. Checking the derived start against the requested range keeps
/// a peer from filing a response under an arbitrary number that would never be imported or freed.
fn ensure_response_within_request<B: BlockT>(
	start_block: NumberFor<B>,
	len: usize,
	range_start: NumberFor<B>,
	max_blocks: Option<u32>,
	peer_id: &PeerId,
) -> Result<(), BadPeer> {
	let len: NumberFor<B> = (len as u32).into();
	let range_end = max_blocks.map(|max| range_start.saturating_add(max.into()));
	let response_end = start_block.saturating_add(len);

	if start_block < range_start || range_end.map_or(false, |end| response_end > end) {
		debug!(
			target: LOG_TARGET,
			"Block response from {} outside requested range: got {}..{}, requested {}..{:?}.",
			peer_id,
			start_block,
			response_end,
			range_start,
			range_end,
		);

		return Err(BadPeer(*peer_id, rep::NOT_REQUESTED));
	}

	Ok(())
}

#[cfg(test)]
mod security_tests {
	use super::*;
	use crate::mock::MockBlockDownloader;
	use sc_client_api::{ChildInfo, CompactProof, StorageProof};
	use sc_consensus::{
		BasicQueue, BlockCheckParams, BlockImport, BlockImportParams, ImportQueue, ImportResult,
		Link, Verifier,
	};
	use sp_blockchain::{CachedHeaderMetadata, Info};
	use sp_core::H256;
	use sp_runtime::{
		testing::{Block as RawBlock, MockCallU64, TestXt},
		traits::BlakeTwo256,
	};
	use sp_state_machine::{KeyValueStates, KeyValueStorageLevel};

	type TestBlock = RawBlock<TestXt<MockCallU64, ()>>;

	/// A client stuck at genesis, so anything the sync strategy could learn
	/// about being behind can only come from peer claims.
	struct GenesisClient {
		genesis: <TestBlock as BlockT>::Header,
		best_number: u64,
	}

	impl GenesisClient {
		fn new() -> Self {
			Self {
				best_number: 0,
				genesis: <<TestBlock as BlockT>::Header as HeaderT>::new(
					0,
					Default::default(),
					Default::default(),
					Default::default(),
					Default::default(),
				),
			}
		}
	}

	impl HeaderBackend<TestBlock> for GenesisClient {
		fn header(
			&self,
			hash: H256,
		) -> sp_blockchain::Result<Option<<TestBlock as BlockT>::Header>> {
			Ok((hash == self.genesis.hash()).then(|| self.genesis.clone()))
		}

		fn info(&self) -> Info<TestBlock> {
			Info {
				best_hash: self.genesis.hash(),
				best_number: self.best_number,
				genesis_hash: self.genesis.hash(),
				finalized_hash: self.genesis.hash(),
				finalized_number: 0,
				finalized_state: Some((self.genesis.hash(), 0)),
				number_leaves: 1,
				block_gap: None,
			}
		}

		fn status(&self, hash: H256) -> sp_blockchain::Result<sp_blockchain::BlockStatus> {
			Ok(if hash == self.genesis.hash() {
				sp_blockchain::BlockStatus::InChain
			} else {
				sp_blockchain::BlockStatus::Unknown
			})
		}

		fn number(&self, hash: H256) -> sp_blockchain::Result<Option<u64>> {
			Ok((hash == self.genesis.hash()).then_some(0))
		}

		fn hash(&self, number: u64) -> sp_blockchain::Result<Option<H256>> {
			Ok((number == 0).then(|| self.genesis.hash()))
		}
	}

	impl HeaderMetadata<TestBlock> for GenesisClient {
		type Error = ClientError;

		fn header_metadata(
			&self,
			hash: H256,
		) -> Result<CachedHeaderMetadata<TestBlock>, ClientError> {
			if hash == self.genesis.hash() {
				Ok(CachedHeaderMetadata::from(&self.genesis))
			} else {
				Err(ClientError::UnknownBlock(format!("{hash:?}")))
			}
		}

		fn insert_header_metadata(&self, _hash: H256, _metadata: CachedHeaderMetadata<TestBlock>) {}

		fn remove_header_metadata(&self, _hash: H256) {}
	}

	impl BlockBackend<TestBlock> for GenesisClient {
		fn block_body(
			&self,
			_hash: H256,
		) -> sp_blockchain::Result<Option<Vec<<TestBlock as BlockT>::Extrinsic>>> {
			Ok(None)
		}

		fn block_indexed_body(&self, _hash: H256) -> sp_blockchain::Result<Option<Vec<Vec<u8>>>> {
			Ok(None)
		}

		fn block(
			&self,
			_hash: H256,
		) -> sp_blockchain::Result<Option<sp_runtime::generic::SignedBlock<TestBlock>>> {
			Ok(None)
		}

		fn block_status(&self, hash: H256) -> sp_blockchain::Result<BlockStatus> {
			Ok(if hash == self.genesis.hash() {
				BlockStatus::InChainWithState
			} else {
				BlockStatus::Unknown
			})
		}

		fn justifications(&self, _hash: H256) -> sp_blockchain::Result<Option<Justifications>> {
			Ok(None)
		}

		fn block_hash(&self, number: u64) -> sp_blockchain::Result<Option<H256>> {
			Ok((number == 0).then(|| self.genesis.hash()))
		}

		fn indexed_transaction(&self, _hash: H256) -> sp_blockchain::Result<Option<Vec<u8>>> {
			Ok(None)
		}

		fn requires_full_sync(&self) -> bool {
			false
		}
	}

	impl ProofProvider<TestBlock> for GenesisClient {
		fn read_proof(
			&self,
			_hash: H256,
			_keys: &mut dyn Iterator<Item = &[u8]>,
		) -> sp_blockchain::Result<StorageProof> {
			unimplemented!()
		}

		fn read_child_proof(
			&self,
			_hash: H256,
			_child_info: &ChildInfo,
			_keys: &mut dyn Iterator<Item = &[u8]>,
		) -> sp_blockchain::Result<StorageProof> {
			unimplemented!()
		}

		fn execution_proof(
			&self,
			_hash: H256,
			_method: &str,
			_call_data: &[u8],
		) -> sp_blockchain::Result<(Vec<u8>, StorageProof)> {
			unimplemented!()
		}

		fn read_proof_collection(
			&self,
			_hash: H256,
			_start_keys: &[Vec<u8>],
			_size_limit: usize,
		) -> sp_blockchain::Result<(CompactProof, u32)> {
			unimplemented!()
		}

		fn storage_collection(
			&self,
			_hash: H256,
			_start_key: &[Vec<u8>],
			_size_limit: usize,
		) -> sp_blockchain::Result<Vec<(KeyValueStorageLevel, bool)>> {
			unimplemented!()
		}

		fn verify_range_proof(
			&self,
			_root: H256,
			_proof: CompactProof,
			_start_keys: &[Vec<u8>],
		) -> sp_blockchain::Result<(KeyValueStates, usize)> {
			unimplemented!()
		}
	}

	/// Active regression for the forged-handshake report: peers claiming an
	/// arbitrarily high best block must never flip the node into major sync;
	/// only the local import backlog can (Bitcoin-style IBD).
	#[test]
	fn forged_handshake_height_does_not_trigger_major_sync() {
		let client = Arc::new(GenesisClient::new());
		let mut sync = ChainSync::new(
			ChainSyncMode::Full,
			client,
			1,
			64,
			ProtocolName::Static(""),
			Arc::new(MockBlockDownloader::new()),
			None,
			std::iter::empty(),
		)
		.unwrap();

		for _ in 0..3 {
			sync.add_peer(PeerId::random(), H256::random(), 1_000_000);
		}

		assert_eq!(sync.peers.len(), 3, "forged peers must still be registered");
		assert!(!sync.is_major_syncing());
		assert_eq!(sync.status().state, SyncState::Idle);
		// The claim IS allowed to surface in the display/monitoring field;
		// the security property is that gating state above stays untouched.
		assert_eq!(sync.status().best_seen_block, Some(1_000_000));
	}

	/// The median keeps a single liar from dominating the displayed target.
	#[test]
	fn displayed_height_is_the_median_peer_claim() {
		let client = Arc::new(GenesisClient::new());
		let mut sync = ChainSync::new(
			ChainSyncMode::Full,
			client,
			1,
			64,
			ProtocolName::Static(""),
			Arc::new(MockBlockDownloader::new()),
			None,
			std::iter::empty(),
		)
		.unwrap();

		assert_eq!(sync.median_claimed_height(), None);
		sync.add_peer(PeerId::random(), H256::random(), 10);
		sync.add_peer(PeerId::random(), H256::random(), 12);
		sync.add_peer(PeerId::random(), H256::random(), 1_000_000);
		assert_eq!(sync.median_claimed_height(), Some(12));
		assert_eq!(sync.status().best_seen_block, Some(12));
	}

	#[test]
	fn transport_tolerance_does_not_wait_for_verified_sync() {
		assert!(catch_up_request_is_gated(100u64, 0, true));
		assert!(!catch_up_request_is_gated(100u64, 0, false));
		assert!(!catch_up_request_is_gated(0u64, 0, true));
	}

	#[test]
	fn notifications_are_only_suppressed_during_normal_sync_backlog() {
		assert_eq!(block_import_origin(false, false, true), BlockOrigin::NetworkBroadcast);
		assert_eq!(block_import_origin(false, true, false), BlockOrigin::NetworkBroadcast);
		assert_eq!(block_import_origin(false, true, true), BlockOrigin::NetworkInitialSync);
		assert_eq!(block_import_origin(true, false, false), BlockOrigin::NetworkInitialSync);
	}

	/// A block with an empty body. The default extrinsics root of the header is not the
	/// root of an empty list, so it is set here.
	fn block_data(mut header: <TestBlock as BlockT>::Header) -> BlockData<TestBlock> {
		header.set_extrinsics_root(BlakeTwo256::ordered_trie_root(Vec::new(), StateVersion::V0));
		BlockData::<TestBlock> {
			hash: header.hash(),
			header: Some(header),
			body: Some(Vec::new()),
			indexed_body: None,
			receipt: None,
			message_queue: None,
			justification: None,
			justifications: None,
		}
	}

	fn header(number: u64, parent_hash: H256) -> <TestBlock as BlockT>::Header {
		<<TestBlock as BlockT>::Header as HeaderT>::new(
			number,
			Default::default(),
			Default::default(),
			parent_hash,
			Default::default(),
		)
	}

	/// `len` chained blocks ending at `top`, lowest first, which is how `on_block_data` orders a
	/// descending response before validating it.
	fn chained_blocks(top: u64, len: u64) -> Vec<BlockData<TestBlock>> {
		let mut parent_hash = H256::random();
		(top + 1 - len..=top)
			.map(|number| {
				let data = block_data(header(number, parent_hash));
				parent_hash = data.hash;
				data
			})
			.collect()
	}

	fn descending_request(from: u64, max: u32) -> BlockRequest<TestBlock> {
		BlockRequest::<TestBlock> {
			id: 0,
			fields: BlockAttributes::HEADER | BlockAttributes::BODY,
			from: FromBlock::Number(from),
			direction: Direction::Descending,
			max: Some(max),
		}
	}

	#[test]
	fn contiguous_response_is_filed_under_its_lowest_block() {
		let peer = PeerId::random();
		let blocks = chained_blocks(10, 5);

		assert_eq!(
			validate_blocks::<TestBlock>(&blocks, &peer, Some(descending_request(10, 5))),
			Ok(Some(6)),
		);
	}

	/// Only the block at `request.from` is pinned by the request. A peer must not be able to
	/// attach a block with an arbitrary number below it and have the response filed there.
	#[test]
	fn response_whose_lowest_block_is_forged_is_rejected() {
		let peer = PeerId::random();
		let mut blocks = chained_blocks(10, 5);
		blocks[0] = block_data(header(1 << 31, H256::random()));

		assert_eq!(
			validate_blocks::<TestBlock>(&blocks, &peer, Some(descending_request(10, 5))),
			Err(BadPeer(peer, rep::BAD_BLOCK)),
		);
	}

	#[test]
	fn response_with_broken_parent_link_is_rejected() {
		let peer = PeerId::random();
		let mut blocks = chained_blocks(10, 5);
		blocks[2] = block_data(header(8, H256::random()));

		assert_eq!(
			validate_blocks::<TestBlock>(&blocks, &peer, Some(descending_request(10, 5))),
			Err(BadPeer(peer, rep::BAD_BLOCK)),
		);
	}

	/// The contiguity check must not overflow on a peer-supplied block number at the type's
	/// maximum.
	#[test]
	fn response_with_max_block_number_is_rejected_without_panicking() {
		let peer = PeerId::random();
		let mut blocks = chained_blocks(10, 5);
		blocks[0] = block_data(header(u64::MAX, H256::random()));

		assert_eq!(
			validate_blocks::<TestBlock>(&blocks, &peer, Some(descending_request(10, 5))),
			Err(BadPeer(peer, rep::BAD_BLOCK)),
		);
	}

	#[test]
	fn response_whose_body_does_not_match_extrinsics_root_is_rejected() {
		let peer = PeerId::random();
		let mut blocks = chained_blocks(10, 1);
		blocks[0].body = Some(vec![TestXt::new_bare(MockCallU64(1))]);

		assert_eq!(
			validate_blocks::<TestBlock>(&blocks, &peer, Some(descending_request(10, 1))),
			Err(BadPeer(peer, rep::BAD_BLOCK)),
		);
	}

	/// The runtime chooses the trie layout of the extrinsics root, and a runtime upgrade can
	/// change it. Sync must keep accepting honest bodies across that change.
	#[test]
	fn body_is_matched_against_the_header_under_either_trie_layout() {
		type OpaqueBlock = sp_runtime::generic::Block<
			sp_runtime::generic::Header<u64, BlakeTwo256>,
			sp_runtime::OpaqueExtrinsic,
		>;
		let header_committing_to = |body: &[sp_runtime::OpaqueExtrinsic], layout| {
			let root =
				BlakeTwo256::ordered_trie_root(body.iter().map(Encode::encode).collect(), layout);
			<OpaqueBlock as BlockT>::Header::new(
				1,
				root,
				Default::default(),
				Default::default(),
				Default::default(),
			)
		};

		// The layouts only produce different roots for values of 33 bytes or more, the size
		// of any signed extrinsic.
		let body = vec![sp_runtime::OpaqueExtrinsic::from_blob(vec![7; 64])];
		let v0_header = header_committing_to(&body, StateVersion::V0);
		let v1_header = header_committing_to(&body, StateVersion::V1);
		assert_ne!(v0_header.extrinsics_root(), v1_header.extrinsics_root());

		assert!(extrinsics_match_header::<OpaqueBlock>(&v0_header, &body));
		assert!(extrinsics_match_header::<OpaqueBlock>(&v1_header, &body));

		let other_body = vec![sp_runtime::OpaqueExtrinsic::from_blob(vec![8; 64])];
		assert!(!extrinsics_match_header::<OpaqueBlock>(&v0_header, &other_body));
		assert!(!extrinsics_match_header::<OpaqueBlock>(&v1_header, &other_body));
	}

	fn idle_sync() -> (Arc<GenesisClient>, ChainSync<TestBlock, GenesisClient>) {
		let client = Arc::new(GenesisClient::new());
		let sync = ChainSync::new(
			ChainSyncMode::Full,
			client.clone(),
			1,
			64,
			ProtocolName::Static(""),
			Arc::new(MockBlockDownloader::new()),
			None,
			std::iter::empty(),
		)
		.unwrap();
		(client, sync)
	}

	struct AcceptingVerifier;

	#[async_trait::async_trait]
	impl Verifier<TestBlock> for AcceptingVerifier {
		async fn verify(
			&self,
			block: BlockImportParams<TestBlock>,
		) -> Result<BlockImportParams<TestBlock>, String> {
			Ok(block)
		}
	}

	/// Block import that passes `check_block` and fails every `import_block` with `error`.
	struct FailingImport {
		error: fn() -> sp_consensus::Error,
	}

	#[async_trait::async_trait]
	impl BlockImport<TestBlock> for FailingImport {
		type Error = sp_consensus::Error;

		async fn check_block(
			&self,
			_block: BlockCheckParams<TestBlock>,
		) -> Result<ImportResult, Self::Error> {
			Ok(ImportResult::imported(false))
		}

		async fn import_block(
			&self,
			_block: BlockImportParams<TestBlock>,
		) -> Result<ImportResult, Self::Error> {
			Err((self.error)())
		}
	}

	type ImportResults = Vec<(Result<BlockImportStatus<u64>, BlockImportError>, H256)>;

	#[derive(Default)]
	struct RecordingLink {
		processed: std::sync::Mutex<Option<(usize, usize, ImportResults)>>,
	}

	impl Link<TestBlock> for RecordingLink {
		fn blocks_processed(&self, imported: usize, count: usize, results: ImportResults) {
			*self.processed.lock().unwrap() = Some((imported, count, results));
		}
	}

	/// A peer sends block 1. The block goes through the real import queue, whose block import
	/// fails with `error`, and the queue's result is handed back to sync. Returns the peer and
	/// sync, which holds the actions it took.
	fn sync_after_failed_import(
		error: fn() -> sp_consensus::Error,
	) -> (PeerId, ChainSync<TestBlock, GenesisClient>) {
		let (client, mut sync) = idle_sync();
		let serving = PeerId::random();
		sync.add_peer(serving, client.genesis.hash(), 0);
		sync.actions.clear();

		let block = block_data(header(1, client.genesis.hash()));
		sync.on_block_data(
			&serving,
			None,
			BlockResponse::<TestBlock> { id: 0, blocks: vec![block] },
		)
		.unwrap();
		let (origin, blocks) = std::mem::take(&mut sync.actions)
			.into_iter()
			.find_map(|action| match action {
				SyncingAction::ImportBlocks { origin, blocks } => Some((origin, blocks)),
				_ => None,
			})
			.expect("the block is queued for import");

		let mut queue = BasicQueue::new(
			AcceptingVerifier,
			Box::new(FailingImport { error }),
			None,
			&sp_core::testing::TaskExecutor::new(),
			None,
		);
		queue.service().import_blocks(origin, blocks);
		let link = RecordingLink::default();
		let (imported, count, results) =
			futures::executor::block_on(futures::future::poll_fn(|cx| {
				queue.poll_actions(cx, &link);
				match link.processed.lock().unwrap().take() {
					Some(processed) => std::task::Poll::Ready(processed),
					None => std::task::Poll::Pending,
				}
			}));

		sync.on_blocks_processed(imported, count, results);
		assert!(sync.queue_blocks.is_empty());
		(serving, sync)
	}

	fn drops_a_peer(sync: &ChainSync<TestBlock, GenesisClient>) -> bool {
		sync.actions.iter().any(|action| matches!(action, SyncingAction::DropPeer(_)))
	}

	#[test]
	fn block_reported_invalid_by_import_drops_the_sender() {
		let (serving, sync) = sync_after_failed_import(|| {
			sp_consensus::Error::Other(Box::new(InvalidBlock::new("execution failed")))
		});

		assert!(sync.actions.iter().any(|action| matches!(
			action,
			SyncingAction::DropPeer(BadPeer(peer, change))
				if *peer == serving && *change == rep::BAD_BLOCK
		)));
	}

	#[test]
	fn block_from_the_future_pauses_the_sender_without_dropping_it() {
		let before = Instant::now();
		let (serving, sync) =
			sync_after_failed_import(|| sp_consensus::Error::Other(Box::new(FutureBlock)));

		assert!(!drops_a_peer(&sync));
		assert!(sync.peers.contains_key(&serving));
		assert!(sync.future_block_peers.is_paused(&serving, before));
	}

	#[test]
	fn paused_peer_is_not_asked_for_blocks() {
		let asked_for_blocks = |paused: bool| {
			let (_client, mut sync) = idle_sync();
			let serving = PeerId::random();
			sync.add_peer(serving, H256::repeat_byte(9), 1);
			if paused {
				// Repeat until the pause is far longer than this test can take.
				for _ in 0..6 {
					sync.future_block_peers.pause(serving, Instant::now());
				}
			}
			sync.block_requests().iter().any(|(peer, _)| *peer == serving)
		};

		assert!(asked_for_blocks(false));
		assert!(!asked_for_blocks(true));
	}

	#[test]
	fn local_import_failure_does_not_drop_the_sender() {
		let before = Instant::now();
		let (serving, sync) =
			sync_after_failed_import(|| sp_consensus::Error::ClientImport("database error".into()));

		assert!(!drops_a_peer(&sync));
		assert!(!sync.future_block_peers.is_paused(&serving, before));
	}

	/// `Other` is also what `?` makes of any boxed error. Only the marker types name the block.
	#[test]
	fn unrelated_boxed_import_error_does_not_drop_the_sender() {
		let before = Instant::now();
		let (serving, sync) = sync_after_failed_import(|| {
			sp_consensus::Error::Other("inherent data provider failed".into())
		});

		assert!(!drops_a_peer(&sync));
		assert!(!sync.future_block_peers.is_paused(&serving, before));
	}

	#[test]
	fn unknown_parent_does_not_drop_the_sender() {
		let (client, mut sync) = idle_sync();
		let genesis = client.genesis.hash();
		let serving = PeerId::random();
		sync.add_peer(serving, genesis, 0);
		let target = header(1, genesis).hash();
		sync.queue_blocks
			.insert(target, QueuedBlock { origin: Some(serving), fork_target: None });
		sync.actions.clear();

		sync.on_blocks_processed(0, 1, vec![(Err(BlockImportError::UnknownParent), target)]);

		assert!(sync.actions.iter().all(|action| !matches!(action, SyncingAction::DropPeer(_))));
	}

	/// A block the peer announces without making it its best block is requested through a
	/// fork target, which is dropped once the block is queued. When the peer's best block is
	/// already known here, nothing else leads back to the announced block.
	#[test]
	fn future_block_on_a_side_fork_is_requested_again_after_the_pause() {
		let (client, mut sync) = idle_sync();
		let genesis = client.genesis.hash();
		let announcing = PeerId::random();
		sync.add_peer(announcing, genesis, 0);

		let block = block_data(header(1, genesis));
		let hash = block.hash;
		let header = block.header.clone().expect("the block has a header");
		let announce = BlockAnnounce { header, state: None, data: None };
		let _ = sync.on_validated_block_announce(false, announcing, &announce);

		let request_for_the_block = |sync: &mut ChainSync<TestBlock, GenesisClient>| {
			sync.block_requests()
				.into_iter()
				.find(|(peer, request)| {
					*peer == announcing && request.from == FromBlock::Hash(hash)
				})
				.map(|(_, request)| request)
		};
		let request = request_for_the_block(&mut sync).expect("the announced block is requested");
		sync.on_block_data(
			&announcing,
			Some(request),
			BlockResponse::<TestBlock> { id: 0, blocks: vec![block] },
		)
		.unwrap();
		assert!(sync.queue_blocks.contains_key(&hash));

		let future_block = sp_consensus::Error::Other(Box::new(FutureBlock));
		sync.on_blocks_processed(0, 1, vec![(Err(BlockImportError::Other(future_block)), hash)]);
		assert!(request_for_the_block(&mut sync).is_none(), "the peer is paused");

		// The pause ends. No second announcement arrives.
		sync.future_block_peers = FutureBlockPeers::new();
		assert!(request_for_the_block(&mut sync).is_some());
	}

	/// A fork target is the last block of its batch, and the queue does not try the blocks
	/// behind a failed one.
	#[test]
	fn fork_target_behind_a_future_block_is_kept() {
		let kept_after = |error: fn() -> sp_consensus::Error| {
			let (client, mut sync) = idle_sync();
			let peer = PeerId::random();
			sync.add_peer(peer, client.genesis.hash(), 0);
			let (parent, target) = (H256::repeat_byte(1), H256::repeat_byte(2));
			let fork_target =
				ForkTarget { number: 2, parent_hash: Some(parent), peers: [peer].into() };
			sync.queue_blocks
				.insert(parent, QueuedBlock { origin: Some(peer), fork_target: None });
			sync.queue_blocks
				.insert(target, QueuedBlock { origin: Some(peer), fork_target: Some(fork_target) });

			sync.on_blocks_processed(
				0,
				2,
				vec![
					(Err(BlockImportError::Other(error())), parent),
					(Err(BlockImportError::Cancelled), target),
				],
			);
			sync.fork_targets.contains_key(&target)
		};

		assert!(kept_after(|| sp_consensus::Error::Other(Box::new(FutureBlock))));
		assert!(!kept_after(|| {
			sp_consensus::Error::Other(Box::new(InvalidBlock::new("execution failed")))
		}));
	}

	#[test]
	fn response_must_be_filed_inside_the_requested_range() {
		let peer = PeerId::random();
		let within =
			|start, len| ensure_response_within_request::<TestBlock>(start, len, 6, Some(5), &peer);

		assert_eq!(within(6, 5), Ok(()));
		assert_eq!(within(9, 2), Ok(()));
		assert_eq!(within(5, 5), Err(BadPeer(peer, rep::NOT_REQUESTED)));
		assert_eq!(within(8, 5), Err(BadPeer(peer, rep::NOT_REQUESTED)));
		assert_eq!(within(1 << 31, 5), Err(BadPeer(peer, rep::NOT_REQUESTED)));
	}
	#[test]
	fn empty_unverified_state_response_is_rejected() {
		use crate::{
			schema::v1::StateResponse,
			strategy::state_sync::{ImportResult, StateSync, StateSyncProvider},
		};
		let client = Arc::new(GenesisClient::new());
		let mut state_sync = StateSync::<TestBlock, _>::new(
			client.clone(),
			client.genesis.clone(),
			None,
			None,
			true,
		);
		assert!(matches!(
			state_sync.import(StateResponse { entries: Vec::new(), proof: vec![1] }),
			ImportResult::BadResponse
		));
		assert!(!state_sync.is_complete());
		assert!(state_sync.next_request().start.is_empty());
	}

	#[test]
	fn unknown_fork_announcement_starts_ancestry_search() {
		let mut client = GenesisClient::new();
		client.best_number = 20;
		let client = Arc::new(client);
		let mut sync = ChainSync::new(
			ChainSyncMode::Full,
			client,
			1,
			64,
			ProtocolName::Static(""),
			Arc::new(MockBlockDownloader::new()),
			None,
			std::iter::empty(),
		)
		.unwrap();
		sync.best_queued_number = 20;
		sync.best_queued_hash = H256::repeat_byte(20);
		let peer_id = PeerId::random();
		sync.add_peer(peer_id, sync.best_queued_hash, 20);
		let peer = sync.peers.get_mut(&peer_id).unwrap();
		peer.state = PeerSyncState::DownloadingNew(20);
		peer.common_number = 20;
		sync.actions.clear();
		let header = <<TestBlock as BlockT>::Header as HeaderT>::new(
			21,
			Default::default(),
			Default::default(),
			H256::repeat_byte(99),
			Default::default(),
		);
		let announce = BlockAnnounce { header, state: None, data: None };
		sync.on_validated_block_announce(true, peer_id, &announce);
		let peer = sync.peers.get(&peer_id).unwrap();
		assert!(matches!(peer.state, PeerSyncState::AncestorSearch { current: 20, .. }));
		assert_eq!(peer.common_number, 0);
		assert!(
			matches!(sync.actions.first(), Some(SyncingAction::CancelRequest { peer_id: id, .. }) if *id == peer_id)
		);
	}
	#[test]
	fn reset_sync_drops_a_gap_already_closed_by_the_database() {
		let client = Arc::new(GenesisClient::new());
		let mut sync = ChainSync::new(
			ChainSyncMode::Full,
			client,
			1,
			64,
			ProtocolName::Static(""),
			Arc::new(MockBlockDownloader::new()),
			None,
			std::iter::empty(),
		)
		.unwrap();
		sync.gap_sync =
			Some(GapSync { blocks: BlockCollection::new(), best_queued_number: 1, target: 10 });
		sync.reset_sync_start_point().unwrap();
		assert!(sync.gap_sync.is_none());
	}
}
