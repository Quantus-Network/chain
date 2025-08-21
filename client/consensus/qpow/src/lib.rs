mod chain_management;
mod miner;

pub use chain_management::{ChainManagement, HeaviestChain};
pub use miner::QPoWMiner;
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_api::{ProvideRuntimeApi, __private::BlockT};
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::generic::BlockId;
use std::{marker::PhantomData, sync::Arc};

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
