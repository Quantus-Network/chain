use codec::{Decode, Encode};
use primitive_types::{H256, U512};
use sc_client_api::{AuxStore, BlockBackend, Finalizer};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{Backend, HeaderBackend};
use sp_consensus::{Error as ConsensusError, SelectChain};
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Block as BlockT, Header, Zero};
use std::{fmt, marker::PhantomData, sync::Arc};

const ACHIEVED_WORK_PREFIX: &[u8] = b"QPow:AchievedWork:";

#[derive(Debug)]
pub enum ChainManagementError {
	ChainLookup(String),
	NoValidChain,
	FailedToFetchLeaves(String),
	FinalizationFailed(String),
	RuntimeApiError(String),
}

impl fmt::Display for ChainManagementError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::ChainLookup(msg) => write!(f, "Chain lookup error: {}", msg),
			Self::NoValidChain => write!(f, "No valid chain found"),
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
			ChainManagementError::NoValidChain =>
				ConsensusError::ChainLookup("No valid chain found".into()),
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

/// Finalizes blocks that are `max_reorg_depth - 1` blocks behind the current best block.
/// This should be called synchronously after each block import to ensure finalization
/// happens before the next block is imported.
pub fn finalize_canonical_at_depth<B, C, BE>(client: &C) -> Result<(), ConsensusError>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + Finalizer<B, BE>,
	C::Api: QPoWApi<B>,
	BE: sc_client_api::Backend<B>,
{
	log::debug!("✓✓✓ Starting finalization process");

	// Get the current best block
	let best_hash = client.info().best_hash;
	log::debug!("Current best hash: {:?}", best_hash);

	if best_hash == Default::default() {
		log::debug!("✓ No blocks to finalize - best hash is default");
		return Ok(()); // No blocks to finalize
	}

	let best_header = client
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
	log::debug!("Current best block number: {:?}", best_number);

	let max_reorg_depth = client.runtime_api().get_max_reorg_depth(best_hash).map_err(|e| {
		log::error!("Failed to get max reorg depth: {:?}", e);
		ChainManagementError::RuntimeApiError(format!("Failed to get max reorg depth: {:?}", e))
	})?;

	// Calculate how far back to finalize
	let finalize_depth = max_reorg_depth.saturating_sub(1);

	// Only finalize if we have enough blocks
	if best_number <= finalize_depth.into() {
		log::debug!(
			"✓ Chain not long enough for finalization. Best number: {:?}, Required: > {}",
			best_number,
			finalize_depth
		);
		return Ok(()); // Chain not long enough yet
	}

	// Calculate block number to finalize
	let finalize_number = best_number - finalize_depth.into();
	log::debug!("Target block number to finalize: {:?}", finalize_number);

	// Get the hash for that block number in the current canonical chain
	let finalize_hash = client
		.hash(finalize_number)
		.map_err(|e| {
			log::error!("Failed to get hash for block #{:?}: {:?}", finalize_number, e);
			ChainManagementError::ChainLookup(format!(
				"Failed to get hash at #{:?}: {:?}",
				finalize_number, e
			))
		})?
		.ok_or_else(|| {
			log::error!("No block found at #{:?} for finalization", finalize_number);
			ChainManagementError::ChainLookup(format!("No block found at #{:?}", finalize_number))
		})?;

	log::debug!("✓ Found hash for finalization target: {:?}", finalize_hash);

	// Get last finalized block before attempting finalization
	let last_finalized_before = client.info().finalized_number;
	log::debug!("Last finalized block before attempt: {:?}", last_finalized_before);

	// Finalize the block
	client.finalize_block(finalize_hash, None, true).map_err(|e| {
		log::error!(
			"Failed to finalize block #{:?} ({:?}): {:?}",
			finalize_number,
			finalize_hash,
			e
		);
		ChainManagementError::FinalizationFailed(format!(
			"Failed to finalize block #{:?}: {:?}",
			finalize_number, e
		))
	})?;

	// Check if finalization was successful
	let last_finalized_after = client.info().finalized_number;

	log::debug!(
		"✓ Finalization stats: best={:?}, finalized={:?}, finalize_depth={}, target_finalize={:?}",
		best_number,
		last_finalized_after,
		finalize_depth,
		finalize_number
	);

	log::debug!("✓ Finalized block #{:?} ({:?})", finalize_number, finalize_hash);

	Ok(())
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
		self.backend.blockchain().leaves().map_err(|e| {
			ChainManagementError::FailedToFetchLeaves(format!("Failed to fetch leaves: {:?}", e))
				.into()
		})
	}

	/// Returns the current best chain header.
	///
	/// Since finalization now happens synchronously during block import,
	/// the client's best_hash is always authoritative. The fork choice is
	/// determined during import based on achieved work, and finalization
	/// prunes competing forks that are beyond max_reorg_depth.
	async fn best_chain(&self) -> Result<B::Header, ConsensusError> {
		let best_hash = self.client.info().best_hash;

		self.client
			.header(best_hash)
			.map_err(|e| {
				ChainManagementError::ChainLookup(format!("Failed to get best header: {:?}", e))
			})?
			.ok_or_else(|| ChainManagementError::NoValidChain.into())
	}
}
