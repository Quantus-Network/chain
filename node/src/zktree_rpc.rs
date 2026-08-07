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
	#[method(name = "zkTree_getState", blocking)]
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
	/// `at_block` must be within the last `BlockHashCount` blocks: older
	/// proofs are rejected on-chain anyway (the block hash is no longer in
	/// `frame_system::BlockHash`), so querying deeper history is refused
	/// (error 9005; unknown hashes: error 9004; backend lookup failures:
	/// error 9006).
	///
	/// The window size is the `BlockHashCount` compiled into this node binary
	/// (currently 4096), not a value read from the live chain. A forkless
	/// runtime upgrade that changes `BlockHashCount` will leave already-running
	/// nodes using the old window until they are rebuilt.
	///
	/// Returns `null` if the leaf index is out of bounds.
	#[method(name = "zkTree_getMerkleProof", blocking)]
	fn get_merkle_proof(
		&self,
		leaf_index: u64,
		at_block: Option<H256>,
	) -> RpcResult<Option<ZkMerkleProofRpc>>;
}

/// Resolve and validate the block a Merkle proof is generated at.
///
/// `at_block` is caller-controlled and this node runs with full canonical state
/// retention, so an unbounded `at_block` would let an unauthenticated caller force
/// cold trie reads across the entire chain history. There is no legitimate reason
/// to query that far back: a wormhole spend proof is only accepted on-chain while
/// the proving block's hash is still in `frame_system::BlockHash`, a sliding
/// window of `BlockHashCount` blocks. Blocks outside that window are rejected.
///
/// The requested hash must also be the *canonical* hash at its height. The backend
/// resolves numbers for any imported block (side forks included), but settlement
/// verifies the claimed hash against `frame_system::BlockHash`, so a proof built on
/// fork state is unusable by construction — reject it here instead of spending
/// state-execution resources producing it.
///
/// The window is `quantus_runtime::configs::BlockHashCount` from the runtime
/// crate linked into this node binary — a compile-time constant, not a live
/// chain/metadata lookup. After a forkless upgrade that changes
/// `BlockHashCount`, already-running nodes keep the old value until rebuilt.
/// That is acceptable for this DoS guard: a slightly stale window is still
/// bounded, and a smaller window is the safer direction.
fn resolve_proof_block<C>(
	client: &C,
	at_block: Option<H256>,
) -> Result<H256, jsonrpsee::types::error::ErrorObject<'static>>
where
	C: HeaderBackend<Block>,
{
	let info = client.info();
	let Some(hash) = at_block else {
		return Ok(info.best_hash);
	};

	let number = match client.number(hash) {
		Ok(Some(n)) => n,
		Ok(None) =>
			return Err(jsonrpsee::types::error::ErrorObject::owned(
				9004,
				format!("Unknown block hash {hash:?}"),
				None::<()>,
			)),
		Err(e) =>
			return Err(jsonrpsee::types::error::ErrorObject::owned(
				9006,
				format!("Failed to resolve block number for {hash:?}: {e}"),
				None::<()>,
			)),
	};

	// The backend resolves a number for ANY block it has imported, including
	// side-fork blocks — resolvability is not canonicality. On-chain settlement
	// compares the proof's claimed hash against `frame_system::BlockHash` (the
	// canonical chain), so proof material derived from fork state can never
	// settle. Reject anything that is not the canonical hash at its height;
	// heights above best (where `best - number` saturates to 0) are rejected
	// first for a precise error.
	if number > info.best_number {
		return Err(jsonrpsee::types::error::ErrorObject::owned(
			9007,
			format!(
				"Block {hash:?} (#{number}) is above the current best block \
				 (#{best}); it is not on the canonical chain",
				best = info.best_number,
			),
			None::<()>,
		));
	}

	let canonical = client.hash(number).map_err(|e| {
		jsonrpsee::types::error::ErrorObject::owned(
			9006,
			format!("Failed to resolve canonical hash at #{number}: {e}"),
			None::<()>,
		)
	})?;
	if canonical != Some(hash) {
		return Err(jsonrpsee::types::error::ErrorObject::owned(
			9008,
			format!(
				"Block {hash:?} (#{number}) is not on the canonical chain; proofs \
				 against fork state cannot be verified on-chain"
			),
			None::<()>,
		));
	}

	// Compile-time constant from the linked runtime crate — see fn docs.
	let window = <quantus_runtime::configs::BlockHashCount as sp_core::Get<u32>>::get();
	if info.best_number.saturating_sub(number) > window {
		return Err(jsonrpsee::types::error::ErrorObject::owned(
			9005,
			format!(
				"Block {hash:?} (#{number}) is older than the {window}-block proof window; \
				 proofs against it can no longer be verified on-chain"
			),
			None::<()>,
		));
	}

	Ok(hash)
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
		let block_hash = resolve_proof_block(&*self.client, at_block)?;

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

#[cfg(test)]
mod tests {
	use super::*;
	use sp_blockchain::{BlockStatus, Info, Result as BlockchainResult};
	use sp_core::Get;
	use sp_runtime::traits::{Block as BlockT, NumberFor};
	use std::collections::{HashMap, HashSet};

	fn window() -> u32 {
		<quantus_runtime::configs::BlockHashCount as Get<u32>>::get()
	}

	fn hash_for(number: u32) -> H256 {
		H256::from_low_u64_be(u64::from(number) + 1)
	}

	/// Minimal chain view: a best block, the known (hash -> number) blocks the
	/// backend has imported (canonical *and* side-fork), and the canonical
	/// (number -> hash) index.
	struct MockChain {
		best_number: u32,
		/// Every imported block, like the backend's hash->number index. Includes
		/// side-fork blocks, which is exactly why resolvability != canonicality.
		known: HashMap<H256, u32>,
		/// The canonical chain's number->hash index.
		canonical: HashMap<u32, H256>,
		/// Hashes for which `number()` simulates a backend/DB failure.
		failing: HashSet<H256>,
	}

	impl MockChain {
		fn with_blocks(best_number: u32, numbers: &[u32]) -> Self {
			let known = numbers.iter().map(|n| (hash_for(*n), *n)).collect();
			let canonical = numbers.iter().map(|n| (*n, hash_for(*n))).collect();
			Self { best_number, known, canonical, failing: HashSet::new() }
		}

		fn with_number_failure(best_number: u32, failing_hash: H256) -> Self {
			let mut chain = Self::with_blocks(best_number, &[best_number]);
			chain.failing.insert(failing_hash);
			chain
		}

		/// Add a block the backend knows about (imported) that is NOT on the
		/// canonical chain, at the given height. Returns its hash.
		fn add_fork_block(&mut self, number: u32) -> H256 {
			let fork_hash = H256::from_low_u64_be(0xF0_0000 + u64::from(number));
			self.known.insert(fork_hash, number);
			fork_hash
		}
	}

	impl HeaderBackend<Block> for MockChain {
		fn header(&self, _hash: H256) -> BlockchainResult<Option<<Block as BlockT>::Header>> {
			Ok(None)
		}

		fn info(&self) -> Info<Block> {
			Info {
				best_hash: hash_for(self.best_number),
				best_number: self.best_number,
				genesis_hash: hash_for(0),
				finalized_hash: hash_for(self.best_number),
				finalized_number: self.best_number,
				finalized_state: None,
				number_leaves: 1,
				block_gap: None,
			}
		}

		fn status(&self, hash: H256) -> BlockchainResult<BlockStatus> {
			Ok(if self.known.contains_key(&hash) {
				BlockStatus::InChain
			} else {
				BlockStatus::Unknown
			})
		}

		fn number(&self, hash: H256) -> BlockchainResult<Option<NumberFor<Block>>> {
			if self.failing.contains(&hash) {
				return Err(sp_blockchain::Error::Backend("simulated db failure".into()));
			}
			Ok(self.known.get(&hash).copied())
		}

		fn hash(&self, number: NumberFor<Block>) -> BlockchainResult<Option<H256>> {
			Ok(self.canonical.get(&number).copied())
		}
	}

	#[test]
	fn defaults_to_best_block() {
		let best = 10 * window();
		let chain = MockChain::with_blocks(best, &[best]);
		assert_eq!(resolve_proof_block(&chain, None).unwrap(), hash_for(best));
	}

	#[test]
	fn accepts_blocks_within_the_proof_window() {
		let best = 10 * window();
		let recent = best - 5;
		let boundary = best - window();
		let chain = MockChain::with_blocks(best, &[best, recent, boundary]);

		assert_eq!(resolve_proof_block(&chain, Some(hash_for(recent))).unwrap(), hash_for(recent));
		// The oldest block whose hash is still on-chain in frame_system::BlockHash.
		assert_eq!(
			resolve_proof_block(&chain, Some(hash_for(boundary))).unwrap(),
			hash_for(boundary)
		);
	}

	/// Proofs at blocks older than `BlockHashCount` can never be verified on-chain
	/// (the wormhole pallet rejects them with BlockNotFound), so the RPC must not
	/// let callers use them to force cold archive-state reads.
	#[test]
	fn rejects_blocks_older_than_the_proof_window() {
		let best = 10 * window();
		let too_old = best - window() - 1;
		let ancient = 1;
		let chain = MockChain::with_blocks(best, &[best, too_old, ancient]);

		let err = resolve_proof_block(&chain, Some(hash_for(too_old)))
			.expect_err("block just outside the window must be rejected");
		assert_eq!(err.code(), 9005);

		assert!(resolve_proof_block(&chain, Some(hash_for(ancient))).is_err());
	}

	/// The backend resolves a number for ANY imported block, including side-fork
	/// blocks — resolvability is not canonicality. A proof generated against fork
	/// state can never settle (the wormhole pallet compares the claimed hash to
	/// `frame_system::BlockHash`, the canonical chain), so the RPC must reject
	/// noncanonical hashes instead of burning state-execution resources on them.
	#[test]
	fn rejects_noncanonical_hashes_within_the_window() {
		let best = 10 * window();
		let fork_height = best - 5;
		let mut chain = MockChain::with_blocks(best, &[best, fork_height]);
		let fork_hash = chain.add_fork_block(fork_height);

		let err = resolve_proof_block(&chain, Some(fork_hash))
			.expect_err("side-fork hash must be rejected even inside the proof window");
		assert_eq!(err.code(), 9008);

		// The canonical block at the same height is still accepted.
		assert_eq!(
			resolve_proof_block(&chain, Some(hash_for(fork_height))).unwrap(),
			hash_for(fork_height)
		);
	}

	/// A backend-known block ABOVE the current best (e.g. from a longer side
	/// fork that was imported but not chosen) makes `best_number - number`
	/// saturate to 0, which the one-sided window check happily accepts. Heights
	/// above best have no canonical hash and can never settle.
	#[test]
	fn rejects_blocks_above_the_best_number() {
		let best = 10 * window();
		let mut chain = MockChain::with_blocks(best, &[best]);
		let ahead_hash = chain.add_fork_block(best + 5);

		let err = resolve_proof_block(&chain, Some(ahead_hash))
			.expect_err("block above best must be rejected");
		assert_eq!(err.code(), 9007);
	}

	#[test]
	fn rejects_unknown_block_hashes() {
		let best = 10 * window();
		let chain = MockChain::with_blocks(best, &[best]);

		let err = resolve_proof_block(&chain, Some(H256::repeat_byte(0xEE)))
			.expect_err("unknown hash must be rejected");
		assert_eq!(err.code(), 9004);
	}

	/// A genuine backend/`number()` failure must not be reported as "unknown
	/// block hash" — operators need to tell a corrupted DB from client garbage.
	#[test]
	fn surfaces_backend_errors_separately_from_unknown_hashes() {
		let best = 10 * window();
		let broken = H256::repeat_byte(0xAB);
		let chain = MockChain::with_number_failure(best, broken);

		let err = resolve_proof_block(&chain, Some(broken))
			.expect_err("backend failure must surface as an error");
		assert_eq!(err.code(), 9006);
		assert!(
			err.message().contains("Failed to resolve block number"),
			"message should name the backend failure, got: {}",
			err.message()
		);
	}
}
