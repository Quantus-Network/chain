mod miner;

use std::marker::PhantomData;
use std::sync::Arc;
use codec::{Decode, Encode};
use primitive_types::{H256, U256};
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_consensus_pow::{Seal as RawSeal};
use sp_api::__private::BlockT;
use sp_api::ProvideRuntimeApi;
use sp_runtime::generic::BlockId;
use sp_consensus_qpow::QPoWApi;
use sc_client_api::BlockBackend;
use sp_consensus::{SelectChain};
use sp_runtime::traits::{ Header, Zero};
use sp_blockchain::{Backend, HeaderBackend};

pub use miner::QPoWMiner;



#[derive(Clone, Debug, Encode, Decode, PartialEq)]
pub struct QPoWSeal {
    pub nonce: [u8; 64],
}

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
        // Convert seal to nonce [u8; 64]
        let nonce: [u8; 64] = match seal.as_slice().try_into() {
            Ok(arr) => arr,
            Err(_) => panic!("Vec<u8> does not have exactly 64 elements"),
        };
        let parent_hash = match extract_block_hash(parent) {
            Ok(hash) => hash,
            Err(_) => return Ok(false),
        };

        let pre_hash = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

        // Verify the nonce using QPoW
        if !self.client.runtime_api()
            .verify_nonce(parent_hash, pre_hash, nonce, difficulty.low_u64())
            .map_err(|e| Error::Runtime(format!("API error in verify_nonce: {:?}", e)))? {
            return Ok(false);
        }

        Ok(true)
    }
}


pub fn extract_block_hash<B: BlockT<Hash = H256>>(parent: &BlockId<B>) -> Result<H256, Error<B>> {
    match parent {
        BlockId::Hash(hash) => Ok(*hash),
        BlockId::Number(_) => Err(Error::Runtime("Expected BlockId::Hash, but got BlockId::Number".into())),
    }
}

//#[derive(Clone)]
pub struct HeaviestChain<B, C, BE>
where 
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B>,
    BE: sc_client_api::Backend<B>,
{
    backend: Arc<BE>,
    client: Arc<C>,
    algorithm: QPowAlgorithm<B, C>,
    _phantom: PhantomData<B>,
}

impl<B, C, BE> Clone for HeaviestChain<B, C, BE>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B>,
    BE: sc_client_api::Backend<B>,
{
    fn clone(&self) -> Self {
        Self {
            backend: Arc::clone(&self.backend),
            client: Arc::clone(&self.client),
            algorithm: self.algorithm.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<B, C, BE> HeaviestChain<B, C, BE>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
    BE: sc_client_api::Backend<B> + 'static,
{
    pub fn new(backend: Arc<BE>, client: Arc<C>, algorithm: QPowAlgorithm<B,C>) -> Self {
        Self {
               backend,
               client,
               algorithm,
               _phantom: PhantomData }
    }

    fn calculate_chain_difficulty(&self, chain_head: &B::Header) -> Result<U256, sp_consensus::Error> {
        // calculate cumulative difficulty of a chain
        
        let mut current_hash = chain_head.hash();
        let mut total_difficulty = U256::zero();

        log::info!(
            "Calculating difficulty for chain with head: {:?} (#{:?})", 
            current_hash, 
            chain_head.number()
        );
        
        if chain_head.number().is_zero() {
            // Genesis block should have some minimal difficulty
            let genesis_difficulty = self.client.runtime_api().get_difficulty(current_hash.clone())
                        .map_err(|e| sp_consensus::Error::Other(format!("Failed to get genesis difficulty {:?}", e).into()))?;

            return Ok(U256::from(genesis_difficulty));
            //return Ok(U256::from(1));
        }

        // Traverse the chain backwards to calculate cumulative difficulty
        loop {
            let header = self.client.header(current_hash)
                                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                                .ok_or_else(|| sp_consensus::Error::Other(format!("Missing Header {:?}", current_hash).into()))?;

            // Stop at genesis block
            if header.number().is_zero() {
                // Genesis block should have some minimal difficulty
                let genesis_difficulty = self.client.runtime_api().get_difficulty(current_hash.clone())
                .map_err(|e| sp_consensus::Error::Other(format!("Failed to get genesis difficulty {:?}", e).into()))?;

                total_difficulty = total_difficulty.saturating_add(U256::from(genesis_difficulty));
                break;
            }
            
            let seal_log = header.digest().logs().iter().find(|item| 
                item.as_seal().is_some())
                .ok_or_else(|| sp_consensus::Error::Other("No seal found in block digest".into()))?;

            let (_, seal_data) = seal_log.as_seal().ok_or_else(|| sp_consensus::Error::Other("Invalid seal format".into()))?;

            // Convert header hash to [u8; 32]
            let header_bytes = header.hash().as_ref().try_into().unwrap_or([0u8; 32]);
            
            // Try to decode nonce from seal data
            let nonce = if seal_data.len() == 64 {
                let mut nonce_bytes = [0u8; 64];
                nonce_bytes.copy_from_slice(&seal_data[0..64]);
                nonce_bytes
            } else {
                //seal data doesn't match expected format
                return Err(sp_consensus::Error::Other(format!("Invalid seal data length: {}", seal_data.len()).into()));
            };

            let max_distance = self.client.runtime_api().get_max_distance(current_hash.clone())
                .map_err(|e| sp_consensus::Error::Other(format!("Failed to get max distance: {:?}", e).into()))?;

            let actual_distance = self.client.runtime_api().get_nonce_distance(current_hash.clone(), header_bytes, nonce)
                .map_err(|e| sp_consensus::Error::Other(format!("Failed to get nonce distance: {:?}", e).into()))?;

            let block_difficulty = U256::from(max_distance.saturating_sub(actual_distance));

            log::info!(
                "Block #{:?} difficulty: {:?}", 
                header.number(), 
                block_difficulty
            );
            
            total_difficulty = total_difficulty.saturating_add(U256::from(block_difficulty));
            
            // Move to the parent block
            current_hash = *header.parent_hash();
        }

        log::info!(
            "Total chain difficulty: {:?} for chain with head at #{:?}", 
            total_difficulty, 
            chain_head.number()
        );
        
        Ok(total_difficulty)
    }
}

#[async_trait::async_trait]
impl<B, C, BE> SelectChain<B> for HeaviestChain<B, C, BE>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
    BE: sc_client_api::Backend<B> + 'static,
{
    async fn leaves(&self) -> Result<Vec<B::Hash>, sp_consensus::Error>{
        self.backend.blockchain().leaves().map_err(|e| {
            sp_consensus::Error::Other(format!("Failed to fetch leaves: {:?}", e).into())
        })
    }

    async fn best_chain(&self) -> Result<B::Header, sp_consensus::Error> {
        let leaves = self.backend.blockchain().leaves().map_err(|e| sp_consensus::Error::Other(format!("Failed to fetch leaves: {:?}", e).into()))?;
        if leaves.is_empty() {
            return Err(sp_consensus::Error::Other("Blockchain has no leaves".into()));
        }

        let mut best_header = None;
        let mut best_work = U256::zero();
        
        for leaf_hash in leaves {
            let header = self.client.header(leaf_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other(format!("Missing header for {:?}", leaf_hash).into()))?;
            let chain_work = self.calculate_chain_difficulty(&header)?;
            if chain_work > best_work {
                best_work = chain_work;
                best_header = Some(header);
            }
        }

        best_header.ok_or(sp_consensus::Error::Other("No Valid Chain Found".into()))
        
    }
}