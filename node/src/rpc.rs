//! A collection of node-specific RPC methods.
//! Substrate provides the `sc-rpc` crate, which defines the core RPC layer
//! used by Substrate nodes. This file extends those RPC definitions with
//! capabilities that are specific to this project's runtime configuration.

#![warn(missing_docs)]

use std::sync::Arc;

use jsonrpsee::{
	core::{async_trait, RpcResult},
	proc_macros::rpc,
	RpcModule,
};
use quantus_runtime::{opaque::Block, AccountId, Balance, Nonce};
use sc_network::service::traits::NetworkService;
use sc_transaction_pool_api::TransactionPool;
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder;
use sp_blockchain::{Error as BlockChainError, HeaderBackend, HeaderMetadata};

/// Network identity and topology of this node, as returned by `peer_getNetworkInfo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
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
	/// Get this node's network identity and topology (peer ID, connected peers,
	/// external and listen addresses).
	///
	/// This is an *unsafe* RPC (like upstream `system_peers`/`system_unstable_networkState`):
	/// it discloses the node's network topology, so it is only served to local connections
	/// or when the node runs with `--rpc-methods unsafe`.
	#[method(name = "peer_getNetworkInfo", with_extensions)]
	async fn get_network_info(&self) -> RpcResult<NetworkInfo>;
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

#[async_trait]
impl PeerApiServer for Peer {
	async fn get_network_info(&self, ext: &jsonrpsee::Extensions) -> RpcResult<NetworkInfo> {
		// Peer IDs and external/listen addresses enable network mapping and targeted
		// eclipse/partition attempts; upstream only exposes them via unsafe-gated RPCs.
		sc_rpc_api::check_if_safe(ext)?;

		if let Some(network) = &self.network {
			let network_state = network.network_state().await.map_err(|_| {
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

			Ok(NetworkInfo {
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
pub struct FullDeps<C, P> {
	/// The client instance to use.
	pub client: Arc<C>,
	/// Transaction pool instance.
	pub pool: Arc<P>,
	/// Network service instance (optional, only when peer sharing is enabled).
	pub network: Option<Arc<dyn NetworkService>>,
}

/// Instantiate all full RPC extensions.
///
/// # RPC admission model
///
/// There is deliberately no module-level gate here: like upstream Substrate's
/// node template, admission control is *per method*, enforced against the
/// per-connection [`sc_rpc_api::DenyUnsafe`] policy that the substrate RPC
/// server injects into request extensions (local connections and
/// `--rpc-methods unsafe` get `No`; external connections under the default
/// `auto` get `Yes`). Server-level limits (`--rpc-max-connections`,
/// `--rpc-max-subscriptions-per-connection`, `--rpc-rate-limit`, CORS) apply
/// on top.
///
/// Every method merged below must therefore be explicitly classified:
///
/// - **Unsafe** (discloses topology/identity or does privileged work): must
///   call `sc_rpc_api::check_if_safe(ext)` before doing anything, like
///   [`Peer::get_network_info`] and upstream `system_dryRun`.
/// - **Safe** (public chain/mempool data, bounded per-request work): must have
///   its request-driven resource use bounded, like the `zkTree_getMerkleProof`
///   proof-window guard and `TxWatch`'s fixed-capacity broadcast fanout.
///
/// Do not merge a new module without classifying each of its methods this way.
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
	C::Api: pallet_zk_tree::ZkTreeApi<Block>,
	C::Api: BlockBuilder<Block>,
	P: TransactionPool<Block = Block> + 'static,
{
	use crate::{
		txwatch::{TxWatch, TxWatchApiServer},
		zktree_rpc::{ZkTree, ZkTreeApiServer},
	};
	use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApiServer};
	use substrate_frame_rpc_system::{System, SystemApiServer};

	let mut module = RpcModule::new(());
	let FullDeps { client, pool, network } = deps;

	module.merge(System::new(client.clone(), pool.clone()).into_rpc())?;
	module.merge(TransactionPayment::new(client.clone()).into_rpc())?;
	module.merge(Peer::new(network).into_rpc())?;
	module.merge(TxWatch::new(pool).into_rpc())?;
	module.merge(ZkTree::new(client).into_rpc())?;

	Ok(module)
}

#[cfg(test)]
mod tests {
	use super::*;
	use jsonrpsee::MethodsError;
	use sc_rpc_api::DenyUnsafe;

	/// Call `peer_getNetworkInfo` on a `Peer` module with the given per-connection safety
	/// policy, the way the substrate RPC server injects it for real connections.
	async fn call_get_network_info(deny_unsafe: DenyUnsafe) -> Result<NetworkInfo, MethodsError> {
		let mut module = Peer::new(None).into_rpc();
		module.extensions_mut().insert(deny_unsafe);
		module.call("peer_getNetworkInfo", jsonrpsee::rpc_params![]).await
	}

	/// `peer_getNetworkInfo` discloses the node's peer ID, connected peer IDs and
	/// external/listen addresses -- the same network topology upstream Substrate only
	/// serves through unsafe-gated RPCs (`system_peers`, `system_unstable_networkState`).
	/// Untrusted callers (external connections under the default `--rpc-methods auto`)
	/// must be rejected by the safety gate before the handler does anything.
	#[tokio::test]
	async fn get_network_info_is_denied_for_untrusted_callers() {
		let err = call_get_network_info(DenyUnsafe::Yes).await.unwrap_err();
		match err {
			MethodsError::JsonRpc(err) => assert!(
				err.message().contains("unsafe"),
				"expected the unsafe-RPC denial, got: {err:?}"
			),
			other => panic!("expected a JSON-RPC error object, got: {other:?}"),
		}
	}

	/// Trusted callers (local connections, or `--rpc-methods unsafe`) pass the gate and
	/// reach the handler proper: without a network handle it reports that peer sharing
	/// is disabled (code 5000) rather than an authorization error.
	#[tokio::test]
	async fn get_network_info_reaches_the_handler_for_trusted_callers() {
		let err = call_get_network_info(DenyUnsafe::No).await.unwrap_err();
		match err {
			MethodsError::JsonRpc(err) => assert_eq!(err.code(), 5000),
			other => panic!("expected a JSON-RPC error object, got: {other:?}"),
		}
	}
}
