mod miner;

use std::marker::PhantomData;
use std::sync::Arc;
use codec::{Decode, Encode};
use primitive_types::{H256, U256, U512};
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_consensus_pow::{Seal as RawSeal};
use sp_api::__private::BlockT;
use sp_api::ProvideRuntimeApi;
use sp_runtime::generic::BlockId;
use sp_consensus_qpow::QPoWApi;
use sc_client_api::BlockBackend;

pub use miner::QPoWMiner;



#[derive(Clone, Debug, Encode, Decode, PartialEq)]
pub struct QPoWSeal {
    pub difficulty: U256,
    pub work: [u8; 64], // 512 bit work
    pub nonce: u64,
}

//#[derive(Clone)]
pub struct QPowAlgorithm<B,C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B>
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
        Self {
            client: Arc::clone(&self.client),
            _phantom: PhantomData,
        }
    }
}

// Here we implement the general PowAlgorithm trait for our concrete Sha3Algorithm
impl<B, C> PowAlgorithm<B> for QPowAlgorithm<B,C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{

    type Difficulty = U256;

    fn difficulty(&self, parent: B::Hash) -> Result<Self::Difficulty, Error<B>> {
        self.client
            .runtime_api()
            .get_difficulty(parent)
            .map(U256::from)
            .map_err(|_| Error::Runtime("Failed to fetch difficulty".into()))
    }

    fn verify(
        &self,
        parent: &BlockId<B>,
        pre_hash: &H256,
        _pre_digest: Option<&[u8]>,
        seal: &RawSeal,
        difficulty: Self::Difficulty,
    ) -> Result<bool, Error<B>> {
        // Try to construct a seal object by decoding the raw seal given
        let seal = match QPoWSeal::decode(&mut &seal[..]) {
            Ok(seal) => seal,
            Err(_) => return Ok(false),
        };
        // FILL IN using qpow

        // Convert pre_hash to [u8; 32] for verification
        let header = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

        // Verify the solution using QPoW
        if !self.client.runtime_api()
            .verify_solution(extract_block_hash(parent)?, header, seal.work, difficulty.low_u64())
            .map_err(|e| Error::Runtime(format!("API error in verify_solution: {:?}", e)))? {
            return Ok(false);
        }

        // Make sure the provided work actually comes from the correct pre_hash
        let compute = Compute {
            difficulty,
            pre_hash: *pre_hash,
            nonce: seal.nonce,
            _phantom: Default::default(),
        };

        match compute.compute(extract_block_hash(parent)?, &self.client) {
            Ok(computed_seal) => {
                if computed_seal != seal {
                    return Ok(false);
                }
            }
            Err(e) => {
                return Err(e);
            }
        }

        Ok(true)
    }
}

#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug)]
pub struct Compute<B: BlockT,C>
where
    C: ProvideRuntimeApi<B>
{
    pub difficulty: U256,
    pub pre_hash: H256,
    pub nonce: u64,
    pub _phantom: PhantomData<(B, C)>,
}

impl<B,C> Compute<B,C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    pub fn compute(self,parent_hash: B::Hash, client: &Arc<C>) -> Result<QPoWSeal, Error<B>> {
        // Convert pre_hash into U512.
        let header_int = U512::from_big_endian(self.pre_hash.as_bytes());
        // Convert nonce into U512.
        let nonce_val = U512::from(self.nonce);
        // Get RSA-like parameters (m, n) deterministically from the pre_hash.
        let (m, n) = client.runtime_api().get_random_rsa(parent_hash,self.pre_hash.as_ref().try_into().unwrap())
            .map(|(m,n)| (U512::from(m), U512::from(n)))
            .map_err(|_| Error::Runtime("Failed to get random RSA".into()))?;
        // Compute group element (an array of 16 u32 values) from header and nonce.
        let work = client.runtime_api().hash_to_group_bigint(parent_hash,&header_int, &m, &n, &nonce_val)
            .map(|work| U512::from(work))
            .map_err(|_| Error::Runtime("Failed to convert hash to group_bigint".into()))?;

        Ok(QPoWSeal {
            nonce: self.nonce,
            difficulty: self.difficulty,
            work: work.to_big_endian().try_into().unwrap(),
        })
    }
}

pub fn extract_block_hash<B: BlockT<Hash = H256>>(parent: &BlockId<B>) -> Result<H256, Error<B>> {
    match parent {
        BlockId::Hash(hash) => Ok(*hash),
        BlockId::Number(_) => Err(Error::Runtime("Expected BlockId::Hash, but got BlockId::Number".into())),
    }
}