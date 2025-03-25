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
use sp_runtime::traits::{ Header, Zero, One};
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
        _difficulty: Self::Difficulty,
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
            .verify_for_import(parent_hash, pre_hash, nonce)
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
    max_reorg_depth: u32,
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
            max_reorg_depth: self.max_reorg_depth,
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
    pub fn new(backend: Arc<BE>, client: Arc<C>, algorithm: QPowAlgorithm<B,C>, max_reorg_depth: u32,) -> Self {
        Self {
               backend,
               client,
               algorithm,
               max_reorg_depth,
               _phantom: PhantomData }
    }

    pub fn calculate_block_difficulty(&self, chain_head: &B::Header) -> Result<U256, sp_consensus::Error> {
        let current_hash = chain_head.hash();

        let header = self.client.header(current_hash)
                            .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                            .ok_or_else(|| sp_consensus::Error::Other(format!("Missing Header {:?}", current_hash).into()))?;

        // Stop at genesis block
        if header.number().is_zero() {
            let genesis_difficulty = self.client.runtime_api().get_difficulty(current_hash.clone())
            .map_err(|e| sp_consensus::Error::Other(format!("Failed to get genesis difficulty {:?}", e).into()))?;

            return Ok(U256::from(genesis_difficulty));
        }
        
        let seal_log = header.digest().logs().iter().find(|item| 
            item.as_seal().is_some())
            .ok_or_else(|| sp_consensus::Error::Other("No seal found in block digest".into()))?;

        let (_, seal_data) = seal_log.as_seal().ok_or_else(|| sp_consensus::Error::Other("Invalid seal format".into()))?;

        // Convert header hash to [u8; 32]
        let header_bytes: [u8; 32] = header.hash().as_ref().try_into().expect("Failed to convert header H256 to [u8; 32]; this should never happen");
        
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

        return Ok(block_difficulty);

    }

    fn calculate_chain_difficulty(&self, chain_head: &B::Header) -> Result<U256, sp_consensus::Error> {
        // calculate cumulative difficulty of a chain
        
        let current_hash = chain_head.hash();
        
        log::info!(
            "Calculating difficulty for chain with head: {:?} (#{:?})", 
            current_hash, 
            chain_head.number()
        );
        
        if chain_head.number().is_zero() {
            // Genesis block
            let genesis_difficulty = self.client.runtime_api().get_difficulty(current_hash.clone())
                        .map_err(|e| sp_consensus::Error::Other(format!("Failed to get genesis difficulty {:?}", e).into()))?;

            return Ok(U256::from(genesis_difficulty));
        }

        let cumulative_difficulty = self.client.runtime_api().get_total_difficulty(current_hash.clone())
                .map_err(|e| sp_consensus::Error::Other(format!("Failed to get total difficulty {:?}", e).into()))?;

        let total_difficulty = U256::from(cumulative_difficulty);

        // // calculate header's difficulty
        // let block_difficulty = self.calculate_block_difficulty(chain_head).map_err(|e| sp_consensus::Error::Other(format!("Failed to get compute block difficulty {:?}", e).into()))?;
        
        // log::info!(
        //     "Block #{:?} difficulty: {:?}", 
        //     chain_head.number(), 
        //     block_difficulty
        // );
        
        // total_difficulty = total_difficulty.saturating_add(U256::from(block_difficulty));

        log::info!(
            "Total chain difficulty: {:?} for chain with head at #{:?}", 
            total_difficulty, 
            chain_head.number()
        );
        
        Ok(total_difficulty)
    }

    /// Method to find best chain when there's no current best header
    async fn find_best_chain(&self, leaves: Vec<B::Hash>) -> Result<B::Header, sp_consensus::Error> {
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

    /// Method to find Re-Org depth and fork-point
    fn find_common_ancestor_and_depth(
        &self,
        current_best: &B::Header,
        competing_header: &B::Header,
    ) -> Result<(B::Hash, u32), sp_consensus::Error> {
        let mut current_best_hash = current_best.hash();
        let mut competing_hash = competing_header.hash();
        
        let mut current_height = *current_best.number();
        let mut competing_height = *competing_header.number();

        let mut reorg_depth = 0;
        
        // First, move the headers to the same height
        while current_height > competing_height {
            if current_best_hash == competing_hash {
                // Fork point found early due to competing_header being a descendant
                return Ok((current_best_hash, reorg_depth));
            }
            current_best_hash = self.client.header(current_best_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other("Missing header".into()))?
                .parent_hash().clone();
            current_height -= One::one();
            reorg_depth += 1;
        }
        
        while competing_height > current_height {
            competing_hash = self.client.header(competing_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other("Missing header".into()))?
                .parent_hash().clone();
            competing_height -= One::one();
        }
        
        // Now both headers are at the same height
        // Find the fork-point by traversing the chain backwards
        while current_best_hash != competing_hash {
            // If current_best reaches height 0 and still no match, no common ancestor
            if current_height.is_zero() {
                return Err(sp_consensus::Error::Other("No common ancestor found".into()));
            }
            
            current_best_hash = self.client.header(current_best_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other("Missing header".into()))?
                .parent_hash().clone();
                
            competing_hash = self.client.header(competing_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other("Missing header".into()))?
                .parent_hash().clone();

            current_height -= One::one();
            reorg_depth += 1;
        }
        
        Ok((current_best_hash, reorg_depth))
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

        // the current head of the chain - will be needed to compare reorg depth
        let current_best = match self.client.info().best_hash {
            hash if hash != Default::default() => self.client.header(hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other("Missing current best header".into()))?,
            _ => {
                // If there's no current best, we don't need to find reorg depth
                return self.find_best_chain(leaves).await;
            }
        };

        let mut best_header = current_best.clone();
        let mut best_work = self.calculate_chain_difficulty(&current_best)?;
        log::info!("Current best chain: {:?} with work: {:?}", best_header.hash(), best_work);
        
        for leaf_hash in leaves {

            // skip if it's the same head as current head
            if leaf_hash == best_header.hash() {
                continue;
            }

            let header = self.client.header(leaf_hash)
                .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
                .ok_or_else(|| sp_consensus::Error::Other(format!("Missing header for {:?}", leaf_hash).into()))?;

            let chain_work = self.calculate_chain_difficulty(&header)?;

            if chain_work >= best_work {
                // This chain has more work, but we need to check reorg depth
                let (_, reorg_depth) = self.find_common_ancestor_and_depth(&current_best, &header)?;

                if reorg_depth <= self.max_reorg_depth {
                    // Switch to this chain as it's within the reorg limit
                    log::info!(
                        "Found better chain: {:?} with work: {:?}, reorg depth: {}",
                        header.hash(),
                        chain_work,
                        reorg_depth
                    );
                    // Tie breaking mechanism when chains have same amount of work
                    if chain_work == best_work {
                        let current_block_height = best_header.number();
                        let new_block_height = header.number();
                        
                        // select the chain with more blocks when chains have equal work
                        if new_block_height > current_block_height{
                            best_header = header;
                        }
                    } else {
                        best_work = chain_work;
                        best_header = header;
                    }
                    
                } else {
                    log::warn!(
                        "Ignoring chain with more work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
                        header.hash(),
                        chain_work,
                        reorg_depth,
                        self.max_reorg_depth
                    );
                }
            }
        }

        Ok(best_header)
    }
}