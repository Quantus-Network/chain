use std::marker::PhantomData;
use std::sync::Arc;
use primitive_types::{H256, U256};
use sc_client_api::{AuxStore, BlockBackend, Finalizer};
use sp_api::__private::BlockT;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{Backend, HeaderBackend};
use sp_consensus::SelectChain;
use sp_runtime::traits::{Header, One, Zero};
use sp_consensus_qpow::QPoWApi;
use crate::QPowAlgorithm;

const IGNORED_CHAINS_PREFIX: &[u8] = b"QPow:IgnoredChains:";

pub struct HeaviestChain<B, C, BE>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore,
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
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore,
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
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> + AuxStore + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
    BE: sc_client_api::Backend<B> + 'static,
{
    pub fn new(backend: Arc<BE>, client: Arc<C>, algorithm: QPowAlgorithm<B,C>) -> Self {

        let genesis_hash = client.hash(Zero::zero())
            .expect("Failed to get gehesis hash")
            .expect("Genesis block must exist");
        let max_reorg_depth = client.runtime_api().get_max_reorg_depth(genesis_hash)
            .expect("Failed to get max reorg depth");

        Self {
            backend,
            client,
            algorithm,
            max_reorg_depth,
            _phantom: PhantomData
        }
    }

    /// Finalizes blocks that are `max_reorg_depth - 1` blocks behind the current best block
    pub fn finalize_canonical_at_depth(&self) -> Result<(), sp_consensus::Error>
    where
        C: Finalizer<B, BE>,
    {
        // Get the current best block
        let best_hash = self.client.info().best_hash;
        if best_hash == Default::default() {
            return Ok(());  // No blocks to finalize
        }

        let best_header = self.client.header(best_hash)
            .map_err(|e| sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into()))?
            .ok_or_else(|| sp_consensus::Error::Other("Missing current best header".into()))?;

        let best_number = *best_header.number();

        // Calculate how far back to finalize
        let finalize_depth = self.max_reorg_depth.saturating_sub(1);

        // Only finalize if we have enough blocks
        if best_number <= finalize_depth.into() {
            return Ok(());  // Chain not long enough yet
        }

        // Calculate block number to finalize
        let finalize_number = best_number - finalize_depth.into();

        // Get the hash for that block number in the current canonical chain
        let finalize_hash = self.client.hash(finalize_number)
            .map_err(|e| sp_consensus::Error::Other(format!("Failed to get hash at #{}: {:?}", finalize_number, e).into()))?
            .ok_or_else(|| sp_consensus::Error::Other(format!("No block found at #{}", finalize_number).into()))?;

        // Finalize the block
        self.client.finalize_block(finalize_hash, None, true)
            .map_err(|e| sp_consensus::Error::Other(format!("Failed to finalize block #{}: {:?}", finalize_number, e).into()))?;

        log::info!("✓ Finalized block #{} ({:?})", finalize_number, finalize_hash);

        Ok(())
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
            log::info!("Calculating difficulty for genesis block: {} ",genesis_difficulty);
            return Ok(U256::from(genesis_difficulty));
        }

        let cumulative_difficulty = self.client.runtime_api().get_total_difficulty(current_hash.clone())
            .map_err(|e| sp_consensus::Error::Other(format!("Failed to get total difficulty {:?}", e).into()))?;

        let total_difficulty = U256::from(cumulative_difficulty);

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

        log::info!(
            "Fork-point ----------------------- found: {:?} at height: {:?} with reorg depth: {}",
            current_best_hash,
            current_height,
            reorg_depth);

        Ok((current_best_hash, reorg_depth))
    }

    fn is_chain_ignored(&self, hash: &B::Hash) -> Result<bool, sp_consensus::Error> {
        let key = ignored_chain_key(hash);

        match self.client.get_aux(&key) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(sp_consensus::Error::Other(format!("Failed to check ignored chain: {:?}", e).into())),
        }
    }

    fn add_ignored_chain(&self, hash: B::Hash) -> Result<(), sp_consensus::Error> {
        let key = ignored_chain_key(&hash);

        //This storage isn't super advanced. We can only add or remove value, updates are impossible.

        let empty_value = vec![];

        self.client.insert_aux(&[(key.as_slice(), empty_value.as_slice())], &[])
            .map_err(|e| sp_consensus::Error::Other(format!("Failed to add ignored chain: {:?}", e).into()))
    }
}

#[async_trait::async_trait]
impl<B, C, BE> SelectChain<B> for HeaviestChain<B, C, BE>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + HeaderBackend<B> + BlockBackend<B> +AuxStore + Send + Sync + 'static,
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

        // Get access to the ignored chains
        //let mut ignored_chains = self.ignored_chains.lock().unwrap();

        for leaf_hash in leaves {

            // Skip if it's the current best or already ignored
            if leaf_hash == best_header.hash() || self.is_chain_ignored(&leaf_hash)? {
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
                    self.add_ignored_chain(leaf_hash)?;
                    log::warn!(
                        "Permanently ignoring chain with more work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
                        header.hash(),
                        chain_work,
                        reorg_depth,
                        self.max_reorg_depth
                    );
                }
            }
            else{
                // This chain has less work - check if it should be ignored
                let (_, reorg_depth) = self.find_common_ancestor_and_depth(&current_best, &header)?;

                if reorg_depth > self.max_reorg_depth {
                    self.add_ignored_chain(leaf_hash)?;
                    log::warn!(
                        "Permanently ignoring chain with less work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
                        leaf_hash,
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

fn ignored_chain_key<T: AsRef<[u8]>>(hash: &T) -> Vec<u8> {
    IGNORED_CHAINS_PREFIX.iter().chain(hash.as_ref()).copied().collect()
}