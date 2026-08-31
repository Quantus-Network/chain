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

//! Transactions handling to plug on top of the network service.
//!
//! Usage:
//!
//! - Use [`TransactionsHandlerPrototype::new`] to create a prototype.
//! - Pass the `NonDefaultSetConfig` returned from [`TransactionsHandlerPrototype::new`] to the
//!   network configuration as an extra peers set.
//! - Use [`TransactionsHandlerPrototype::build`] then [`TransactionsHandler::run`] to obtain a
//! `Future` that processes transactions.

use crate::config::*;

use codec::{Decode, Encode};
use futures::{prelude::*, stream::FuturesUnordered};
use log::{debug, trace, warn};

use prometheus_endpoint::{register, Counter, PrometheusError, Registry, U64};
use sc_network::{
	config::{NonReservedPeerMode, ProtocolId, SetConfig},
	error, multiaddr,
	peer_store::PeerStoreProvider,
	service::{
		traits::{NotificationEvent, NotificationService, ValidationResult},
		NotificationMetrics,
	},
	types::ProtocolName,
	utils::{interval, LruHashSet},
	NetworkBackend, NetworkEventStream, NetworkPeers,
};
use sc_network_common::{role::ObservedRole, ExHashT};
use sc_network_sync::{SyncEvent, SyncEventStream};
use sc_network_types::PeerId;
use sc_utils::mpsc::{tracing_unbounded, TracingUnboundedReceiver, TracingUnboundedSender};
use sp_runtime::traits::Block as BlockT;

use std::{
	collections::{hash_map::Entry, HashMap},
	hash::Hash,
	iter,
	num::NonZeroUsize,
	pin::Pin,
	sync::Arc,
	task::Poll,
};

pub mod config;

/// A set of transactions.
pub type Transactions<E> = Vec<E>;

/// Logging target for the file.
const LOG_TARGET: &str = "sync";

mod rep {
	use sc_network::ReputationChange as Rep;
	/// Reputation change when a peer sends us any transaction.
	///
	/// This forces node to verify it, thus the negative value here. Once transaction is verified,
	/// reputation change should be refunded with `ANY_TRANSACTION_REFUND`
	pub const ANY_TRANSACTION: Rep = Rep::new(-(1 << 4), "Any transaction");
	/// Reputation change when a peer sends us any transaction that is not invalid.
	pub const ANY_TRANSACTION_REFUND: Rep = Rep::new(1 << 4, "Any transaction (refund)");
	/// Reputation change when a peer sends us an transaction that we didn't know about.
	pub const GOOD_TRANSACTION: Rep = Rep::new(1 << 7, "Good transaction");
	/// Reputation change when a peer sends us a bad transaction.
	pub const BAD_TRANSACTION: Rep = Rep::new(-(1 << 12), "Bad transaction");
}

struct Metrics {
	propagated_transactions: Counter<U64>,
}

impl Metrics {
	fn register(r: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			propagated_transactions: register(
				Counter::new(
					"substrate_sync_propagated_transactions",
					"Number of transactions propagated to at least one peer",
				)?,
				r,
			)?,
		})
	}
}

struct PendingTransaction<H> {
	validation: TransactionImportFuture,
	tx_hash: H,
}

impl<H> Unpin for PendingTransaction<H> {}

impl<H: ExHashT> Future for PendingTransaction<H> {
	type Output = (H, TransactionImport);

	fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
		if let Poll::Ready(import_result) = self.validation.poll_unpin(cx) {
			return Poll::Ready((self.tx_hash.clone(), import_result))
		}

		Poll::Pending
	}
}

/// Prototype for a [`TransactionsHandler`].
pub struct TransactionsHandlerPrototype {
	/// Name of the transaction protocol.
	protocol_name: ProtocolName,

	/// Handle that is used to communicate with `sc_network::Notifications`.
	notification_service: Box<dyn NotificationService>,

	/// Per-peer known-transaction cache capacity. Callers should set this to the ready-pool
	/// limit so gossip cannot all-miss a full pool.
	max_known_transactions: NonZeroUsize,
}

impl TransactionsHandlerPrototype {
	/// Create a new instance.
	///
	/// `max_known_transactions` is the per-peer known-hash cache. Pass the ready-pool limit so a
	/// full pool cannot overflow the cache.
	pub fn new<
		Hash: AsRef<[u8]>,
		Block: BlockT,
		Net: NetworkBackend<Block, <Block as BlockT>::Hash>,
	>(
		protocol_id: ProtocolId,
		genesis_hash: Hash,
		fork_id: Option<&str>,
		metrics: NotificationMetrics,
		peer_store_handle: Arc<dyn PeerStoreProvider>,
		max_known_transactions: NonZeroUsize,
	) -> (Self, Net::NotificationProtocolConfig) {
		let genesis_hash = genesis_hash.as_ref();
		let protocol_name: ProtocolName = if let Some(fork_id) = fork_id {
			format!("/{}/{}/transactions/1", array_bytes::bytes2hex("", genesis_hash), fork_id)
		} else {
			format!("/{}/transactions/1", array_bytes::bytes2hex("", genesis_hash))
		}
		.into();
		let (config, notification_service) = Net::notification_config(
			protocol_name.clone(),
			vec![format!("/{}/transactions/1", protocol_id.as_ref()).into()],
			MAX_TRANSACTIONS_SIZE,
			None,
			SetConfig {
				in_peers: 0,
				out_peers: 0,
				reserved_nodes: Vec::new(),
				non_reserved_mode: NonReservedPeerMode::Deny,
			},
			metrics,
			peer_store_handle,
		);

		(Self { protocol_name, notification_service, max_known_transactions }, config)
	}

	/// Turns the prototype into the actual handler. Returns a controller that allows controlling
	/// the behaviour of the handler while it's running.
	///
	/// Important: the transactions handler is initially disabled and doesn't gossip transactions.
	/// Gossiping is enabled when major syncing is done.
	pub fn build<
		B: BlockT + 'static,
		H: ExHashT,
		N: NetworkPeers + NetworkEventStream,
		S: SyncEventStream + sp_consensus::SyncOracle,
	>(
		self,
		network: N,
		sync: S,
		transaction_pool: Arc<dyn TransactionPool<H, B>>,
		metrics_registry: Option<&Registry>,
	) -> error::Result<(TransactionsHandler<B, H, N, S>, TransactionsHandlerController<H>)> {
		let sync_event_stream = sync.event_stream("transactions-handler-sync");
		let (to_handler, from_controller) = tracing_unbounded("mpsc_transactions_handler", 100_000);

		let handler = TransactionsHandler {
			protocol_name: self.protocol_name,
			notification_service: self.notification_service,
			propagate_timeout: (Box::pin(interval(PROPAGATE_TIMEOUT))
				as Pin<Box<dyn Stream<Item = ()> + Send>>)
				.fuse(),
			pending_transactions: FuturesUnordered::new(),
			pending_transactions_peers: HashMap::new(),
			network,
			sync,
			sync_event_stream: sync_event_stream.fuse(),
			peers: HashMap::new(),
			transaction_pool,
			from_controller,
			max_known_transactions: self.max_known_transactions,
			metrics: if let Some(r) = metrics_registry {
				Some(Metrics::register(r)?)
			} else {
				None
			},
		};

		let controller = TransactionsHandlerController { to_handler };

		Ok((handler, controller))
	}
}

/// Controls the behaviour of a [`TransactionsHandler`] it is connected to.
pub struct TransactionsHandlerController<H: ExHashT> {
	to_handler: TracingUnboundedSender<ToHandler<H>>,
}

impl<H: ExHashT> TransactionsHandlerController<H> {
	/// You may call this when new transactions are imported by the transaction pool.
	///
	/// All transactions will be fetched from the `TransactionPool` that was passed at
	/// initialization as part of the configuration and propagated to peers.
	pub fn propagate_transactions(&self) {
		let _ = self.to_handler.unbounded_send(ToHandler::PropagateTransactions);
	}

	/// You must call when new a transaction is imported by the transaction pool.
	///
	/// This transaction will be fetched from the `TransactionPool` that was passed at
	/// initialization as part of the configuration and propagated to peers.
	pub fn propagate_transaction(&self, hash: H) {
		let _ = self.to_handler.unbounded_send(ToHandler::PropagateTransaction(hash));
	}
}

enum ToHandler<H: ExHashT> {
	PropagateTransactions,
	PropagateTransaction(H),
}

/// Handler for transactions. Call [`TransactionsHandler::run`] to start the processing.
pub struct TransactionsHandler<
	B: BlockT + 'static,
	H: ExHashT,
	N: NetworkPeers + NetworkEventStream,
	S: SyncEventStream + sp_consensus::SyncOracle,
> {
	protocol_name: ProtocolName,
	/// Interval at which we call `propagate_transactions`.
	propagate_timeout: stream::Fuse<Pin<Box<dyn Stream<Item = ()> + Send>>>,
	/// Pending transactions verification tasks.
	pending_transactions: FuturesUnordered<PendingTransaction<H>>,
	/// As multiple peers can send us the same transaction, we group
	/// these peers using the transaction hash while the transaction is
	/// imported. This prevents that we import the same transaction
	/// multiple times concurrently.
	pending_transactions_peers: HashMap<H, Vec<PeerId>>,
	/// Network service to use to send messages and manage peers.
	network: N,
	/// Syncing service.
	sync: S,
	/// Receiver for syncing-related events.
	sync_event_stream: stream::Fuse<Pin<Box<dyn Stream<Item = SyncEvent> + Send>>>,
	// All connected peers
	peers: HashMap<PeerId, Peer<H>>,
	transaction_pool: Arc<dyn TransactionPool<H, B>>,
	from_controller: TracingUnboundedReceiver<ToHandler<H>>,
	/// Prometheus metrics.
	metrics: Option<Metrics>,
	/// Handle that is used to communicate with `sc_network::Notifications`.
	notification_service: Box<dyn NotificationService>,
	/// Per-peer known-transaction cache capacity.
	max_known_transactions: NonZeroUsize,
}

/// Peer information
#[derive(Debug)]
struct Peer<H: ExHashT> {
	/// Holds a set of transactions known to this peer.
	known_transactions: LruHashSet<H>,
	role: ObservedRole,
}

impl<B, H, N, S> TransactionsHandler<B, H, N, S>
where
	B: BlockT + 'static,
	H: ExHashT,
	N: NetworkPeers + NetworkEventStream,
	S: SyncEventStream + sp_consensus::SyncOracle,
{
	/// Turns the [`TransactionsHandler`] into a future that should run forever and not be
	/// interrupted.
	pub async fn run(mut self) {
		loop {
			futures::select! {
				_ = self.propagate_timeout.next() => {
					self.propagate_transactions();
				},
				(tx_hash, result) = self.pending_transactions.select_next_some() => {
					if let Some(peers) = self.pending_transactions_peers.remove(&tx_hash) {
						peers.into_iter().for_each(|p| self.on_handle_transaction_import(p, result));
					} else {
						warn!(target: "sub-libp2p", "Inconsistent state, no peers for pending transaction!");
					}
				},
				sync_event = self.sync_event_stream.next() => {
					if let Some(sync_event) = sync_event {
						self.handle_sync_event(sync_event);
					} else {
						// Syncing has seemingly closed. Closing as well.
						return;
					}
				}
				message = self.from_controller.select_next_some() => {
					match message {
						ToHandler::PropagateTransaction(hash) => self.propagate_transaction(&hash),
						ToHandler::PropagateTransactions => self.propagate_transactions(),
					}
				},
				event = self.notification_service.next_event().fuse() => {
					if let Some(event) = event {
						self.handle_notification_event(event)
					} else {
						// `Notifications` has seemingly closed. Closing as well.
						return
					}
				}
			}
		}
	}

	fn handle_notification_event(&mut self, event: NotificationEvent) {
		match event {
			NotificationEvent::ValidateInboundSubstream { peer, handshake, result_tx, .. } => {
				// only accept peers whose role can be determined
				let result = self
					.network
					.peer_role(peer, handshake)
					.map_or(ValidationResult::Reject, |_| ValidationResult::Accept);
				let _ = result_tx.send(result);
			},
			NotificationEvent::NotificationStreamOpened { peer, handshake, .. } => {
				let Some(role) = self.network.peer_role(peer, handshake) else {
					log::debug!(target: "sub-libp2p", "role for {peer} couldn't be determined");
					return
				};

				let _was_in = self.peers.insert(
					peer,
					Peer { known_transactions: LruHashSet::new(self.max_known_transactions), role },
				);
				debug_assert!(_was_in.is_none());
			},
			NotificationEvent::NotificationStreamClosed { peer } => {
				let _peer = self.peers.remove(&peer);
				debug_assert!(_peer.is_some());
			},
			NotificationEvent::NotificationReceived { peer, notification } => {
				if let Ok(m) =
					<Transactions<B::Extrinsic> as Decode>::decode(&mut notification.as_ref())
				{
					self.on_transactions(peer, m);
				} else {
					warn!(target: "sub-libp2p", "Failed to decode transactions list from peer {peer}");
					self.network.report_peer(peer, rep::BAD_TRANSACTION);
				}
			},
		}
	}

	fn handle_sync_event(&mut self, event: SyncEvent) {
		match event {
			SyncEvent::PeerConnected(remote) => {
				let addr = iter::once(multiaddr::Protocol::P2p(remote.into()))
					.collect::<multiaddr::Multiaddr>();
				let result = self.network.add_peers_to_reserved_set(
					self.protocol_name.clone(),
					iter::once(addr).collect(),
				);
				if let Err(err) = result {
					log::error!(target: LOG_TARGET, "Add reserved peer failed: {}", err);
				}
			},
			SyncEvent::PeerDisconnected(remote) => {
				let result = self.network.remove_peers_from_reserved_set(
					self.protocol_name.clone(),
					iter::once(remote).collect(),
				);
				if let Err(err) = result {
					log::error!(target: LOG_TARGET, "Remove reserved peer failed: {}", err);
				}
			},
		}
	}

	/// Called when peer sends us new transactions
	fn on_transactions(&mut self, who: PeerId, transactions: Transactions<B::Extrinsic>) {
		// Accept transactions only when node is not major syncing
		if self.sync.is_major_syncing() {
			trace!(target: LOG_TARGET, "{} Ignoring transactions while major syncing", who);
			return
		}

		trace!(target: LOG_TARGET, "Received {} transactions from {}", transactions.len(), who);
		if let Some(ref mut peer) = self.peers.get_mut(&who) {
			for t in transactions {
				if self.pending_transactions.len() > MAX_PENDING_TRANSACTIONS {
					debug!(
						target: LOG_TARGET,
						"Ignoring any further transactions that exceed `MAX_PENDING_TRANSACTIONS`({}) limit",
						MAX_PENDING_TRANSACTIONS,
					);
					break
				}

				let hash = self.transaction_pool.hash_of(&t);
				peer.known_transactions.insert(hash.clone());

				self.network.report_peer(who, rep::ANY_TRANSACTION);

				match self.pending_transactions_peers.entry(hash.clone()) {
					Entry::Vacant(entry) => {
						self.pending_transactions.push(PendingTransaction {
							validation: self.transaction_pool.import(t),
							tx_hash: hash,
						});
						entry.insert(vec![who]);
					},
					Entry::Occupied(mut entry) => {
						entry.get_mut().push(who);
					},
				}
			}
		}
	}

	fn on_handle_transaction_import(&mut self, who: PeerId, import: TransactionImport) {
		match import {
			TransactionImport::KnownGood =>
				self.network.report_peer(who, rep::ANY_TRANSACTION_REFUND),
			TransactionImport::NewGood => self.network.report_peer(who, rep::GOOD_TRANSACTION),
			TransactionImport::Bad => self.network.report_peer(who, rep::BAD_TRANSACTION),
			TransactionImport::None => {},
		}
	}

	/// Propagate one transaction.
	pub fn propagate_transaction(&mut self, hash: &H) {
		// Accept transactions only when node is not major syncing
		if self.sync.is_major_syncing() {
			return
		}

		debug!(target: LOG_TARGET, "Propagating transaction [{:?}]", hash);
		if let Some(transaction) = self.transaction_pool.transaction(hash) {
			let propagated_to = self.do_propagate_transactions(&[(hash.clone(), transaction)]);
			self.transaction_pool.on_broadcasted(propagated_to);
		} else {
			debug!(target: "sync", "Propagating transaction failure [{:?}]", hash);
		}
	}

	fn do_propagate_transactions(
		&mut self,
		transactions: &[(H, Arc<B::Extrinsic>)],
	) -> HashMap<H, Vec<String>> {
		let mut propagated_to = HashMap::<_, Vec<_>>::new();
		let mut propagated_transactions = 0;

		for (who, peer) in self.peers.iter_mut() {
			// never send transactions to the light node
			if matches!(peer.role, ObservedRole::Light) {
				continue
			}

			// Sending into a full synchronous channel force-closes the connection, so never
			// queue more than the channel can hold. Unsent transactions are not marked known
			// and are retried once the peer drains its channel.
			let budget = self
				.notification_service
				.sync_notification_capacity(who)
				.map_or(MAX_TRANSACTIONS_PER_PROPAGATION, |capacity| {
					capacity.min(MAX_TRANSACTIONS_PER_PROPAGATION)
				});

			let selected =
				take_unknown_transactions(&mut peer.known_transactions, transactions, budget);

			propagated_transactions += selected.len();

			if !selected.is_empty() {
				for (hash, _) in &selected {
					propagated_to.entry(hash.clone()).or_default().push(who.to_base58());
				}
				trace!(target: "sync", "Sending {} transactions to {}", selected.len(), who);
				// Historically, the format of a notification of the transactions protocol
				// consisted in a (SCALE-encoded) `Vec<Transaction>`.
				// After RFC 56, the format was modified in a backwards-compatible way to be
				// a (SCALE-encoded) tuple `(Compact(1), Transaction)`, which is the same encoding
				// as a `Vec` of length one. This is no coincidence, as the change was
				// intentionally done in a backwards-compatible way.
				// In other words, the `Vec` that is sent below **must** always have only a single
				// element in it.
				// See <https://github.com/polkadot-fellows/RFCs/blob/main/text/0056-one-transaction-per-notification.md>
				for (_, to_send) in selected {
					self.notification_service.send_sync_notification(who, vec![to_send].encode());
				}
			}
		}

		if let Some(ref metrics) = self.metrics {
			metrics.propagated_transactions.inc_by(propagated_transactions as _)
		}

		propagated_to
	}

	/// Call when we must propagate ready transactions to peers.
	fn propagate_transactions(&mut self) {
		// Accept transactions only when node is not major syncing
		if self.sync.is_major_syncing() {
			return
		}

		let transactions = self.transaction_pool.transactions();

		if transactions.is_empty() {
			return
		}

		debug!(target: LOG_TARGET, "Propagating transactions");

		let propagated_to = self.do_propagate_transactions(&transactions);
		self.transaction_pool.on_broadcasted(propagated_to);
	}
}

/// Select at most `max_send` transactions that are not yet in `known`.
///
/// Only hashes that will be sent are inserted, so a later pass can continue the remainder
/// without all-missing the already queued set.
fn take_unknown_transactions<H, E>(
	known: &mut LruHashSet<H>,
	transactions: &[(H, E)],
	max_send: usize,
) -> Vec<(H, E)>
where
	H: Clone + Hash + Eq,
	E: Clone,
{
	let mut selected = Vec::new();
	for (hash, tx) in transactions {
		if selected.len() >= max_send {
			break
		}
		if known.insert(hash.clone()) {
			selected.push((hash.clone(), tx.clone()));
		}
	}
	selected
}

#[cfg(test)]
mod tests {
	use super::*;
	use sc_network::{
		config::MultiaddrWithPeerId, service::traits::MessageSink, Event, Multiaddr,
		ReputationChange,
	};
	use sp_runtime::testing::{Block as RawBlock, MockCallU64, TestXt};
	use std::{
		collections::HashSet,
		sync::{Arc, Mutex},
	};

	type Extrinsic = TestXt<MockCallU64, ()>;
	type Block = RawBlock<Extrinsic>;

	/// Mirrors litep2p's `SYNC_CHANNEL_SIZE`. A synchronous send into a full channel makes
	/// litep2p schedule `NotificationCommand::ForceClose`, closing the whole connection.
	const LITEP2P_SYNC_CHANNEL_SIZE: usize = 2048;

	#[derive(Debug, Default)]
	struct SinkState {
		/// Notifications currently sitting in the peer's synchronous channel.
		queued: usize,
		/// All send attempts, including ones made against a full channel.
		total_sent: usize,
		/// Send attempts made while the channel was full. One of these force-closes the peer.
		clogged_sends: usize,
	}

	/// `NotificationService` whose sends behave like litep2p's bounded synchronous channel
	/// towards a single peer. The channel drains only when a test explicitly drains it.
	#[derive(Debug)]
	struct BoundedChannelNotificationService {
		protocol_name: ProtocolName,
		state: Arc<Mutex<SinkState>>,
	}

	#[async_trait::async_trait]
	impl NotificationService for BoundedChannelNotificationService {
		async fn open_substream(&mut self, _peer: PeerId) -> Result<(), ()> {
			unimplemented!();
		}

		async fn close_substream(&mut self, _peer: PeerId) -> Result<(), ()> {
			unimplemented!();
		}

		fn send_sync_notification(&mut self, _peer: &PeerId, _notification: Vec<u8>) {
			let mut state = self.state.lock().unwrap();
			state.total_sent += 1;
			if state.queued >= LITEP2P_SYNC_CHANNEL_SIZE {
				state.clogged_sends += 1;
			} else {
				state.queued += 1;
			}
		}

		fn sync_notification_capacity(&self, _peer: &PeerId) -> Option<usize> {
			Some(LITEP2P_SYNC_CHANNEL_SIZE - self.state.lock().unwrap().queued)
		}

		async fn send_async_notification(
			&mut self,
			_peer: &PeerId,
			_notification: Vec<u8>,
		) -> Result<(), error::Error> {
			unimplemented!();
		}

		async fn set_handshake(&mut self, _handshake: Vec<u8>) -> Result<(), ()> {
			unimplemented!();
		}

		fn try_set_handshake(&mut self, _handshake: Vec<u8>) -> Result<(), ()> {
			unimplemented!();
		}

		async fn next_event(&mut self) -> Option<NotificationEvent> {
			unimplemented!();
		}

		fn clone(&mut self) -> Result<Box<dyn NotificationService>, ()> {
			unimplemented!();
		}

		fn protocol(&self) -> &ProtocolName {
			&self.protocol_name
		}

		fn message_sink(&self, _peer: &PeerId) -> Option<Box<dyn MessageSink>> {
			unimplemented!();
		}
	}

	#[derive(Debug)]
	struct TestNetwork;

	#[async_trait::async_trait]
	impl NetworkPeers for TestNetwork {
		fn set_authorized_peers(&self, _peers: HashSet<PeerId>) {
			unimplemented!();
		}

		fn set_authorized_only(&self, _reserved_only: bool) {
			unimplemented!();
		}

		fn add_known_address(&self, _peer_id: PeerId, _addr: Multiaddr) {
			unimplemented!();
		}

		fn report_peer(&self, _peer_id: PeerId, _cost_benefit: ReputationChange) {}

		fn peer_reputation(&self, _peer_id: &PeerId) -> i32 {
			unimplemented!();
		}

		fn disconnect_peer(&self, _peer_id: PeerId, _protocol: ProtocolName) {
			unimplemented!();
		}

		fn accept_unreserved_peers(&self) {
			unimplemented!();
		}

		fn deny_unreserved_peers(&self) {
			unimplemented!();
		}

		fn add_reserved_peer(&self, _peer: MultiaddrWithPeerId) -> Result<(), String> {
			unimplemented!();
		}

		fn remove_reserved_peer(&self, _peer_id: PeerId) {
			unimplemented!();
		}

		fn set_reserved_peers(
			&self,
			_protocol: ProtocolName,
			_peers: HashSet<Multiaddr>,
		) -> Result<(), String> {
			unimplemented!();
		}

		fn add_peers_to_reserved_set(
			&self,
			_protocol: ProtocolName,
			_peers: HashSet<Multiaddr>,
		) -> Result<(), String> {
			unimplemented!();
		}

		fn remove_peers_from_reserved_set(
			&self,
			_protocol: ProtocolName,
			_peers: Vec<PeerId>,
		) -> Result<(), String> {
			unimplemented!();
		}

		fn sync_num_connected(&self) -> usize {
			unimplemented!();
		}

		fn peer_role(&self, _peer_id: PeerId, _handshake: Vec<u8>) -> Option<ObservedRole> {
			unimplemented!();
		}

		async fn reserved_peers(&self) -> Result<Vec<PeerId>, ()> {
			unimplemented!();
		}
	}

	impl NetworkEventStream for TestNetwork {
		fn event_stream(&self, _name: &'static str) -> Pin<Box<dyn Stream<Item = Event> + Send>> {
			Box::pin(stream::pending())
		}
	}

	#[derive(Debug)]
	struct TestSync;

	impl SyncEventStream for TestSync {
		fn event_stream(
			&self,
			_name: &'static str,
		) -> Pin<Box<dyn Stream<Item = SyncEvent> + Send>> {
			Box::pin(stream::pending())
		}
	}

	impl sp_consensus::SyncOracle for TestSync {
		fn is_major_syncing(&self) -> bool {
			false
		}

		fn is_offline(&self) -> bool {
			false
		}
	}

	struct TestPool(Vec<(u64, Arc<Extrinsic>)>);

	impl TestPool {
		fn new(transaction_count: u64) -> Self {
			Self(
				(0..transaction_count)
					.map(|nonce| (nonce, Arc::new(Extrinsic::new_bare(MockCallU64(nonce)))))
					.collect(),
			)
		}
	}

	impl TransactionPool<u64, Block> for TestPool {
		fn transactions(&self) -> Vec<(u64, Arc<Extrinsic>)> {
			self.0.clone()
		}

		fn hash_of(&self, _transaction: &Extrinsic) -> u64 {
			unimplemented!();
		}

		fn import(&self, _transaction: Extrinsic) -> TransactionImportFuture {
			unimplemented!();
		}

		fn on_broadcasted(&self, _propagations: HashMap<u64, Vec<String>>) {}

		fn transaction(&self, hash: &u64) -> Option<Arc<Extrinsic>> {
			self.0.iter().find(|(candidate, _)| candidate == hash).map(|(_, tx)| tx.clone())
		}
	}

	struct TestHarness {
		handler: TransactionsHandler<Block, u64, TestNetwork, TestSync>,
		sink_state: Arc<Mutex<SinkState>>,
		/// Keeps `from_controller` open.
		_controller: TransactionsHandlerController<u64>,
	}

	/// Build a handler with one full peer whose synchronous channel is never drained unless the
	/// test drains it, backed by a pool of `pool_size` ready transactions.
	fn test_harness(pool_size: u64) -> TestHarness {
		let sink_state = Arc::new(Mutex::new(SinkState::default()));
		let protocol_name: ProtocolName = "/test/transactions/1".into();
		let (to_handler, from_controller) =
			tracing_unbounded("mpsc_test_transactions_handler", 100);
		let max_known_transactions = NonZeroUsize::new(pool_size as usize).unwrap();

		let mut handler = TransactionsHandler {
			protocol_name: protocol_name.clone(),
			notification_service: Box::new(BoundedChannelNotificationService {
				protocol_name,
				state: sink_state.clone(),
			}),
			propagate_timeout: (Box::pin(stream::pending::<()>())
				as Pin<Box<dyn Stream<Item = ()> + Send>>)
				.fuse(),
			pending_transactions: FuturesUnordered::new(),
			pending_transactions_peers: HashMap::new(),
			network: TestNetwork,
			sync: TestSync,
			sync_event_stream: (Box::pin(stream::pending::<SyncEvent>())
				as Pin<Box<dyn Stream<Item = SyncEvent> + Send>>)
				.fuse(),
			peers: HashMap::new(),
			transaction_pool: Arc::new(TestPool::new(pool_size)),
			from_controller,
			max_known_transactions,
			metrics: None,
		};
		handler.peers.insert(
			PeerId::random(),
			Peer {
				known_transactions: LruHashSet::new(max_known_transactions),
				role: ObservedRole::Full,
			},
		);

		TestHarness {
			handler,
			sink_state,
			_controller: TransactionsHandlerController { to_handler },
		}
	}

	/// A peer that stops draining its synchronous channel must not have more notifications
	/// pushed at it than the channel holds: the first send into the full channel makes litep2p
	/// force-close the whole connection. Repeated propagate passes over a large pool must
	/// therefore stop sending once the channel is full.
	#[test]
	fn undrained_peer_is_not_sent_more_than_the_sync_channel_holds() {
		let mut harness = test_harness(8 * LITEP2P_SYNC_CHANNEL_SIZE as u64);

		for _ in 0..16 {
			harness.handler.propagate_transactions();
		}

		let state = harness.sink_state.lock().unwrap();
		assert_eq!(
			state.clogged_sends, 0,
			"sent {} notifications into a full 2048-slot channel; litep2p would force-close \
			 the peer connection",
			state.clogged_sends,
		);
	}

	/// Same as above but through the immediate per-import path: a burst of imports, each
	/// propagated individually, must not overflow an undrained channel either.
	#[test]
	fn import_burst_is_not_sent_beyond_the_sync_channel_capacity() {
		let pool_size = 2 * LITEP2P_SYNC_CHANNEL_SIZE as u64;
		let mut harness = test_harness(pool_size);

		for hash in 0..pool_size {
			harness.handler.propagate_transaction(&hash);
		}

		let state = harness.sink_state.lock().unwrap();
		assert_eq!(
			state.clogged_sends, 0,
			"import burst pushed {} notifications into a full 2048-slot channel; litep2p \
			 would force-close the peer connection",
			state.clogged_sends,
		);
	}

	/// Throttling must not stall a healthy peer: when the channel drains between passes, every
	/// ready transaction is eventually delivered exactly once.
	#[test]
	fn draining_peer_eventually_receives_every_transaction() {
		let pool_size = 8 * LITEP2P_SYNC_CHANNEL_SIZE as u64;
		let mut harness = test_harness(pool_size);

		for _ in 0..64 {
			harness.handler.propagate_transactions();
			harness.sink_state.lock().unwrap().queued = 0;
		}

		let state = harness.sink_state.lock().unwrap();
		assert_eq!(state.clogged_sends, 0);
		assert_eq!(state.total_sent as u64, pool_size);
	}

	fn scan(cache: &mut LruHashSet<u32>, transactions: &[(u32, u32)], max_send: usize) -> Vec<u32> {
		take_unknown_transactions(cache, transactions, max_send)
			.into_iter()
			.map(|(hash, _)| hash)
			.collect()
	}

	#[test]
	fn bounded_pass_leaves_remainder_for_next_scan() {
		let mut cache = LruHashSet::new(NonZeroUsize::new(8).unwrap());
		let transactions: Vec<(u32, u32)> = (0..5).map(|h| (h, h)).collect();

		assert_eq!(scan(&mut cache, &transactions, 3), vec![0, 1, 2]);
		assert_eq!(scan(&mut cache, &transactions, 3), vec![3, 4]);
		assert!(scan(&mut cache, &transactions, 3).is_empty());
	}

	#[test]
	fn full_cache_rescan_sends_nothing() {
		let mut cache = LruHashSet::new(NonZeroUsize::new(5).unwrap());
		let transactions: Vec<(u32, u32)> = (0..5).map(|h| (h, h)).collect();

		assert_eq!(scan(&mut cache, &transactions, 5).len(), 5);
		assert!(scan(&mut cache, &transactions, 5).is_empty());
	}
}
