//! A collection of node-specific RPC methods.
//! Substrate provides the `sc-rpc` crate, which defines the core RPC layer
//! used by Substrate nodes. This file extends those RPC definitions with
//! capabilities that are specific to this project's runtime configuration.

#![warn(missing_docs)]

use jsonrpsee::{core::RpcResult, proc_macros::rpc, RpcModule};
use quantus_runtime::{opaque::Block, AccountId, Balance, Nonce};
use sc_client_api::{AuxStore, Backend, BlockchainEvents, StorageProvider, UsageProvider};
use sc_network::service::traits::NetworkService;
use sc_rpc::SubscriptionTaskExecutor;
use sc_transaction_pool_api::TransactionPool;
use serde::{Deserialize, Serialize};
use sp_api::{CallApiAt, ProvideRuntimeApi};
use sp_block_builder::BlockBuilder;
use sp_blockchain::{Error as BlockChainError, HeaderBackend, HeaderMetadata};
use sp_inherents::CreateInherentDataProviders;
use sp_runtime::traits::Block as BlockT;
use std::sync::Arc;

use crate::eth_rpc::{create_eth, EthDeps};

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

/// Full client dependencies.
pub struct FullDeps<B: BlockT, C, P, CT, CIDP> {
	/// The client instance to use.
	pub client: Arc<C>,
	/// Transaction pool instance.
	pub pool: Arc<P>,
	/// Network service instance (optional, only when peer sharing is enabled).
	pub network: Option<Arc<dyn NetworkService>>,
	/// Ethereum-compatibility specific dependencies.
	pub eth: EthDeps<B, C, P, CT, CIDP>,
}

pub struct DefaultEthConfig<C, BE>(std::marker::PhantomData<(C, BE)>);

impl<B, C, BE> fc_rpc::EthConfig<B, C> for DefaultEthConfig<C, BE>
where
	B: BlockT,
	C: StorageProvider<B, BE> + Sync + Send + 'static,
	BE: Backend<B> + 'static,
{
	type EstimateGasAdapter = ();
	type RuntimeStorageOverride =
		fc_rpc::frontier_backend_client::SystemAccountId20StorageOverride<B, C, BE>;
}

/// Instantiate all full RPC extensions.
pub fn create_full<C, P, BE, CT, CIDP>(
	deps: FullDeps<Block, C, P, CT, CIDP>,
	subscription_task_executor: SubscriptionTaskExecutor,
	pubsub_notification_sinks: Arc<
		fc_mapping_sync::EthereumBlockNotificationSinks<
			fc_mapping_sync::EthereumBlockNotification<Block>,
		>,
	>,
) -> Result<RpcModule<()>, Box<dyn std::error::Error + Send + Sync>>
where
	C: CallApiAt<Block>,
	C: ProvideRuntimeApi<Block>,
	C: HeaderBackend<Block> + HeaderMetadata<Block, Error = BlockChainError> + 'static,
	C: BlockchainEvents<Block> + AuxStore + UsageProvider<Block> + StorageProvider<Block, BE>,
	C: Send + Sync + 'static,
	C::Api: substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Nonce>,
	C::Api: pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<Block, Balance>,
	C::Api: sp_consensus_qpow::QPoWApi<Block>,
	C::Api: fp_rpc::ConvertTransactionRuntimeApi<Block>,
	C::Api: fp_rpc::EthereumRuntimeRPCApi<Block>,
	C::Api: BlockBuilder<Block>,
	BE: Backend<Block> + 'static,
	P: TransactionPool<Block = Block, Hash = <Block as BlockT>::Hash> + 'static,
	CIDP: CreateInherentDataProviders<Block, ()> + Send + 'static,
	CT: fp_rpc::ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
{
	use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApiServer};
	use substrate_frame_rpc_system::{System, SystemApiServer};

	let mut module = RpcModule::new(());
	let FullDeps { client, pool, network, eth } = deps;

	module.merge(System::new(client.clone(), pool.clone()).into_rpc())?;
	module.merge(TransactionPayment::new(client.clone()).into_rpc())?;
	module.merge(Peer::new(network).into_rpc())?;

	// Ethereum compatibility RPCs
	let io = create_eth::<Block, C, _, _, _, _, DefaultEthConfig<C, BE>>(
		module,
		eth,
		subscription_task_executor,
		pubsub_notification_sinks,
	)?;

	Ok(io)
}
