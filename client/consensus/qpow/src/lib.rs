mod chain_management;
mod worker;

pub use chain_management::{ChainManagement, HeaviestChain};
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sp_api::{ProvideRuntimeApi, __private::BlockT};
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::generic::BlockId;
use std::{sync::Arc, time::Duration};

pub use crate::worker::{MiningBuild, MiningHandle, MiningMetadata};

use crate::worker::UntilImportedOrTimeout;
use futures::{Future, StreamExt};
use log::*;
use prometheus_endpoint::Registry;
use sc_client_api::{self, backend::AuxStore, BlockOf, BlockchainEvents};
use sc_consensus::{
	BasicQueue, BlockCheckParams, BlockImport, BlockImportParams, BoxBlockImport,
	BoxJustificationImport, ForkChoiceStrategy, ImportResult, JustificationSyncLink, Verifier,
};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::{Environment, Error as ConsensusError, Proposer, SelectChain, SyncOracle};
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

// Removed PoW aux storage in QPoW implementation to reduce abstractions.

/// A block importer for PoW.
pub struct PowBlockImport<B: BlockT<Hash = H256>, I, C, S, CIDP> {
	inner: I,
	select_chain: S,
	client: Arc<C>,
	create_inherent_data_providers: Arc<CIDP>,
	check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
}

impl<B: BlockT<Hash = H256>, I: Clone, C: ProvideRuntimeApi<B>, S: Clone, CIDP> Clone
	for PowBlockImport<B, I, C, S, CIDP>
{
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
			select_chain: self.select_chain.clone(),
			client: self.client.clone(),
			create_inherent_data_providers: self.create_inherent_data_providers.clone(),
			check_inherents_after: self.check_inherents_after,
		}
	}
}

impl<B, I, C, S, CIDP> PowBlockImport<B, I, C, S, CIDP>
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
{
	/// Create a new block import suitable to be used in PoW
	pub fn new(
		inner: I,
		client: Arc<C>,
		check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
		select_chain: S,
		create_inherent_data_providers: CIDP,
	) -> Self {
		Self {
			inner,
			client,
			check_inherents_after,
			select_chain,
			create_inherent_data_providers: Arc::new(create_inherent_data_providers),
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
			.check_inherents(at_hash, block, inherent_data)
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
impl<B, I, C, S, CIDP> BlockImport<B> for PowBlockImport<B, I, C, S, CIDP>
where
	B: BlockT<Hash = H256>,
	I: BlockImport<B> + Send + Sync,
	I::Error: Into<ConsensusError>,
	S: SelectChain<B>,
	C: ProvideRuntimeApi<B>
		+ BlockBackend<B>
		+ Send
		+ Sync
		+ HeaderBackend<B>
		+ AuxStore
		+ BlockOf
		+ 'static,
	C::Api: BlockBuilderApi<B> + QPoWApi<B>,
	CIDP: CreateInherentDataProviders<B, ()> + Send + Sync,
{
	type Error = ConsensusError;

	async fn check_block(&self, block: BlockCheckParams<B>) -> Result<ImportResult, Self::Error> {
		self.inner.check_block(block).await.map_err(Into::into)
	}

	async fn import_block(
		&self,
		mut block: BlockImportParams<B>,
	) -> Result<ImportResult, Self::Error> {
		let parent_hash = *block.header.parent_hash();

		if let Some(inner_body) = block.body.take() {
			let check_block = B::new(block.header.clone(), inner_body);

			if !block.state_action.skip_execution_checks() {
				self.check_inherents(
					check_block.clone(),
					parent_hash,
					self.create_inherent_data_providers
						.create_inherent_data_providers(parent_hash, ())
						.await?,
				)
				.await?;
			}

			block.body = Some(check_block.deconstruct().1);
		}

		let inner_seal = fetch_seal::<B>(block.post_digests.last(), block.header.hash())?;

		let pre_hash = block.header.hash();
		let verified = qpow_verify::<B, C>(
			&*self.client,
			&BlockId::hash(parent_hash),
			&pre_hash,
			&inner_seal,
		)?;

		if !verified {
			log::error!("Invalid Seal {:?} for parent hash {:?}", inner_seal, parent_hash);
			return Err(Error::<B>::InvalidSeal.into());
		}

		// Use default fork choice if not provided; avoid aux total difficulty bookkeeping
		if block.fork_choice.is_none() {
			block.fork_choice = Some(ForkChoiceStrategy::LongestChain);
		}

		self.inner.import_block(block).await.map_err(Into::into)
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

/// Import queue for PoW engine.
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

/// Start the mining worker for QPoW. This function provides the necessary helper functions that can
/// be used to implement a miner. However, it does not do the CPU-intensive mining itself.
///
/// Two values are returned -- a worker, which contains functions that allows querying the current
/// mining metadata and submitting mined blocks, and a future, which must be polled to fill in
/// information in the worker.
///
/// `pre_runtime` is a parameter that allows a custom additional pre-runtime digest to be inserted
/// for blocks being built. This can encode authorship information, or just be a graffiti.
pub fn start_mining_worker<Block, C, S, E, SO, L, CIDP>(
	block_import: BoxBlockImport<Block>,
	client: Arc<C>,
	select_chain: S,
	mut env: E,
	sync_oracle: SO,
	justification_sync_link: L,
	pre_runtime: Option<Vec<u8>>,
	create_inherent_data_providers: CIDP,
	timeout: Duration,
	build_time: Duration,
) -> (MiningHandle<Block, C, L, <E::Proposer as Proposer<Block>>::Proof>, impl Future<Output = ()>)
where
	Block: BlockT<Hash = H256>,
	C: BlockchainEvents<Block>
		+ ProvideRuntimeApi<Block>
		+ BlockBackend<Block>
		+ Send
		+ Sync
		+ 'static,
	C::Api: QPoWApi<Block>,
	S: SelectChain<Block> + 'static,
	E: Environment<Block> + Send + Sync + 'static,
	E::Error: std::fmt::Debug,
	E::Proposer: Proposer<Block>,
	SO: SyncOracle + Clone + Send + Sync + 'static,
	L: JustificationSyncLink<Block>,
	CIDP: CreateInherentDataProviders<Block, ()>,
{
	let mut timer = UntilImportedOrTimeout::new(client.import_notification_stream(), timeout);
	let worker = MiningHandle::new(client.clone(), block_import, justification_sync_link);
	let worker_ret = worker.clone();

	let task = async move {
		loop {
			if timer.next().await.is_none() {
				break;
			}

			if sync_oracle.is_major_syncing() {
				debug!(target: LOG_TARGET, "Skipping proposal due to sync.");
				worker.on_major_syncing();
				continue;
			}

			let best_header = match select_chain.best_chain().await {
				Ok(x) => x,
				Err(err) => {
					warn!(
						target: LOG_TARGET,
						"Unable to pull new block for authoring. \
						 Select best chain error: {}",
						err
					);
					continue;
				},
			};
			let best_hash = best_header.hash();

			if worker.best_hash() == Some(best_hash) {
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
			if let Some(pre_runtime) = &pre_runtime {
				inherent_digest.push(DigestItem::PreRuntime(POW_ENGINE_ID, pre_runtime.to_vec()));
			}

			let pre_runtime = pre_runtime.clone();

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
					pre_runtime: pre_runtime.clone(),
					difficulty,
				},
				proposal,
			};

			worker.on_build(build);
		}
	};

	(worker_ret, task)
}

/// Find QPoW pre-runtime.
// fn find_pre_digest<B: BlockT>(header: &B::Header) -> Result<Option<Vec<u8>>, Error<B>> {
// 	let mut pre_digest: Option<_> = None;
// 	for log in header.digest().logs() {
// 		trace!(target: LOG_TARGET, "Checking log {:?}, looking for pre runtime digest", log);
// 		match (log, pre_digest.is_some()) {
// 			(DigestItem::PreRuntime(POW_ENGINE_ID, _), true) => {
// 				return Err(Error::MultiplePreRuntimeDigests)
// 			},
// 			(DigestItem::PreRuntime(POW_ENGINE_ID, v), false) => {
// 				pre_digest = Some(v.clone());
// 			},
// 			(_, _) => trace!(target: LOG_TARGET, "Ignoring digest not meant for us"),
// 		}
// 	}
//
// 	Ok(pre_digest)
// }

/// Fetch PoW seal.
fn fetch_seal<B: BlockT>(digest: Option<&DigestItem>, hash: B::Hash) -> Result<Vec<u8>, Error<B>> {
	match digest {
		Some(DigestItem::Seal(id, seal)) =>
			if id == &POW_ENGINE_ID {
				Ok(seal.clone())
			} else {
				Err(Error::<B>::WrongEngine(*id))
			},
		_ => Err(Error::<B>::HeaderUnsealed(hash)),
	}
}

pub fn extract_block_hash<B: BlockT<Hash = H256>>(parent: &BlockId<B>) -> Result<H256, Error<B>> {
	match parent {
		BlockId::Hash(hash) => Ok(*hash),
		BlockId::Number(_) =>
			Err(Error::Runtime("Expected BlockId::Hash, but got BlockId::Number".into())),
	}
}

// Helper functions to enable removal of QPowAlgorithm by using the client directly.
pub fn qpow_get_difficulty<B, C>(client: &C, parent: B::Hash) -> Result<U512, Error<B>>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B>,
	C::Api: QPoWApi<B>,
{
	client
		.runtime_api()
		.get_difficulty(parent)
		.map(U512::from)
		.map_err(|_| Error::Runtime("Failed to fetch difficulty".into()))
}

pub fn qpow_verify<B, C>(
	client: &C,
	parent: &BlockId<B>,
	pre_hash: &H256,
	seal: &RawSeal,
) -> Result<bool, Error<B>>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B>,
	C::Api: QPoWApi<B>,
{
	// Convert seal to nonce [u8; 64]
	let nonce: [u8; 64] = match seal.as_slice().try_into() {
		Ok(arr) => arr,
		Err(_) => panic!("Vec<u8> does not have exactly 64 elements"),
	};

	let parent_hash = match extract_block_hash(parent) {
		Ok(hash) => hash,
		Err(_) => return Ok(false),
	};

	let pre_hash_arr = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

	let verified = client
		.runtime_api()
		.verify_nonce_on_import_block(parent_hash, pre_hash_arr, nonce)
		.map_err(|e| Error::Runtime(format!("API error in verify_nonce: {:?}", e)))?;

	let difficulty = client
		.runtime_api()
		.get_difficulty(parent_hash)
		.map_err(|e| Error::Runtime(format!("API error getting difficulty: {:?}", e)))?;

	if !verified {
		log::warn!(
            "Current block {:?} with parent_hash {:?} and nonce {:?} and difficulty {:?} failed to verify in runtime",
            pre_hash_arr,
            parent_hash,
            nonce,
            difficulty
        );
		return Ok(false);
	}

	Ok(true)
}
