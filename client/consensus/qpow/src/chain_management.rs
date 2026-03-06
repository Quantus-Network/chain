use codec::{Decode, Encode};
use futures::StreamExt;
use primitive_types::{H256, U512};
use sc_client_api::{AuxStore, BlockBackend, BlockchainEvents, Finalizer};
use sc_service::TaskManager;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{Backend, HeaderBackend};
use sp_consensus::{Error as ConsensusError, SelectChain};
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Block as BlockT, Header, One, Zero};
use std::{fmt, marker::PhantomData, sync::Arc};

const IGNORED_CHAINS_PREFIX: &[u8] = b"QPow:IgnoredChains:";
const ACHIEVED_WORK_PREFIX: &[u8] = b"QPow:AchievedWork:";

#[derive(Debug)]
pub enum ChainManagementError {
	ChainLookup(String),
	StateUnavailable(String),
	NoValidChain,
	NoCommonAncestor,
	FailedToAddIgnoredChain(String),
	FailedToFetchLeaves(String),
	FinalizationFailed(String),
	RuntimeApiError(String),
}

impl fmt::Display for ChainManagementError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::ChainLookup(msg) => write!(f, "Chain lookup error: {}", msg),
			Self::StateUnavailable(msg) => write!(f, "State unavailable: {}", msg),
			Self::NoValidChain => write!(f, "No valid chain found"),
			Self::NoCommonAncestor => write!(f, "No common ancestor found"),
			Self::FailedToAddIgnoredChain(msg) => write!(f, "Failed to add ignored chain: {}", msg),
			Self::FailedToFetchLeaves(msg) => write!(f, "Failed to fetch leaves: {}", msg),
			Self::FinalizationFailed(msg) => write!(f, "Finalization failed: {}", msg),
			Self::RuntimeApiError(msg) => write!(f, "Runtime API error: {}", msg),
		}
	}
}

impl std::error::Error for ChainManagementError {}

impl From<ChainManagementError> for ConsensusError {
	fn from(err: ChainManagementError) -> Self {
		match err {
			ChainManagementError::ChainLookup(msg) => ConsensusError::ChainLookup(msg),
			ChainManagementError::StateUnavailable(msg) => ConsensusError::StateUnavailable(msg),
			ChainManagementError::NoValidChain =>
				ConsensusError::ChainLookup("No valid chain found".into()),
			ChainManagementError::NoCommonAncestor =>
				ConsensusError::ChainLookup("No common ancestor found".into()),
			other => ConsensusError::Other(Box::new(other)),
		}
	}
}

/// Store cumulative achieved work for a block in auxiliary storage.
/// This is used for chain selection based on achieved difficulty.
pub fn store_cumulative_achieved_work<B: BlockT, C: AuxStore>(
	client: &C,
	block_hash: B::Hash,
	cumulative_work: U512,
) -> Result<(), sp_blockchain::Error> {
	let key = [ACHIEVED_WORK_PREFIX, block_hash.as_ref()].concat();
	client.insert_aux(&[(&key[..], &cumulative_work.encode()[..])], &[])?;
	log::debug!(
		target: "qpow",
		"Stored cumulative achieved work {} for block {:?}",
		cumulative_work,
		block_hash
	);
	Ok(())
}

/// Get cumulative achieved work for a block from auxiliary storage.
/// Returns U512::zero() if not found (e.g., for genesis block before initialization).
pub fn get_cumulative_achieved_work<B: BlockT, C: AuxStore>(
	client: &C,
	block_hash: B::Hash,
) -> Result<U512, sp_blockchain::Error> {
	let key = [ACHIEVED_WORK_PREFIX, block_hash.as_ref()].concat();
	match client.get_aux(&key)? {
		Some(bytes) => {
			let work = U512::decode(&mut &bytes[..]).map_err(|e| {
				sp_blockchain::Error::Backend(format!(
					"Failed to decode cumulative work for {:?}: {:?}",
					block_hash, e
				))
			})?;
			Ok(work)
		},
		None => {
			log::trace!(
				target: "qpow",
				"No cumulative achieved work found for block {:?}, returning zero",
				block_hash
			);
			Ok(U512::zero())
		},
	}
}

/// Initialize the genesis block's achieved work if not already set.
/// Genesis block has achieved work = 1 (no mining, but represents the start of the chain).
/// This should be called during node startup.
pub fn initialize_genesis_achieved_work<B: BlockT, C: AuxStore + HeaderBackend<B>>(
	client: &C,
) -> Result<(), sp_blockchain::Error> {
	// Get genesis hash
	let genesis_hash = client
		.hash(Zero::zero())?
		.ok_or_else(|| sp_blockchain::Error::Backend("Genesis block not found".to_string()))?;

	// Check if already initialized
	let existing = get_cumulative_achieved_work::<B, C>(client, genesis_hash)?;
	if existing != U512::zero() {
		log::debug!(
			target: "qpow",
			"Genesis achieved work already initialized to {}",
			existing
		);
		return Ok(());
	}

	// Initialize genesis achieved work to 1
	let genesis_work = U512::one();
	store_cumulative_achieved_work::<B, C>(client, genesis_hash, genesis_work)?;
	log::info!(
		target: "qpow",
		"Initialized genesis block {:?} achieved work to {}",
		genesis_hash,
		genesis_work
	);

	Ok(())
}

/// Get chain work using achieved difficulty from auxiliary storage.
/// This is the new chain selection metric based on actual work done.
pub fn get_chain_work<B, C>(client: &C, at_hash: B::Hash) -> Result<U512, sp_consensus::Error>
where
	B: BlockT,
	C: AuxStore,
{
	get_cumulative_achieved_work::<B, C>(client, at_hash).map_err(|e| {
		sp_consensus::Error::Other(
			format!("Failed to get cumulative achieved work: {:?}", e).into(),
		)
	})
}

pub fn is_heavier<N: PartialOrd>(
	candidate_work: U512,
	candidate_number: N,
	current_work: U512,
	current_number: N,
) -> bool {
	candidate_work > current_work ||
		(candidate_work == current_work && candidate_number > current_number)
}

pub struct HeaviestChain<B, C, BE>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore,
	BE: sc_client_api::Backend<B>,
{
	backend: Arc<BE>,
	client: Arc<C>,
	_phantom: PhantomData<B>,
}

impl<B, C, BE> Clone for HeaviestChain<B, C, BE>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore,
	BE: sc_client_api::Backend<B>,
{
	fn clone(&self) -> Self {
		Self {
			backend: Arc::clone(&self.backend),
			client: Arc::clone(&self.client),
			_phantom: PhantomData,
		}
	}
}

impl<B, C, BE> HeaviestChain<B, C, BE>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore + Send + Sync + 'static,
	C::Api: QPoWApi<B>,
	BE: sc_client_api::Backend<B> + 'static,
{
	pub fn new(backend: Arc<BE>, client: Arc<C>) -> Self {
		log::debug!("Creating new HeaviestChain instance");

		Self { backend, client, _phantom: PhantomData }
	}

	/// Finalizes blocks that are `max_reorg_depth - 1` blocks behind the current best block
	pub fn finalize_canonical_at_depth(&self) -> Result<(), ConsensusError>
	where
		C: Finalizer<B, BE>,
	{
		log::debug!("✓✓✓ Starting finalization process");

		// Get the current best block
		let best_hash = self.client.info().best_hash;
		log::debug!("Current best hash: {:?}", best_hash);

		if best_hash == Default::default() {
			log::debug!("✓ No blocks to finalize - best hash is default");
			return Ok(()); // No blocks to finalize
		}

		let best_header = self
			.client
			.header(best_hash)
			.map_err(|e| {
				log::error!("Failed to get header for best hash: {:?}, error: {:?}", best_hash, e);
				ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
			})?
			.ok_or_else(|| {
				log::error!("Missing header for best hash: {:?}", best_hash);
				ChainManagementError::ChainLookup("Missing current best header".into())
			})?;

		let best_number = *best_header.number();
		log::debug!("Current best block number: {}", best_number);

		let max_reorg_depth =
			self.client.runtime_api().get_max_reorg_depth(best_hash).map_err(|e| {
				log::error!("Failed to get max reorg depth: {:?}", e);
				ChainManagementError::RuntimeApiError(format!(
					"Failed to get max reorg depth: {:?}",
					e
				))
			})?;

		// Calculate how far back to finalize
		let finalize_depth = max_reorg_depth.saturating_sub(1);

		// Only finalize if we have enough blocks
		if best_number <= finalize_depth.into() {
			log::debug!(
				"✓ Chain not long enough for finalization. Best number: {}, Required: > {}",
				best_number,
				finalize_depth
			);
			return Ok(()); // Chain not long enough yet
		}

		// Calculate block number to finalize
		let finalize_number = best_number - finalize_depth.into();
		log::debug!("Target block number to finalize: {}", finalize_number);

		// Get the hash for that block number in the current canonical chain
		let finalize_hash = self
			.client
			.hash(finalize_number)
			.map_err(|e| {
				log::error!("Failed to get hash for block #{}: {:?}", finalize_number, e);
				ChainManagementError::ChainLookup(format!(
					"Failed to get hash at #{}: {:?}",
					finalize_number, e
				))
			})?
			.ok_or_else(|| {
				log::error!("No block found at #{} for finalization", finalize_number);
				ChainManagementError::ChainLookup(format!("No block found at #{}", finalize_number))
			})?;

		log::debug!("✓ Found hash for finalization target: {:?}", finalize_hash);

		// Get last finalized block before attempting finalization
		let last_finalized_before = self.client.info().finalized_number;
		log::debug!("Last finalized block before attempt: #{}", last_finalized_before);

		// Finalize the block
		self.client.finalize_block(finalize_hash, None, true).map_err(|e| {
			log::error!(
				"Failed to finalize block #{} ({:?}): {:?}",
				finalize_number,
				finalize_hash,
				e
			);
			ChainManagementError::FinalizationFailed(format!(
				"Failed to finalize block #{}: {:?}",
				finalize_number, e
			))
		})?;

		// Check if finalization was successful
		let last_finalized_after = self.client.info().finalized_number;

		log::debug!(
			"✓ Finalization stats: best={}, finalized={}, finalize_depth={}, target_finalize={}",
			best_number,
			last_finalized_after,
			finalize_depth,
			finalize_number
		);

		log::debug!("✓ Finalized block #{} ({:?})", finalize_number, finalize_hash);

		Ok(())
	}

	fn evaluate_leaf(
		&self,
		leaf_hash: B::Hash,
	) -> Result<Option<(B::Header, U512)>, ConsensusError> {
		let header = self
			.client
			.header(leaf_hash)
			.map_err(|e| {
				log::error!(
					target: "qpow",
					"Blockchain error when getting header for leaf {:?}: {:?}",
					leaf_hash,
					e
				);
				ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
			})?
			.ok_or_else(|| {
				log::error!("Missing header for leaf {:?}", leaf_hash);
				ChainManagementError::ChainLookup(format!("Missing header for {:?}", leaf_hash))
			})?;

		match self.try_calculate_chain_work(&header)? {
			Some(work) => Ok(Some((header, work))),
			None => {
				log::warn!(
					target: "qpow",
					"Skipping leaf #{} ({:?}) - block state was pruned. Adding to ignored chains.",
					header.number(),
					leaf_hash
				);
				if let Err(e) = self.add_ignored_chain(leaf_hash) {
					log::warn!(
						target: "qpow",
						"Failed to add pruned leaf {:?} to ignored chains: {:?}",
						leaf_hash,
						e
					);
				}
				Ok(None)
			},
		}
	}

	fn try_calculate_chain_work(
		&self,
		chain_head: &B::Header,
	) -> Result<Option<U512>, ConsensusError> {
		let current_hash = chain_head.hash();
		let current_number = *chain_head.number();

		// Use achieved work from aux storage instead of runtime's TotalWork
		match get_cumulative_achieved_work::<B, C>(&*self.client, current_hash) {
			Ok(total_work) => {
				if total_work == U512::zero() {
					// No achieved work stored - this block hasn't been imported with the new logic
					log::warn!(
						"No achieved work found in aux storage for chain head #{} (hash: {:?})",
						current_number,
						current_hash
					);
					Ok(None)
				} else {
					log::info!(
						"⛏️ Total achieved work: {:?} for chain with head at #{:?} hash: {:?}",
						total_work,
						current_number,
						current_hash
					);
					Ok(Some(total_work))
				}
			},
			Err(e) => {
				log::error!(
					"Failed to get achieved work for chain with head #{}: {:?}",
					current_number,
					e
				);
				Err(ChainManagementError::ChainLookup(format!(
					"Failed to get achieved work: {:?}",
					e
				))
				.into())
			},
		}
	}

	/// Method to find best chain when there's no current best header
	async fn find_best_chain(&self, leaves: Vec<B::Hash>) -> Result<B::Header, ConsensusError> {
		log::debug!(target: "qpow", "Finding best chain among {} leaves when no current best exists", leaves.len());

		let mut best_header = None;
		let mut best_work = U512::zero();

		for (idx, leaf_hash) in leaves.iter().enumerate() {
			log::debug!(target: "qpow", "Checking leaf [{}/{}]: {:?}", idx + 1, leaves.len(), leaf_hash);

			let (header, chain_work) = match self.evaluate_leaf(*leaf_hash)? {
				Some(result) => result,
				None => continue,
			};

			let header_number = *header.number();
			log::debug!("Chain work for leaf #{}: {}", header_number, chain_work);

			let current_best_number =
				best_header.as_ref().map(|h: &B::Header| *h.number()).unwrap_or_else(Zero::zero);
			if is_heavier(chain_work, header_number, best_work, current_best_number) {
				log::debug!(
					target: "qpow",
					"Found new best chain candidate: #{} (hash: {:?}) with work: {}",
					header_number,
					leaf_hash,
					chain_work
				);
				best_work = chain_work;
				best_header = Some(header);
			} else {
				log::debug!(
					target: "qpow",
					"Leaf #{} (hash: {:?}) has less work ({}) than current best ({})",
					header_number,
					leaf_hash,
					chain_work,
					best_work
				);
			}
		}

		if let Some(ref header) = best_header {
			log::info!(
				"Selected best chain with head at #{} (hash: {:?}) with total work: {}",
				header.number(),
				header.hash(),
				best_work
			);
		} else {
			log::warn!("No valid chain found among {} leaves", leaves.len());
		}

		best_header.ok_or(ChainManagementError::NoValidChain.into())
	}

	/// Method to find Re-Org depth and fork-point
	fn find_common_ancestor_and_depth(
		&self,
		current_best: &B::Header,
		competing_header: &B::Header,
	) -> Result<(B::Hash, u32), ConsensusError> {
		let current_best_hash = current_best.hash();
		let competing_hash = competing_header.hash();
		let current_height = *current_best.number();
		let competing_height = *competing_header.number();

		log::debug!(
			"Finding common ancestor between current best #{} ({:?}) and competing #{} ({:?})",
			current_height,
			current_best_hash,
			competing_height,
			competing_hash
		);

		// Quick check for identical headers
		if current_best_hash == competing_hash {
			log::debug!("Headers are identical, no reorganization needed");
			return Ok((current_best_hash, 0));
		}

		let mut current_best_hash = current_best_hash;
		let mut competing_hash = competing_hash;
		let mut current_height = current_height;
		let mut competing_height = competing_height;
		let mut reorg_depth = 0;

		// First, move the headers to the same height
		log::debug!(
			"Phase 1: Aligning heights - current: {}, competing: {}",
			current_height,
			competing_height
		);

		while current_height > competing_height {
			// Check if the blocks are identical during descent
			if current_best_hash == competing_hash {
				// Fork point found early
				log::debug!(
                    "Early fork point found during height alignment: {:?} at height {} with reorg depth {}",
                    current_best_hash,
                    current_height,
                    reorg_depth
                );
				return Ok((current_best_hash, reorg_depth));
			}

			log::debug!(
				"Current chain higher: moving down from #{} ({:?}), reorg_depth: {}",
				current_height,
				current_best_hash,
				reorg_depth
			);

			current_best_hash = *self
				.client
				.header(current_best_hash)
				.map_err(|e| {
					log::error!(
						"Blockchain error when getting header for #{}: {:?}",
						current_height,
						e
					);
					ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
				})?
				.ok_or_else(|| {
					log::error!("Missing header at #{} ({:?})", current_height, current_best_hash);
					ChainManagementError::ChainLookup("Missing header".into())
				})?
				.parent_hash();

			current_height -= One::one();
			reorg_depth += 1;

			log::debug!(
				"Moved down current chain to #{} ({:?}), reorg_depth now: {}",
				current_height,
				current_best_hash,
				reorg_depth
			);
		}

		// Similarly, if the competing chain is taller, move it down to the same height
		log::debug!(
			"Phase 2: Aligning heights if competing chain is taller - current: {}, competing: {}",
			current_height,
			competing_height
		);

		while competing_height > current_height {
			reorg_depth += 1;
			log::debug!(
				"Competing chain higher: moving down from #{} ({:?})",
				competing_height,
				competing_hash
			);

			competing_hash = *self
				.client
				.header(competing_hash)
				.map_err(|e| {
					log::error!(
						"Blockchain error when getting header for competing chain #{}: {:?}",
						competing_height,
						e
					);
					ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
				})?
				.ok_or_else(|| {
					log::error!(
						"Missing header for competing chain at #{} ({:?})",
						competing_height,
						competing_hash
					);
					ChainManagementError::ChainLookup("Missing header".into())
				})?
				.parent_hash();

			competing_height -= One::one();

			log::debug!(
				"Moved down competing chain to #{} ({:?})",
				competing_height,
				competing_hash
			);
		}

		log::debug!("Phase 3: Both chains now at height {} - finding fork point", current_height);

		// Now both headers are at the same height
		// Find the fork-point by traversing backwards
		while current_best_hash != competing_hash {
			// If we reach genesis and still no match, no common ancestor
			if current_height.is_zero() {
				log::error!("Reached genesis block without finding common ancestor");
				return Err(ChainManagementError::NoCommonAncestor.into());
			}

			log::debug!(
				"Blocks at #{} differ: current ({:?}) vs competing ({:?})",
				current_height,
				current_best_hash,
				competing_hash
			);

			// Move down one block in the current best chain
			current_best_hash = *self
				.client
				.header(current_best_hash)
				.map_err(|e| {
					log::error!(
						"Blockchain error when getting parent at #{}: {:?}",
						current_height,
						e
					);
					ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
				})?
				.ok_or_else(|| {
					log::error!(
						"Missing header for parent at #{} ({:?})",
						current_height,
						current_best_hash
					);
					ChainManagementError::ChainLookup("Missing header".into())
				})?
				.parent_hash();

			// Move down one block in the competing chain
			competing_hash = *self
				.client
				.header(competing_hash)
				.map_err(|e| {
					log::error!(
						"Blockchain error when getting parent for competing chain at #{}: {:?}",
						current_height,
						e
					);
					ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
				})?
				.ok_or_else(|| {
					log::error!(
						"Missing header for competing chain parent at #{} ({:?})",
						current_height,
						competing_hash
					);
					ChainManagementError::ChainLookup("Missing header".into())
				})?
				.parent_hash();

			// Both chains are now one block lower
			current_height -= One::one();
			// Each step backwards increases the reorganization depth
			reorg_depth += 1;

			log::debug!(
				"Moved both chains down to #{}, current ({:?}), competing ({:?}), reorg_depth: {}",
				current_height,
				current_best_hash,
				competing_hash,
				reorg_depth
			);
		}

		// Log the fork point and reorg depth for debugging
		log::warn!(
			"Fork-point ----------------------- found: {:?} at height: {:?} with reorg depth: {}",
			current_best_hash,
			current_height,
			reorg_depth
		);

		Ok((current_best_hash, reorg_depth))
	}

	fn is_chain_ignored(&self, hash: &B::Hash) -> Result<bool, ConsensusError> {
		log::debug!("Checking if chain with head {:?} is ignored", hash);

		let key = ignored_chain_key(hash);

		match self.client.get_aux(&key) {
			Ok(Some(_)) => {
				log::debug!("Chain with head {:?} is ignored", hash);
				Ok(true)
			},
			Ok(None) => {
				log::debug!("Chain with head {:?} is not ignored", hash);
				Ok(false)
			},
			Err(e) => {
				log::error!("Failed to check if chain with head {:?} is ignored: {:?}", hash, e);
				Err(ChainManagementError::FailedToAddIgnoredChain(format!(
					"Failed to check ignored chain: {:?}",
					e
				))
				.into())
			},
		}
	}

	fn add_ignored_chain(&self, hash: B::Hash) -> Result<(), ConsensusError> {
		log::debug!("Adding chain with head {:?} to ignored chains", hash);

		let key = ignored_chain_key(&hash);

		//This storage isn't super advanced. We can only add or remove value, updates are
		// impossible.

		let empty_value = vec![];

		self.client
			.insert_aux(&[(key.as_slice(), empty_value.as_slice())], &[])
			.map_err(|e| {
				log::error!("Failed to add chain with head {:?} to ignored chains: {:?}", hash, e);
				ChainManagementError::FailedToAddIgnoredChain(format!(
					"Failed to add ignored chain: {:?}",
					e
				))
				.into()
			})
	}
}

#[async_trait::async_trait]
impl<B, C, BE> SelectChain<B> for HeaviestChain<B, C, BE>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore + Send + Sync + 'static,
	C::Api: QPoWApi<B>,
	BE: sc_client_api::Backend<B> + 'static,
{
	async fn leaves(&self) -> Result<Vec<B::Hash>, ConsensusError> {
		log::debug!("Getting blockchain leaves");

		let leaves = self.backend.blockchain().leaves().map_err(|e| {
			log::error!("Failed to fetch leaves: {:?}", e);
			ChainManagementError::FailedToFetchLeaves(format!("Failed to fetch leaves: {:?}", e))
		})?;

		log::debug!("Found {} leaves", leaves.len());

		Ok(leaves)
	}

	async fn best_chain(&self) -> Result<B::Header, ConsensusError> {
		log::debug!(target: "qpow", "------ 🍴️Starting best chain selection process ------");

		let leaves = self.backend.blockchain().leaves().map_err(|e| {
			log::error!("🍴️ Failed to fetch leaves: {:?}", e);
			ChainManagementError::FailedToFetchLeaves(format!("Failed to fetch leaves: {:?}", e))
		})?;

		log::debug!(target: "qpow", "🍴️ Found {} leaves to evaluate", leaves.len());

		if leaves.is_empty() {
			log::error!("🍴️ Blockchain has no leaves");
			return Err(ChainManagementError::NoValidChain.into());
		}

		// Get info about last finalized block
		let finalized_number = self.client.info().finalized_number;
		log::debug!(target: "qpow", "🍴️ Current finalized block: #{}", finalized_number);

		// the current head of the chain - will be needed to compare reorg depth
		let current_best = match self.client.info().best_hash {
			hash if hash != Default::default() => {
				log::debug!(target: "qpow", "🍴️ Current best hash: {:?}", hash);

				self.client
					.header(hash)
					.map_err(|e| {
						log::error!(
							"🍴️ Blockchain error when getting header for best hash: {:?}",
							e
						);
						ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
					})?
					.ok_or_else(|| {
						log::error!("🍴️ Missing header for current best hash: {:?}", hash);
						ChainManagementError::ChainLookup("Missing current best header".into())
					})?
			},
			_ => {
				// If there's no current best, we don't need to find reorg depth
				log::debug!(
					target: "qpow",
					"🍴️ No current best hash, finding best chain without reorg constraints"
				);
				return self.find_best_chain(leaves).await;
			},
		};

		let current_best_number = *current_best.number();
		log::debug!(
			target: "qpow",
			"🍴️ Current best block: #{} ({:?})",
			current_best_number,
			current_best.hash()
		);

		let mut best_header = current_best.clone();
		let mut best_work = match self.try_calculate_chain_work(&current_best)? {
			Some(work) => work,
			None => {
				// Emergency fallback: current best has pruned state. Evaluate all leaves without
				// reorg depth constraints. Note: this bypasses max_reorg_depth for the fallback
				// path, which is acceptable since we have no valid current best to compare against.
				log::warn!(
					target: "qpow",
					"🍴️ Current best block state was pruned. Falling back to evaluating all leaves."
				);
				return self.find_best_chain(leaves).await;
			},
		};
		log::debug!(
			target: "qpow",
			"🍴️ Current best chain: {:?} with work: {:?}",
			best_header.hash(),
			best_work
		);

		log::debug!(
			target: "qpow",
			"🍴️ Evaluating {} leaves for potential best chain",
			leaves.len()
		);

		for (idx, leaf_hash) in leaves.iter().enumerate() {
			log::debug!(
				target: "qpow",
				"🍴️ Evaluating leaf [{}/{}]: {:?}",
				idx + 1,
				leaves.len(),
				leaf_hash
			);

			// Skip if it's the current best or already ignored
			if *leaf_hash == best_header.hash() {
				log::debug!(target: "qpow", "🍴️ Skipping leaf {:?} - it's the current best", leaf_hash);
				continue;
			}

			if self.is_chain_ignored(leaf_hash)? {
				log::debug!(
					target: "qpow",
					"🍴️ Skipping leaf {:?} - it's in the ignored list",
					leaf_hash
				);
				continue;
			}

			let (header, chain_work) = match self.evaluate_leaf(*leaf_hash)? {
				Some(result) => result,
				None => continue,
			};

			let header_number = *header.number();
			log::debug!(target: "qpow", "🍴️ Chain work for leaf #{}: {}", header_number, chain_work);

			let max_reorg_depth =
				self.client.runtime_api().get_max_reorg_depth(best_header.hash()).map_err(|e| {
					log::error!(target: "qpow", "Failed to get max reorg depth: {:?}", e);
					ChainManagementError::RuntimeApiError(format!(
						"Failed to get max reorg depth: {:?}",
						e
					))
				})?;
			log::debug!(target: "qpow", "🍴️ Max reorg depth from runtime: {}", max_reorg_depth);

			if chain_work >= best_work {
				// This chain has more work, but we need to check reorg depth
				log::debug!(
					target: "qpow",
					"🍴️ Chain with head #{} ({:?}) has at least as much work ({}) as current best ({}), checking reorg depth",
					header_number,
					leaf_hash,
					chain_work,
					best_work
				);

				let (fork_point, reorg_depth) =
					self.find_common_ancestor_and_depth(&current_best, &header)?;
				log::debug!(
					target: "qpow",
					"🍴️ Found common ancestor with hash {:?} with reorg depth: {}",
					fork_point,
					reorg_depth
				);

				if reorg_depth <= max_reorg_depth {
					// Switch to this chain as it's within the reorg limit
					log::debug!(
						target: "qpow",
						"🍴️ Found better chain: {:?} with work: {:?}, reorg depth: {} (within limit of {})",
						header.hash(),
						chain_work,
						reorg_depth,
						max_reorg_depth
					);

					if is_heavier(chain_work, *header.number(), best_work, *best_header.number()) {
						log::debug!(
							target: "qpow",
							"🍴️ Switching to heavier chain: work {} vs {}, height #{} vs #{}",
							chain_work, best_work, header.number(), best_header.number()
						);
						best_work = chain_work;
						best_header = header;
					}
				} else {
					log::debug!(
						target: "qpow",
						"🍴️ Chain with more work exceeds reorg limit: {} > {}. Adding to ignored chains.",
						reorg_depth,
						max_reorg_depth
					);

					self.add_ignored_chain(*leaf_hash)?;
					log::warn!(
						target: "qpow",
						"🍴️ Permanently ignoring chain with more work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
						header.hash(),
						chain_work,
						reorg_depth,
						max_reorg_depth
					);
				}
			} else {
				// This chain has less work - check if it should be ignored
				log::debug!(
					target: "qpow",
					"🍴️ Chain has less work ({} < {}), checking if it should be ignored",
					chain_work,
					best_work
				);

				let (fork_point, reorg_depth) =
					self.find_common_ancestor_and_depth(&current_best, &header)?;
				log::debug!(
					target: "qpow",
					"🍴️ Found common ancestor with hash {:?} with reorg depth: {}",
					fork_point,
					reorg_depth
				);

				if reorg_depth > max_reorg_depth {
					log::debug!(
						target: "qpow",
						"🍴️ Chain exceeds reorg limit: {} > {}. Adding to ignored chains.",
						reorg_depth,
						max_reorg_depth
					);

					self.add_ignored_chain(*leaf_hash)?;
					log::debug!(
						target: "qpow",
						"🍴️ Permanently ignoring chain with less work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
						leaf_hash,
						chain_work,
						reorg_depth,
						max_reorg_depth
					);
				} else {
					log::debug!(
						target: "qpow",
						"🍴️ Chain has less work but is within reorg limit: {} <= {}. Keeping in consideration.",
						reorg_depth,
						max_reorg_depth
					);
				}
			}
		}

		if leaves.len() > 1 {
			log::info!(
				"🍴️ Evaluated {} leaves and selected best chain with head: #{} ({:?}) and work: {}",
				leaves.len(),
				best_header.number(),
				best_header.hash(),
				best_work
			);
		} else {
			log::debug!(
				target: "qpow",
				"🍴️ Finished chain selection. Selected best chain with head: #{} ({:?}) and work: {}",
				best_header.number(),
				best_header.hash(),
				best_work
			);
		}

		Ok(best_header)
	}
}

fn ignored_chain_key<T: AsRef<[u8]>>(hash: &T) -> Vec<u8> {
	IGNORED_CHAINS_PREFIX.iter().chain(hash.as_ref()).copied().collect()
}

pub struct ChainManagement;

impl ChainManagement {
	/// Start a task that listens for block imports and triggers finalization
	pub fn spawn_finalization_task<B, C, BE>(
		select_chain: Arc<HeaviestChain<B, C, BE>>,
		task_manager: &TaskManager,
	) where
		B: BlockT<Hash = H256>,
		C: ProvideRuntimeApi<B>
			+ HeaderBackend<B>
			+ BlockBackend<B>
			+ AuxStore
			+ BlockchainEvents<B>
			+ Finalizer<B, BE>
			+ Send
			+ Sync
			+ 'static,
		C::Api: QPoWApi<B>,
		BE: sc_client_api::Backend<B> + 'static,
	{
		log::info!("⛓️ Spawning chain finalization task");

		task_manager
			.spawn_essential_handle()
			.spawn("chain_finalization", None, async move {
				log::info!("⛓️ Chain finalization task spawned");

				let mut import_notification_stream =
					select_chain.client.every_import_notification_stream();
				log::debug!("⛓️ Listening for block import notifications");

				while let Some(notification) = import_notification_stream.next().await {
					if let Err(e) = select_chain.finalize_canonical_at_depth() {
						log::error!("⛓️ Failed to finalize blocks: {:?}", e);
					} else {
						log::debug!(
							"⛓️ Successfully processed finalization after import of block #{}",
							notification.header.number()
						);
					}
				}

				log::info!("Block import notification stream ended");
			});
	}
}
