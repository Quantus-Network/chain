mod chain_management;
mod miner;
mod worker;

pub use chain_management::{ChainManagement, HeaviestChain};
pub use miner::QPoWMiner;
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sp_api::{ProvideRuntimeApi, __private::BlockT};
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::generic::BlockId;
use std::{marker::PhantomData, sync::Arc, time::Duration};


pub use crate::worker::{MiningBuild, MiningHandle, MiningMetadata};

use crate::worker::UntilImportedOrTimeout;
use codec::{Decode, Encode};
use futures::{Future, StreamExt};
use log::*;
use prometheus_endpoint::Registry;
use sc_client_api::{self, backend::AuxStore, BlockOf, BlockchainEvents};
use sc_consensus::{
	BasicQueue, BlockCheckParams, BlockImport, BlockImportParams, BoxBlockImport,
	BoxJustificationImport, ForkChoiceStrategy, ImportResult, Verifier, JustificationSyncLink
};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::{Environment, Error as ConsensusError, Proposer, SelectChain, SyncOracle};
use sp_consensus_pow::{Seal, POW_ENGINE_ID};
use sp_inherents::{CreateInherentDataProviders, InherentDataProvider};
use sp_runtime::{
	generic::{Digest, DigestItem},
	traits::{Header as HeaderT},
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

/// Local minimal PowAlgorithm trait to avoid dependency on sc-consensus-pow.
pub trait PowAlgorithm<B: BlockT> {
	/// Difficulty for the algorithm.
	type Difficulty: Default + Encode + Decode + Ord + Clone + Copy;

	/// Get the next block's difficulty.
	fn difficulty(&self, parent: B::Hash) -> Result<Self::Difficulty, Error<B>>;
	/// Verify that the seal is valid against given pre hash when parent block is not yet imported.
	/// None means that preliminary verify is not available for this algorithm.
	fn preliminary_verify(
		&self,
		_pre_hash: &B::Hash,
		_seal: &Seal,
	) -> Result<Option<bool>, Error<B>> {
		Ok(None)
	}
	/// Break a fork choice tie.
	/// Returns if the new seal should be considered best block.
	fn break_tie(&self, _own_seal: &Seal, _new_seal: &Seal) -> bool { false }
	/// Verify that the difficulty is valid against given seal.
	fn verify(
		&self,
		parent: &BlockId<B>,
		pre_hash: &B::Hash,
		pre_digest: Option<&[u8]>,
		seal: &Seal,
		difficulty: Self::Difficulty,
	) -> Result<(bool, U512), Error<B>>;
}

/// A block importer for PoW.
pub struct PowBlockImport<B: BlockT, I, C, S, Algorithm, CIDP> {
	algorithm: Algorithm,
	inner: I,
	select_chain: S,
	client: Arc<C>,
	create_inherent_data_providers: Arc<CIDP>,
	check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
}

impl<B: BlockT, I: Clone, C, S: Clone, Algorithm: Clone, CIDP> Clone
for PowBlockImport<B, I, C, S, Algorithm, CIDP>
{
	fn clone(&self) -> Self {
		Self {
			algorithm: self.algorithm.clone(),
			inner: self.inner.clone(),
			select_chain: self.select_chain.clone(),
			client: self.client.clone(),
			create_inherent_data_providers: self.create_inherent_data_providers.clone(),
			check_inherents_after: self.check_inherents_after,
		}
	}
}

impl<B, I, C, S, Algorithm, CIDP> PowBlockImport<B, I, C, S, Algorithm, CIDP>
where
	B: BlockT,
	I: BlockImport<B> + Send + Sync,
	I::Error: Into<ConsensusError>,
	C: ProvideRuntimeApi<B> + Send + Sync + HeaderBackend<B> + AuxStore + BlockOf,
	C::Api: BlockBuilderApi<B>,
	Algorithm: PowAlgorithm<B>,
	CIDP: CreateInherentDataProviders<B, ()>,
{
	/// Create a new block import suitable to be used in PoW
	pub fn new(
		inner: I,
		client: Arc<C>,
		algorithm: Algorithm,
		check_inherents_after: <<B as BlockT>::Header as HeaderT>::Number,
		select_chain: S,
		create_inherent_data_providers: CIDP,
	) -> Self {
		Self {
			inner,
			client,
			algorithm,
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
impl<B, I, C, S, Algorithm, CIDP> BlockImport<B> for PowBlockImport<B, I, C, S, Algorithm, CIDP>
where
	B: BlockT,
	I: BlockImport<B> + Send + Sync,
	I::Error: Into<ConsensusError>,
	S: SelectChain<B>,
	C: ProvideRuntimeApi<B> + Send + Sync + HeaderBackend<B> + AuxStore + BlockOf,
	C::Api: BlockBuilderApi<B>,
	Algorithm: PowAlgorithm<B> + Send + Sync,
	Algorithm::Difficulty: 'static + Send,
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
		let best_header = self
			.select_chain
			.best_chain()
			.await
			.map_err(|e| format!("Fetch best chain failed via select chain: {}", e))
			.map_err(ConsensusError::ChainLookup)?;
		let best_hash = best_header.hash();

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

		// Fetch difficulty directly; no intermediate passing needed
		let difficulty = self.algorithm.difficulty(parent_hash)?;

		let pre_hash = block.header.hash();
		let pre_digest = find_pre_digest::<B>(&block.header)?;
		let (verified, difficulty) = self.algorithm.verify(
			&BlockId::hash(parent_hash),
			&pre_hash,
			pre_digest.as_ref().map(|v| &v[..]),
			&inner_seal,
			difficulty,
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

/// A verifier for PoW blocks.
pub struct PowVerifier<B: BlockT, Algorithm> {
	algorithm: Algorithm,
	_marker: PhantomData<B>,
}

impl<B: BlockT, Algorithm> PowVerifier<B, Algorithm> {
	pub fn new(algorithm: Algorithm) -> Self {
		Self { algorithm, _marker: PhantomData }
	}

	fn check_header(&self, mut header: B::Header) -> Result<(B::Header, DigestItem), Error<B>>
	where
		Algorithm: PowAlgorithm<B>,
	{
		let hash = header.hash();

		let (seal, inner_seal) = match header.digest_mut().pop() {
			Some(DigestItem::Seal(id, seal)) =>
				if id == POW_ENGINE_ID {
					(DigestItem::Seal(id, seal.clone()), seal)
				} else {
					return Err(Error::WrongEngine(id));
				},
			_ => return Err(Error::HeaderUnsealed(hash)),
		};

		let pre_hash = header.hash();

		if !self.algorithm.preliminary_verify(&pre_hash, &inner_seal)?.unwrap_or(true) {
			return Err(Error::FailedPreliminaryVerify);
		}

		Ok((header, seal))
	}
}

#[async_trait::async_trait]
impl<B: BlockT, Algorithm> Verifier<B> for PowVerifier<B, Algorithm>
where
	Algorithm: PowAlgorithm<B> + Send + Sync,
	Algorithm::Difficulty: 'static + Send,
{
	async fn verify(
		&self,
		mut block: BlockImportParams<B>,
	) -> Result<BlockImportParams<B>, String> {
		let hash = block.header.hash();
		let (checked_header, seal) = self.check_header(block.header)?;

		block.header = checked_header;
		block.post_digests.push(seal);
		block.post_hash = Some(hash);

		Ok(block)
	}
}

/// The PoW import queue type.
pub type PowImportQueue<B> = BasicQueue<B>;

/// Import queue for PoW engine.
pub fn import_queue<B, Algorithm>(
	block_import: BoxBlockImport<B>,
	justification_import: Option<BoxJustificationImport<B>>,
	algorithm: Algorithm,
	spawner: &impl sp_core::traits::SpawnEssentialNamed,
	registry: Option<&Registry>,
) -> Result<PowImportQueue<B>, sp_consensus::Error>
where
	B: BlockT,
	Algorithm: PowAlgorithm<B> + Clone + Send + Sync + 'static,
	Algorithm::Difficulty: Send,
{
	let verifier = PowVerifier::new(algorithm);

	Ok(BasicQueue::new(verifier, block_import, justification_import, spawner, registry))
}

/// Start the mining worker for PoW. This function provides the necessary helper functions that can
/// be used to implement a miner. However, it does not do the CPU-intensive mining itself.
///
/// Two values are returned -- a worker, which contains functions that allows querying the current
/// mining metadata and submitting mined blocks, and a future, which must be polled to fill in
/// information in the worker.
///
/// `pre_runtime` is a parameter that allows a custom additional pre-runtime digest to be inserted
/// for blocks being built. This can encode authorship information, or just be a graffiti.
pub fn start_mining_worker<Block, C, S, Algorithm, E, SO, L, CIDP>(
	block_import: BoxBlockImport<Block>,
	client: Arc<C>,
	select_chain: S,
	algorithm: Algorithm,
	mut env: E,
	sync_oracle: SO,
	justification_sync_link: L,
	pre_runtime: Option<Vec<u8>>,
	create_inherent_data_providers: CIDP,
	timeout: Duration,
	build_time: Duration,
) -> (
	MiningHandle<Block, Algorithm, L, <E::Proposer as Proposer<Block>>::Proof>,
	impl Future<Output = ()>,
)
where
	Block: BlockT,
	C: BlockchainEvents<Block> + 'static,
	S: SelectChain<Block> + 'static,
	Algorithm: PowAlgorithm<Block> + Clone,
	Algorithm::Difficulty: Send + 'static,
	E: Environment<Block> + Send + Sync + 'static,
	E::Error: std::fmt::Debug,
	E::Proposer: Proposer<Block>,
	SO: SyncOracle + Clone + Send + Sync + 'static,
	L: JustificationSyncLink<Block>,
	CIDP: CreateInherentDataProviders<Block, ()>,
{
	let mut timer = UntilImportedOrTimeout::new(client.import_notification_stream(), timeout);
	let worker = MiningHandle::new(algorithm.clone(), block_import, justification_sync_link);
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

			let difficulty = match algorithm.difficulty(best_hash) {
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

			let build = MiningBuild::<Block, Algorithm, _> {
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

/// Find PoW pre-runtime.
fn find_pre_digest<B: BlockT>(header: &B::Header) -> Result<Option<Vec<u8>>, Error<B>> {
	let mut pre_digest: Option<_> = None;
	for log in header.digest().logs() {
		trace!(target: LOG_TARGET, "Checking log {:?}, looking for pre runtime digest", log);
		match (log, pre_digest.is_some()) {
			(DigestItem::PreRuntime(POW_ENGINE_ID, _), true) =>
				return Err(Error::MultiplePreRuntimeDigests),
			(DigestItem::PreRuntime(POW_ENGINE_ID, v), false) => {
				pre_digest = Some(v.clone());
			},
			(_, _) => trace!(target: LOG_TARGET, "Ignoring digest not meant for us"),
		}
	}

	Ok(pre_digest)
}

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



pub struct QPowAlgorithm<B, C>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B>,
{
	pub client: Arc<C>,
	pub _phantom: PhantomData<B>,
}

impl<B, C> Clone for QPowAlgorithm<B, C>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B>,
{
	fn clone(&self) -> Self {
		Self { client: Arc::clone(&self.client), _phantom: PhantomData }
	}
}

// Here we implement the general PowAlgorithm trait for our concrete Sha3Algorithm
impl<B, C> PowAlgorithm<B> for QPowAlgorithm<B, C>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
	C::Api: QPoWApi<B>,
{
	type Difficulty = U512;

	fn difficulty(&self, parent: B::Hash) -> Result<Self::Difficulty, Error<B>> {
		self.client
			.runtime_api()
			.get_difficulty(parent)
			.map(U512::from)
			.map_err(|_| Error::Runtime("Failed to fetch difficulty".into()))
	}

	fn verify(
		&self,
		parent: &BlockId<B>,
		pre_hash: &H256,
		_pre_digest: Option<&[u8]>,
		seal: &RawSeal,
		_difficulty: Self::Difficulty,
	) -> Result<(bool, U512), Error<B>> {
		// Executed for mined and imported blocks

		// Convert seal to nonce [u8; 64]
		let nonce: [u8; 64] = match seal.as_slice().try_into() {
			Ok(arr) => arr,
			Err(_) => panic!("Vec<u8> does not have exactly 64 elements"),
		};
		let parent_hash = match extract_block_hash(parent) {
			Ok(hash) => hash,
			Err(_) => return Ok((false, U512::zero())),
		};

		let pre_hash = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);
		let verified = self
			.client
			.runtime_api()
			.verify_nonce_on_import_block(parent_hash, pre_hash, nonce)
			.map_err(|e| Error::Runtime(format!("API error in verify_nonce: {:?}", e)))?;

		// Get difficulty for error reporting (verification function no longer returns it)
		let difficulty = self
			.client
			.runtime_api()
			.get_difficulty(parent_hash)
			.map_err(|e| Error::Runtime(format!("API error getting difficulty: {:?}", e)))?;

		if !verified {
			log::warn!("Current block {:?} with parent_hash {:?} and nonce {:?} and difficulty {:?} failed to verify in runtime", pre_hash, parent_hash, nonce, difficulty);
			return Ok((false, U512::zero()));
		}

		Ok((true, difficulty))
	}
}

pub fn extract_block_hash<B: BlockT<Hash = H256>>(parent: &BlockId<B>) -> Result<H256, Error<B>> {
	match parent {
		BlockId::Hash(hash) => Ok(*hash),
		BlockId::Number(_) =>
			Err(Error::Runtime("Expected BlockId::Hash, but got BlockId::Number".into())),
	}
}
