//! ZK Tree RPC API implementation.
//!
//! Provides RPC methods for querying the ZK Merkle tree state and generating proofs.

use std::sync::Arc;

use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use pallet_zk_tree::{Hash256, ZkMerkleProofRpc, ZkTreeApi as ZkTreeRuntimeApi};
use quantus_runtime::opaque::Block;
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::H256;

/// ZK Tree state information returned by the RPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkTreeState {
	/// Current root hash of the ZK tree.
	pub root: Hash256,
	/// Number of leaves in the tree.
	pub leaf_count: u64,
	/// Current depth of the tree.
	pub depth: u8,
}

/// ZK Tree RPC API trait.
#[rpc(client, server)]
pub trait ZkTreeApi {
	/// Get the current state of the ZK tree.
	///
	/// Returns the current root hash, leaf count, and tree depth.
	#[method(name = "zkTree_getState")]
	fn get_state(&self) -> RpcResult<ZkTreeState>;

	/// Get a Merkle proof for a leaf at the given index.
	///
	/// If `at_block` is provided, fetch the proof at that specific block hash.
	/// Otherwise, use the best (latest) block.
	///
	/// **IMPORTANT**: For ZK proof generation, you MUST pass the block hash
	/// that you're proving against. The tree root changes with each block,
	/// so the Merkle proof must be from the same block as the header.
	///
	/// Returns `null` if the leaf index is out of bounds.
	#[method(name = "zkTree_getMerkleProof")]
	fn get_merkle_proof(
		&self,
		leaf_index: u64,
		at_block: Option<H256>,
	) -> RpcResult<Option<ZkMerkleProofRpc>>;
}

/// ZK Tree RPC handler.
pub struct ZkTree<C> {
	client: Arc<C>,
}

impl<C> ZkTree<C> {
	/// Create a new ZkTree RPC handler.
	pub fn new(client: Arc<C>) -> Self {
		Self { client }
	}
}

impl<C> ZkTreeApiServer for ZkTree<C>
where
	C: ProvideRuntimeApi<Block>,
	C: HeaderBackend<Block> + Send + Sync + 'static,
	C::Api: ZkTreeRuntimeApi<Block>,
{
	fn get_state(&self) -> RpcResult<ZkTreeState> {
		let best_hash = self.client.info().best_hash;

		let root = self.client.runtime_api().get_root(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9000,
				format!("Failed to get ZK tree root: {:?}", e),
				None::<()>,
			)
		})?;

		let leaf_count = self.client.runtime_api().get_leaf_count(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9001,
				format!("Failed to get ZK tree leaf count: {:?}", e),
				None::<()>,
			)
		})?;

		let depth = self.client.runtime_api().get_depth(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9002,
				format!("Failed to get ZK tree depth: {:?}", e),
				None::<()>,
			)
		})?;

		Ok(ZkTreeState { root, leaf_count, depth })
	}

	fn get_merkle_proof(
		&self,
		leaf_index: u64,
		at_block: Option<H256>,
	) -> RpcResult<Option<ZkMerkleProofRpc>> {
		let block_hash = at_block.unwrap_or_else(|| self.client.info().best_hash);

		let proof =
			self.client
				.runtime_api()
				.get_merkle_proof(block_hash, leaf_index)
				.map_err(|e| {
					jsonrpsee::types::error::ErrorObject::owned(
						9003,
						format!("Failed to get ZK merkle proof at {:?}: {:?}", block_hash, e),
						None::<()>,
					)
				})?;

		Ok(proof)
	}
}
