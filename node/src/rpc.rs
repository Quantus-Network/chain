//! A collection of node-specific RPC methods.
//! Substrate provides the `sc-rpc` crate, which defines the core RPC layer
//! used by Substrate nodes. This file extends those RPC definitions with
//! capabilities that are specific to this project's runtime configuration.

#![warn(missing_docs)]

use std::sync::Arc;

use jsonrpsee::{core::RpcResult, proc_macros::rpc, RpcModule};
use quantus_runtime::{opaque::Block, AccountId, Balance, Nonce};
use sc_network::service::traits::NetworkService;
use sc_transaction_pool_api::TransactionPool;
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder;
use sp_blockchain::{Error as BlockChainError, HeaderBackend, HeaderMetadata};
use sp_consensus_qpow::QPoWApi;

/// Peer information for RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
	/// Peer ID
	pub peer_id: String,
	/// Number of connected peers
	pub peer_count: usize,
	/// List of connected peer IDs
	pub connected_peers: Vec<String>,
	/// External addresses of this node
	pub external_addresses: Vec<String>,
	/// Listen addresses of this node
	pub listen_addresses: Vec<String>,
}

/// Peer RPC API
#[rpc(client, server)]
pub trait PeerApi {
	/// Get basic peer information
	#[method(name = "peer_getBasicInfo")]
	fn get_basic_info(&self) -> RpcResult<PeerInfo>;
}

/// QPoW RPC API
#[rpc(client, server)]
pub trait QPoWApi {
	/// Get difficulty for a specific block hash
	#[method(name = "qpow_getBlockDifficulty")]
	fn get_block_difficulty(&self, block_hash: String) -> RpcResult<Option<String>>;

	/// Get distance achieved for a specific block hash
	#[method(name = "qpow_getBlockDistanceAchieved")]
	fn get_block_distance_achieved(&self, block_hash: String) -> RpcResult<Option<String>>;
}

/// Peer RPC implementation
pub struct Peer {
	/// Network service instance
	network: Option<Arc<dyn NetworkService>>,
}

impl Peer {
	/// Create new Peer RPC handler
	pub fn new(network: Option<Arc<dyn NetworkService>>) -> Self {
		Self { network }
	}
}

impl PeerApiServer for Peer {
	fn get_basic_info(&self) -> RpcResult<PeerInfo> {
		if let Some(network) = &self.network {
			// Get network state
			let network_state =
				futures::executor::block_on(network.network_state()).map_err(|_| {
					jsonrpsee::types::error::ErrorObject::owned(
						5001,
						"Failed to get network state",
						None::<()>,
					)
				})?;

			let connected_peers: Vec<String> =
				network_state.connected_peers.keys().cloned().collect();

			let external_addresses: Vec<String> =
				network_state.external_addresses.iter().map(|addr| addr.to_string()).collect();

			let listen_addresses: Vec<String> =
				network_state.listened_addresses.iter().map(|addr| addr.to_string()).collect();

			Ok(PeerInfo {
				peer_id: network_state.peer_id,
				peer_count: connected_peers.len(),
				connected_peers,
				external_addresses,
				listen_addresses,
			})
		} else {
			Err(jsonrpsee::types::error::ErrorObject::owned(
				5000,
				"Peer sharing is not enabled",
				None::<()>,
			))
		}
	}
}

/// QPoW RPC implementation
pub struct QPoW<C> {
	/// Client instance
	client: Arc<C>,
}

impl<C> QPoW<C> {
	/// Create new QPoW RPC handler
	pub fn new(client: Arc<C>) -> Self {
		Self { client }
	}
}

impl<C> QPoWApiServer for QPoW<C>
where
	C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
	C::Api: sp_consensus_qpow::QPoWApi<Block>,
{
	fn get_block_difficulty(&self, block_hash: String) -> RpcResult<Option<String>> {
		// Parse hex string to [u8; 32]
		let hash_bytes = hex::decode(block_hash.trim_start_matches("0x")).map_err(|_| {
			jsonrpsee::types::error::ErrorObject::owned(
				6001,
				"Invalid block hash format",
				None::<()>,
			)
		})?;

		if hash_bytes.len() != 32 {
			return Err(jsonrpsee::types::error::ErrorObject::owned(
				6002,
				"Block hash must be 32 bytes",
				None::<()>,
			));
		}

		let mut hash_array = [0u8; 32];
		hash_array.copy_from_slice(&hash_bytes);

		let best_hash = self.client.info().best_hash;
		let difficulty = self
			.client
			.runtime_api()
			.get_block_difficulty(best_hash, hash_array)
			.map_err(|e| {
				jsonrpsee::types::error::ErrorObject::owned(
					6003,
					format!("Runtime API call failed: {}", e),
					None::<()>,
				)
			})?;

		Ok(difficulty.map(|d| format!("0x{:x}", d)))
	}

	fn get_block_distance_achieved(&self, block_hash: String) -> RpcResult<Option<String>> {
		// Parse hex string to [u8; 32]
		let hash_bytes = hex::decode(block_hash.trim_start_matches("0x")).map_err(|_| {
			jsonrpsee::types::error::ErrorObject::owned(
				6001,
				"Invalid block hash format",
				None::<()>,
			)
		})?;

		if hash_bytes.len() != 32 {
			return Err(jsonrpsee::types::error::ErrorObject::owned(
				6002,
				"Block hash must be 32 bytes",
				None::<()>,
			));
		}

		let mut hash_array = [0u8; 32];
		hash_array.copy_from_slice(&hash_bytes);

		let best_hash = self.client.info().best_hash;
		let distance = self
			.client
			.runtime_api()
			.get_block_distance_achieved(best_hash, hash_array)
			.map_err(|e| {
				jsonrpsee::types::error::ErrorObject::owned(
					6003,
					format!("Runtime API call failed: {}", e),
					None::<()>,
				)
			})?;

		Ok(distance.map(|d| format!("0x{:x}", d)))
	}
}

/// Full client dependencies.
pub struct FullDeps<C, P> {
	/// The client instance to use.
	pub client: Arc<C>,
	/// Transaction pool instance.
	pub pool: Arc<P>,
	/// Network service instance (optional, only when peer sharing is enabled).
	pub network: Option<Arc<dyn NetworkService>>,
}

/// Instantiate all full RPC extensions.
pub fn create_full<C, P>(
	deps: FullDeps<C, P>,
) -> Result<RpcModule<()>, Box<dyn std::error::Error + Send + Sync>>
where
	C: ProvideRuntimeApi<Block>,
	C: HeaderBackend<Block> + HeaderMetadata<Block, Error = BlockChainError> + 'static,
	C: Send + Sync + 'static,
	C::Api: substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Nonce>,
	C::Api: pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<Block, Balance>,
	C::Api: sp_consensus_qpow::QPoWApi<Block>,
	C::Api: BlockBuilder<Block>,
	P: TransactionPool<Block = Block> + 'static,
{
	use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApiServer};
	use substrate_frame_rpc_system::{System, SystemApiServer};

	let mut module = RpcModule::new(());
	let FullDeps { client, pool, network } = deps;

	module.merge(System::new(client.clone(), pool.clone()).into_rpc())?;
	module.merge(TransactionPayment::new(client.clone()).into_rpc())?;
	module.merge(QPoW::new(client.clone()).into_rpc())?;
	module.merge(Peer::new(network).into_rpc())?;

	Ok(module)
}
