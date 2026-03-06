mod chain_management;
mod worker;

pub use chain_management::{
	finalize_canonical_at_depth, get_chain_work, get_cumulative_achieved_work,
	initialize_genesis_achieved_work, is_heavier, store_cumulative_achieved_work,
	ChainManagementError,
};
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sp_api::ProvideRuntimeApi;
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_qpow::QPoWApi;
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
use sp_consensus_pow::POW_ENGINE_ID;

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
	#[error("Fetching best header failed using select chain: {0}")]
	BestHeaderSelectChain(ConsensusError),
	#[error("Fetching best header failed: {0}")]
	BestHeader(sp_blockchain::Error),
	#[error("Best header does not exist")]
	NoBestHeader,
	#[error("Block proposing error: {0}")]
	BlockProposingError(String),
	#[error("Fetch best hash failed via select chain: {0}")]
	BestHashSelectChain(ConsensusError),
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

		// Store the block hash for achieved work storage after successful import
		// Must use post_hash (with seal) because that's how blocks are referenced:
		// - parent_hash in child blocks is the post_hash of the parent
		// - client.info().best_hash is the post_hash
		// - headers retrieved from DB have the seal, so header.hash() = post_hash
		let block_hash = block_import_params
			.post_hash
			.expect("post_hash must be set by extract_pow_seal");

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
		// triggers notifications that call best_chain which needs this data
		if let Err(e) = store_cumulative_achieved_work::<B, C>(&*self.client, block_hash, new_work)
		{
			log::warn!(
				target: LOG_TARGET,
				"Failed to store cumulative achieved work for {:?}: {:?}",
				block_hash,
				e
			);
		}

		let result = self.inner.import_block(block_import_params).await.map_err(Into::into)?;

		// Finalize blocks synchronously after import to ensure finalization happens
		// before the next block is imported. This prunes competing forks that are
		// beyond max_reorg_depth.
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
	block.post_hash = Some(hash);
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

/// Maximum transaction-triggered rebuilds per second.
/// Hardcoded for now but could be made configurable later.
const MAX_REBUILDS_PER_SEC: u32 = 2;

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
		MAX_REBUILDS_PER_SEC,
	);
	let worker = MiningHandle::new(client.clone(), block_import, justification_sync_link);
	let worker_ret = worker.clone();

	let task = async move {
		// Main block building loop - runs until trigger stream closes
		// Wait for a trigger (Initial, BlockImported, or NewTransactions)
		// continue skips to the next iteration to wait for another trigger
		while let Some(trigger) = trigger_stream.next().await {
			if sync_oracle.is_major_syncing() {
				debug!(target: LOG_TARGET, "Skipping proposal due to sync.");
				worker.on_major_syncing();
				continue;
			}

			let best_hash = client.info().best_hash;
			let best_header = match client.header(best_hash) {
				Ok(Some(header)) => header,
				Ok(None) => {
					warn!(
						target: LOG_TARGET,
						"Unable to pull new block for authoring. \
						 Best header not found for hash: {:?}",
						best_hash
					);
					continue;
				},
				Err(err) => {
					warn!(
						target: LOG_TARGET,
						"Unable to pull new block for authoring. \
						 Header lookup error: {}",
						err
					);
					continue;
				},
			};

			// Skip redundant block import triggers if we're already building on this hash.
			// Initial and NewTransactions triggers should proceed to rebuild.
			if trigger == RebuildTrigger::BlockImported && worker.best_hash() == Some(best_hash) {
				continue;
			}

			// The worker is locked for the duration of the whole proposing period. Within this
			// period, the mining target is outdated and useless anyway.

			let difficulty = match qpow_get_difficulty::<Block, C>(&*client, best_hash) {
				Ok(x) => x,
				Err(err) => {
					warn!(
						target: LOG_TARGET,
						"Unable to propose new block for authoring. \
						 Fetch difficulty failed: {}",
						err,
					);
					continue;
				},
			};

			let inherent_data_providers = match create_inherent_data_providers
				.create_inherent_data_providers(best_hash, ())
				.await
			{
				Ok(x) => x,
				Err(err) => {
					warn!(
						target: LOG_TARGET,
						"Unable to propose new block for authoring. \
						 Creating inherent data providers failed: {}",
						err,
					);
					continue;
				},
			};

			let inherent_data = match inherent_data_providers.create_inherent_data().await {
				Ok(r) => r,
				Err(e) => {
					warn!(
						target: LOG_TARGET,
						"Unable to propose new block for authoring. \
						 Creating inherent data failed: {}",
						e,
					);
					continue;
				},
			};

			let mut inherent_digest = Digest::default();
			let rewards_preimage_bytes = rewards_preimage.to_vec();
			inherent_digest.push(DigestItem::PreRuntime(POW_ENGINE_ID, rewards_preimage_bytes));

			let proposer = match env.init(&best_header).await {
				Ok(x) => x,
				Err(err) => {
					warn!(
						target: LOG_TARGET,
						"Unable to propose new block for authoring. \
						 Creating proposer failed: {:?}",
						err,
					);
					continue;
				},
			};

			let proposal =
				match proposer.propose(inherent_data, inherent_digest, build_time, None).await {
					Ok(x) => x,
					Err(err) => {
						warn!(
							target: LOG_TARGET,
							"Unable to propose new block for authoring. \
							 Creating proposal failed: {}",
							err,
						);
						continue;
					},
				};

			let build = MiningBuild::<Block, _> {
				metadata: MiningMetadata {
					best_hash,
					pre_hash: proposal.block.header().hash(),
					rewards_preimage,
					difficulty,
				},
				proposal,
			};

			worker.on_build(build);
		}
	};

	(worker_ret, task)
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
