//! ZK Trie RPC API implementation.
//!
//! Provides RPC methods for querying the ZK Merkle tree state and generating proofs.

use std::sync::Arc;

use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use pallet_zk_trie::{Hash256, ZkMerkleProofRpc, ZkTrieApi as ZkTrieRuntimeApi};
use quantus_runtime::opaque::Block;
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;

/// ZK Trie state information returned by the RPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkTrieState {
	/// Current root hash of the ZK tree.
	pub root: Hash256,
	/// Number of leaves in the tree.
	pub leaf_count: u64,
	/// Current depth of the tree.
	pub depth: u8,
}

/// ZK Trie RPC API trait.
#[rpc(client, server)]
pub trait ZkTrieApi {
	/// Get the current state of the ZK trie.
	///
	/// Returns the current root hash, leaf count, and tree depth.
	#[method(name = "zkTrie_getState")]
	fn get_state(&self) -> RpcResult<ZkTrieState>;

	/// Get a Merkle proof for a leaf at the given index.
	///
	/// Returns `null` if the leaf index is out of bounds.
	#[method(name = "zkTrie_getMerkleProof")]
	fn get_merkle_proof(&self, leaf_index: u64) -> RpcResult<Option<ZkMerkleProofRpc>>;
}

/// ZK Trie RPC handler.
pub struct ZkTrie<C> {
	client: Arc<C>,
}

impl<C> ZkTrie<C> {
	/// Create a new ZkTrie RPC handler.
	pub fn new(client: Arc<C>) -> Self {
		Self { client }
	}
}

impl<C> ZkTrieApiServer for ZkTrie<C>
where
	C: ProvideRuntimeApi<Block>,
	C: HeaderBackend<Block> + Send + Sync + 'static,
	C::Api: ZkTrieRuntimeApi<Block>,
{
	fn get_state(&self) -> RpcResult<ZkTrieState> {
		let best_hash = self.client.info().best_hash;

		let root = self.client.runtime_api().get_root(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9000,
				format!("Failed to get ZK trie root: {:?}", e),
				None::<()>,
			)
		})?;

		let leaf_count = self.client.runtime_api().get_leaf_count(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9001,
				format!("Failed to get ZK trie leaf count: {:?}", e),
				None::<()>,
			)
		})?;

		let depth = self.client.runtime_api().get_depth(best_hash).map_err(|e| {
			jsonrpsee::types::error::ErrorObject::owned(
				9002,
				format!("Failed to get ZK trie depth: {:?}", e),
				None::<()>,
			)
		})?;

		Ok(ZkTrieState { root, leaf_count, depth })
	}

	fn get_merkle_proof(&self, leaf_index: u64) -> RpcResult<Option<ZkMerkleProofRpc>> {
		let best_hash = self.client.info().best_hash;

		let proof =
			self.client.runtime_api().get_merkle_proof(best_hash, leaf_index).map_err(|e| {
				jsonrpsee::types::error::ErrorObject::owned(
					9003,
					format!("Failed to get ZK merkle proof: {:?}", e),
					None::<()>,
				)
			})?;

		Ok(proof)
	}
}
