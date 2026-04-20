mod chain_management;
mod worker;

pub use chain_management::{
	delete_cumulative_achieved_work, finalize_canonical_at_depth, get_chain_work,
	get_cumulative_achieved_work, initialize_genesis_achieved_work, is_heavier,
	store_cumulative_achieved_work, ChainManagementError,
};
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sp_api::ProvideRuntimeApi;
use sp_consensus_qpow::{QPoWApi, Seal as RawSeal};
use sp_runtime::traits::Block as BlockT;
use std::{marker::PhantomData, sync::Arc, time::Duration};

use crate::worker::UntilImportedOrTransaction;
pub use crate::worker::{MiningBuild, MiningHandle, MiningMetadata, RebuildTrigger};
use futures::{Future, Stream, StreamExt};
use log::*;
use prometheus_endpoint::Registry;
use sc_client_api::{self, backend::AuxStore, BlockOf, BlockchainEvents};
use sc_consensus::{
	BasicQueue, BlockCheckParams, BlockImport, BlockImportParams, BoxBlockImport,
	BoxJustificationImport, ForkChoiceStrategy, ImportResult, JustificationSyncLink, Verifier,
};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::{Environment, Error as ConsensusError, Proposer, SyncOracle};
use sp_consensus_qpow::POW_ENGINE_ID;

use sp_inherents::{CreateInherentDataProviders, InherentDataProvider};
use sp_runtime::{
	generic::{Digest, DigestItem},
	traits::Header as HeaderT,
};

const LOG_TARGET: &str = "pow";

#[derive(Debug, thiserror::Error)]
pub enum Error<B: BlockT> {
	#[error("Header uses the wrong engine {0:?}")]
	WrongEngine([u8; 4]),
	#[error("Header {0:?} is unsealed")]
	HeaderUnsealed(B::Hash),
	#[error("PoW validation error: invalid seal")]
	InvalidSeal,
	#[error("PoW validation error: preliminary verification failed")]
	FailedPreliminaryVerify,
	#[error("Rejecting block too far in future")]
	TooFarInFuture,
	#[error("Fetching best header failed: {0}")]
	BestHeader(sp_blockchain::Error),
	#[error("Best header does not exist")]
	NoBestHeader,
	#[error("Block proposing error: {0}")]
	BlockProposingError(String),
	#[error("Error with block built on {0:?}: {1}")]
	BlockBuiltError(B::Hash, ConsensusError),
	#[error("Creating inherents failed: {0}")]
	CreateInherents(sp_inherents::Error),
	#[error("Checking inherents failed: {0}")]
	CheckInherents(sp_inherents::Error),
	#[error(
		"Checking inherents unknown error for identifier: {}",
		String::from_utf8_lossy(.0)
	)]
	CheckInherentsUnknownError(sp_inherents::InherentIdentifier),
	#[error("Multiple pre-runtime digests")]
	MultiplePreRuntimeDigests,
	#[error(transparent)]
	Client(sp_blockchain::Error),
	#[error(transparent)]
	Codec(codec::Error),
	#[error("{0}")]
	Environment(String),
	#[error("{0}")]
	Runtime(String),
	#[error("{0}")]
	Other(String),
}

impl<B: BlockT> From<Error<B>> for String {
	fn from(error: Error<B>) -> String {
		error.to_string()
	}
}

impl<B: BlockT> From<Error<B>> for ConsensusError {
	fn from(error: Error<B>) -> ConsensusError {
		ConsensusError::ClientImport(error.to_string())
	}
}

/// A block importer for PoW.
pub struct PowBlockImport<B: BlockT<Hash = H256>, I, C, CIDP, BE, const LOGGING_FREQUENCY: u64> {
	inner: I,
	client: Arc<C>,
	create_inherent_data_providers: Arc<CIDP>,
	check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
	_backend: PhantomData<BE>,
}

impl<
		B: BlockT<Hash = H256>,
		I: Clone,
		C: ProvideRuntimeApi<B>,
		CIDP,
		BE,
		const LOGGING_FREQUENCY: u64,
	> Clone for PowBlockImport<B, I, C, CIDP, BE, LOGGING_FREQUENCY>
{
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
			client: self.client.clone(),
			create_inherent_data_providers: self.create_inherent_data_providers.clone(),
			check_inherents_after: self.check_inherents_after,
			_backend: PhantomData,
		}
	}
}

impl<B, I, C, CIDP, BE, const LOGGING_FREQUENCY: u64>
	PowBlockImport<B, I, C, CIDP, BE, LOGGING_FREQUENCY>
where
	B: BlockT<Hash = H256>,
	I: BlockImport<B> + Send + Sync,
	I::Error: Into<ConsensusError>,
	C: ProvideRuntimeApi<B>
		+ BlockBackend<B>
		+ Send
		+ Sync
		+ HeaderBackend<B>
		+ AuxStore
		+ BlockOf
		+ 'static,
	C::Api: QPoWApi<B>,
	C::Api: BlockBuilderApi<B>,
	CIDP: CreateInherentDataProviders<B, ()>,
	BE: sc_client_api::Backend<B>,
{
	/// Create a new block import suitable to be used in PoW
	pub fn new(
		inner: I,
		client: Arc<C>,
		check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
		create_inherent_data_providers: CIDP,
	) -> Self {
		Self {
			inner,
			client,
			check_inherents_after,
			create_inherent_data_providers: Arc::new(create_inherent_data_providers),
			_backend: PhantomData,
		}
	}

	async fn check_inherents(
		&self,
		block: B,
		at_hash: B::Hash,
		inherent_data_providers: CIDP::InherentDataProviders,
	) -> Result<(), Error<B>> {
		if *block.header().number() < self.check_inherents_after {
			return Ok(());
		}

		let inherent_data = inherent_data_providers
			.create_inherent_data()
			.await
			.map_err(|e| Error::CreateInherents(e))?;

		let inherent_res = self
			.client
			.runtime_api()
			.check_inherents(at_hash, block.into(), inherent_data)
			.map_err(|e| Error::Client(e.into()))?;

		if !inherent_res.ok() {
			for (identifier, error) in inherent_res.into_errors() {
				match inherent_data_providers.try_handle_error(&identifier, &error).await {
					Some(res) => res.map_err(Error::CheckInherents)?,
					None => return Err(Error::CheckInherentsUnknownError(identifier)),
				}
			}
		}

		Ok(())
	}
}

#[async_trait::async_trait]
impl<B, I, C, CIDP, BE, const LOGGING_FREQUENCY: u64> BlockImport<B>
	for PowBlockImport<B, I, C, CIDP, BE, LOGGING_FREQUENCY>
where
	B: BlockT<Hash = H256>,
	I: BlockImport<B> + Send + Sync,
	I::Error: Into<ConsensusError>,
	C: ProvideRuntimeApi<B>
		+ BlockBackend<B>
		+ Send
		+ Sync
		+ HeaderBackend<B>
		+ AuxStore
		+ BlockOf
		+ sc_client_api::Finalizer<B, BE>
		+ 'static,
	C::Api: BlockBuilderApi<B> + QPoWApi<B>,
	CIDP: CreateInherentDataProviders<B, ()> + Send + Sync,
	BE: sc_client_api::Backend<B> + 'static,
{
	type Error = ConsensusError;

	async fn check_block(&self, block: BlockCheckParams<B>) -> Result<ImportResult, Self::Error> {
		self.inner.check_block(block).await.map_err(Into::into)
	}

	async fn import_block(
		&self,
		mut block_import_params: BlockImportParams<B>,
	) -> Result<ImportResult, Self::Error> {
		let parent_hash = *block_import_params.header.parent_hash();

		if let Some(inner_body) = block_import_params.body.take() {
			let check_block = B::new(block_import_params.header.clone(), inner_body);

			if !block_import_params.state_action.skip_execution_checks() {
				self.check_inherents(
					check_block.clone(),
					parent_hash,
					self.create_inherent_data_providers
						.create_inherent_data_providers(parent_hash, ())
						.await?,
				)
				.await?;
			}

			block_import_params.body = Some(check_block.deconstruct().1);
		}

		let inner_seal = fetch_seal::<B>(
			block_import_params.post_digests.last(),
			block_import_params.header.hash(),
		)?;

		let pre_hash = block_import_params.header.hash();

		// Convert seal to nonce
		let nonce: [u8; 64] = inner_seal
			.as_slice()
			.try_into()
			.map_err(|_| Error::<B>::Runtime("Seal does not have exactly 64 bytes".to_string()))?;
		let pre_hash_arr: [u8; 32] = pre_hash.0;

		// Verify nonce and get achieved difficulty in a single call
		// This avoids computing the nonce hash twice
		let (verified, achieved_difficulty) = self
			.client
			.runtime_api()
			.verify_and_get_achieved_difficulty(parent_hash, pre_hash_arr, nonce)
			.map_err(|e| {
				Error::<B>::Runtime(format!(
					"API error in verify_and_get_achieved_difficulty: {:?}",
					e
				))
			})?;

		if !verified {
			log::error!("Invalid Seal {:?} for parent hash {:?}", inner_seal, parent_hash);
			return Err(Error::<B>::InvalidSeal.into());
		}

		// Get parent's cumulative achieved work from aux storage
		let parent_work = get_chain_work::<B, C>(&*self.client, parent_hash).unwrap_or_else(|e| {
			log::warn!(target: LOG_TARGET, "Failed to get parent achieved work for {parent_hash:?}: {e:?}");
			U512::zero()
		});

		// Calculate new cumulative achieved work
		let new_work = parent_work.saturating_add(achieved_difficulty);

		let info = self.client.info();
		let current_best_work = get_chain_work::<B, C>(&*self.client, info.best_hash)
			.unwrap_or_else(|e| {
				log::warn!(target: LOG_TARGET, "Failed to get best chain achieved work for {:?}: {e:?}", info.best_hash);
				U512::zero()
			});

		let is_best = is_heavier(
			new_work,
			*block_import_params.header.number(),
			current_best_work,
			info.best_number,
		);
		block_import_params.fork_choice = Some(ForkChoiceStrategy::Custom(is_best));

		// Get block hash (with seal) for achieved work storage.
		// Must use the post-seal hash because that's how blocks are referenced:
		// - parent_hash in child blocks references the post-seal hash
		// - client.info().best_hash is the post-seal hash
		let block_hash = block_import_params.post_header().hash();

		// Log block import progress every LOGGING_FREQUENCY blocks
		let block_number = block_import_params.header.number();
		let block_number_u64: u64 = (*block_number).try_into().unwrap_or(0);
		if block_number_u64 % LOGGING_FREQUENCY == 0 {
			log::info!(
				"⛏️ Imported blocks #{}-{}: {:?} - extrinsics_root={:?}, state_root={:?}",
				block_number_u64.saturating_sub(LOGGING_FREQUENCY),
				block_number,
				block_import_params.header.hash(),
				block_import_params.header.extrinsics_root(),
				block_import_params.header.state_root()
			);
		} else {
			log::debug!(
				target: "qpow",
				"⛏️ Importing block #{}: {:?} - extrinsics_root={:?}, state_root={:?}",
				block_number,
				block_import_params.header.hash(),
				block_import_params.header.extrinsics_root(),
				block_import_params.header.state_root()
			);
		}

		// Store cumulative achieved work BEFORE inner import, because inner import
		// triggers notifications that call best_chain which needs this data.
		store_cumulative_achieved_work::<B, C>(&*self.client, block_hash, new_work).map_err(
			|e| {
				ConsensusError::ClientImport(format!(
					"Failed to store cumulative achieved work for {:?}: {:?}",
					block_hash, e
				))
			},
		)?;

		// Import the block. If import fails, clean up the achieved work entry we just stored
		// to prevent stale aux data accumulation from repeated invalid submissions.
		let result = match self.inner.import_block(block_import_params).await {
			Ok(result) => result,
			Err(e) => {
				// Rollback: remove the achieved work entry for the failed import
				if let Err(cleanup_err) =
					delete_cumulative_achieved_work::<B, C>(&*self.client, block_hash)
				{
					log::warn!(
						target: LOG_TARGET,
						"Failed to clean up achieved work after failed import for {:?}: {:?}",
						block_hash,
						cleanup_err
					);
				}
				return Err(e.into());
			},
		};

		// Finalization prunes competing forks that are beyond max_reorg_depth.
		if let Err(e) = finalize_canonical_at_depth::<B, C, BE>(&*self.client) {
			log::warn!(
				target: LOG_TARGET,
				"Failed to finalize after block import: {:?}",
				e
			);
		}

		let info = self.client.info();
		log::debug!(target: LOG_TARGET, "📦 Canonical tip: #{} ({:?})", info.best_number, info.best_hash);

		Ok(result)
	}
}

/// Extract the PoW seal from header into post_digests for later verification.
async fn extract_pow_seal<B>(
	mut block: BlockImportParams<B>,
) -> Result<BlockImportParams<B>, String>
where
	B: BlockT<Hash = H256>,
{
	let hash = block.header.hash();
	let header = &mut block.header;
	let block_hash = hash;
	let seal_item = match header.digest_mut().pop() {
		Some(DigestItem::Seal(id, seal)) =>
			if id == POW_ENGINE_ID {
				DigestItem::Seal(id, seal)
			} else {
				return Err(Error::<B>::WrongEngine(id).into());
			},
		_ => return Err(Error::<B>::HeaderUnsealed(block_hash).into()),
	};

	block.post_digests.push(seal_item);
	Ok(block)
}

/// The PoW import queue type.
pub type PowImportQueue<B> = BasicQueue<B>;

/// Minimal verifier that extracts the PoW seal from header to post_digests.
struct SimplePowVerifier;

#[async_trait::async_trait]
impl<B> Verifier<B> for SimplePowVerifier
where
	B: BlockT<Hash = H256>,
{
	async fn verify(&self, block: BlockImportParams<B>) -> Result<BlockImportParams<B>, String> {
		extract_pow_seal::<B>(block).await
	}
}

/// Import queue for QPoW engine.
pub fn import_queue<B, C>(
	block_import: BoxBlockImport<B>,
	justification_import: Option<BoxJustificationImport<B>>,
	spawner: &impl sp_core::traits::SpawnEssentialNamed,
	registry: Option<&Registry>,
) -> Result<PowImportQueue<B>, sp_consensus::Error>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
	C::Api: QPoWApi<B>,
{
	let verifier = SimplePowVerifier;
	Ok(BasicQueue::new(verifier, block_import, justification_import, spawner, registry))
}

/// Minimum seconds between transaction-triggered rebuilds.
/// Set high enough to prevent the "rebuild loop" under high tx load where block construction
/// time dominates and effective mining time approaches zero, causing block times to spike.
const MIN_SECS_BETWEEN_TX_REBUILDS: u64 = 2;

/// Start the mining worker for QPoW. This function provides the necessary helper functions that can
/// be used to implement a miner. However, it does not do the CPU-intensive mining itself.
///
/// Two values are returned -- a worker, which contains functions that allows querying the current
/// mining metadata and submitting mined blocks, and a future, which must be polled to fill in
/// information in the worker.
///
/// The worker will rebuild blocks when:
/// - A new block is imported from the network
/// - New transactions arrive (rate limited to MAX_REBUILDS_PER_SEC)
///
/// This allows transactions to be included faster since we don't wait for the next block import
/// to rebuild. Mining on a new block vs the old block has the same probability of success per
/// nonce, so the only cost is the overhead of rebuilding (which is minimal compared to mining
/// time).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn start_mining_worker<Block, C, E, SO, L, CIDP, TxHash, TxStream>(
	block_import: BoxBlockImport<Block>,
	client: Arc<C>,
	mut env: E,
	sync_oracle: SO,
	justification_sync_link: L,
	rewards_preimage: [u8; 32],
	create_inherent_data_providers: CIDP,
	tx_notifications: TxStream,
	build_time: Duration,
) -> (MiningHandle<Block, C, L, <E::Proposer as Proposer<Block>>::Proof>, impl Future<Output = ()>)
where
	Block: BlockT<Hash = H256>,
	C: BlockchainEvents<Block>
		+ ProvideRuntimeApi<Block>
		+ BlockBackend<Block>
		+ HeaderBackend<Block>
		+ Send
		+ Sync
		+ 'static,
	C::Api: QPoWApi<Block>,
	E: Environment<Block> + Send + Sync + 'static,
	E::Error: std::fmt::Debug,
	E::Proposer: Proposer<Block>,
	SO: SyncOracle + Clone + Send + Sync + 'static,
	L: JustificationSyncLink<Block>,
	CIDP: CreateInherentDataProviders<Block, ()>,
	TxHash: Send + 'static,
	TxStream: Stream<Item = TxHash> + Send + Unpin + 'static,
{
	let mut trigger_stream = UntilImportedOrTransaction::new(
		client.import_notification_stream(),
		tx_notifications,
		Duration::from_secs(MIN_SECS_BETWEEN_TX_REBUILDS),
	);
	let worker = MiningHandle::new(client.clone(), block_import, justification_sync_link);
	let worker_ret = worker.clone();

	// Latest build request - overwrites previous if builder is slow.
	// Uses a Mutex<Option> for the value + a channel for wake notification.
	let pending_build: Arc<parking_lot::Mutex<Option<Block::Hash>>> =
		Arc::new(parking_lot::Mutex::new(None));
	let (notify_tx, mut notify_rx) = futures::channel::mpsc::channel::<()>(1);

	// Task 1: Convert triggers into build requests
	let trigger_task = {
		let client = client.clone();
		let worker = worker.clone();
		let pending_build = pending_build.clone();
		let mut notify_tx = notify_tx;
		async move {
			while let Some(trigger) = trigger_stream.next().await {
				if sync_oracle.is_major_syncing() {
					debug!(target: LOG_TARGET, "Skipping proposal due to sync.");
					worker.on_major_syncing();
					continue;
				}

				let best_hash = client.info().best_hash;

				// Optimization, skip if we already imported this block
				if trigger == RebuildTrigger::BlockImported && worker.best_hash() == Some(best_hash)
				{
					continue;
				}

				// Set the latest build request (overwrites any previous)
				*pending_build.lock() = Some(best_hash);
				let _ = notify_tx.try_send(());
			}
		}
	};

	// Task 2: Process build requests and update worker
	let build_task = async move {
		while notify_rx.next().await.is_some() {
			// Take the latest request (may have been overwritten multiple times)
			let Some(target_hash) = pending_build.lock().take() else {
				continue;
			};

			// Build the block
			if let Some(build) = create_proposal(
				&client,
				&mut env,
				&create_inherent_data_providers,
				target_hash,
				rewards_preimage,
				build_time,
			)
			.await
			{
				worker.on_build(build);
			}
		}
	};

	let task = async move {
		futures::join!(trigger_task, build_task);
	};

	(worker_ret, task)
}

/// Create a block proposal. Returns None if any step fails (errors are logged).
async fn create_proposal<Block, C, E, CIDP>(
	client: &Arc<C>,
	env: &mut E,
	create_inherent_data_providers: &CIDP,
	best_hash: Block::Hash,
	rewards_preimage: [u8; 32],
	build_time: Duration,
) -> Option<MiningBuild<Block, <E::Proposer as Proposer<Block>>::Proof>>
where
	Block: BlockT<Hash = H256>,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + Send + Sync + 'static,
	C::Api: QPoWApi<Block>,
	E: Environment<Block>,
	E::Error: std::fmt::Debug,
	E::Proposer: Proposer<Block>,
	CIDP: CreateInherentDataProviders<Block, ()>,
{
	let best_header = match client.header(best_hash) {
		Ok(Some(h)) => h,
		Ok(None) => {
			warn!(target: LOG_TARGET, "Best header not found for hash: {:?}", best_hash);
			return None;
		},
		Err(e) => {
			warn!(target: LOG_TARGET, "Header lookup error: {}", e);
			return None;
		},
	};

	let difficulty = match qpow_get_difficulty::<Block, C>(client, best_hash) {
		Ok(d) => d,
		Err(e) => {
			warn!(target: LOG_TARGET, "Fetch difficulty failed: {}", e);
			return None;
		},
	};

	let inherent_data_providers = match create_inherent_data_providers
		.create_inherent_data_providers(best_hash, ())
		.await
	{
		Ok(p) => p,
		Err(e) => {
			warn!(target: LOG_TARGET, "Creating inherent data providers failed: {}", e);
			return None;
		},
	};

	let inherent_data = match inherent_data_providers.create_inherent_data().await {
		Ok(d) => d,
		Err(e) => {
			warn!(target: LOG_TARGET, "Creating inherent data failed: {}", e);
			return None;
		},
	};

	let proposer = match env.init(&best_header).await {
		Ok(p) => p,
		Err(e) => {
			warn!(target: LOG_TARGET, "Creating proposer failed: {:?}", e);
			return None;
		},
	};

	let mut inherent_digest = Digest::default();
	inherent_digest.push(DigestItem::PreRuntime(POW_ENGINE_ID, rewards_preimage.to_vec()));

	let proposal = match proposer.propose(inherent_data, inherent_digest, build_time, None).await {
		Ok(p) => p,
		Err(e) => {
			warn!(target: LOG_TARGET, "Creating proposal failed: {}", e);
			return None;
		},
	};

	// Check if best_hash changed during building
	if client.info().best_hash != best_hash {
		debug!(target: LOG_TARGET, "Best hash changed during block building, discarding");
		return None;
	}

	Some(MiningBuild {
		metadata: MiningMetadata {
			best_hash,
			pre_hash: proposal.block.header().hash(),
			rewards_preimage,
			difficulty,
		},
		proposal,
	})
}

/// Fetch the QPoW seal from the given digest, if present and valid.
fn fetch_seal<B: BlockT>(digest: Option<&DigestItem>, hash: B::Hash) -> Result<RawSeal, Error<B>> {
	match digest {
		Some(DigestItem::Seal(id, seal)) if *id == POW_ENGINE_ID => Ok(seal.clone()),
		Some(DigestItem::Seal(id, _)) => Err(Error::<B>::WrongEngine(*id)),
		_ => Err(Error::<B>::HeaderUnsealed(hash)),
	}
}

// Helper function to get difficulty via runtime API
pub fn qpow_get_difficulty<B, C>(client: &C, parent: B::Hash) -> Result<U512, Error<B>>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B>,
	C::Api: QPoWApi<B>,
{
	client
		.runtime_api()
		.get_difficulty(parent)
		.map_err(|_| Error::Runtime("Failed to fetch difficulty".into()))
}
