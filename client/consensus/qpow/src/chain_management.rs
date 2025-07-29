use crate::QPowAlgorithm;
use futures::StreamExt;
use primitive_types::{H256, U512};
use sc_client_api::{AuxStore, BlockBackend, BlockchainEvents, Finalizer};
use sc_service::TaskManager;
use sp_api::ProvideRuntimeApi;
use sp_api::__private::BlockT;
use sp_blockchain::{Backend, HeaderBackend};
use sp_consensus::SelectChain;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Header, One, Zero};
use std::marker::PhantomData;
use std::sync::Arc;

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
    pub fn new(backend: Arc<BE>, client: Arc<C>, algorithm: QPowAlgorithm<B, C>) -> Self {
        log::debug!("Creating new HeaviestChain instance");

        Self {
            backend,
            client,
            algorithm,
            _phantom: PhantomData,
        }
    }

    /// Finalizes blocks that are `max_reorg_depth - 1` blocks behind the current best block
    pub fn finalize_canonical_at_depth(&self) -> Result<(), sp_consensus::Error>
    where
        C: Finalizer<B, BE>,
    {
        log::debug!("✓✓✓ Starting finalization process");

        // Get the current best block
        let best_hash = self.client.info().best_hash;
        log::debug!("Current best hash: {:?}", best_hash);

        if best_hash == Default::default() {
            log::debug!("✓ No blocks to finalize - best hash is default");
            return Ok(()); // No blocks to finalize
        }

        let best_header = self
            .client
            .header(best_hash)
            .map_err(|e| {
                log::error!(
                    "Failed to get header for best hash: {:?}, error: {:?}",
                    best_hash,
                    e
                );
                sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
            })?
            .ok_or_else(|| {
                log::error!("Missing header for best hash: {:?}", best_hash);
                sp_consensus::Error::Other("Missing current best header".into())
            })?;

        let best_number = *best_header.number();
        log::debug!("Current best block number: {}", best_number);

        let max_reorg_depth = self
            .client
            .runtime_api()
            .get_max_reorg_depth(best_hash)
            .expect("Failed to get max reorg depth");

        // Calculate how far back to finalize
        let finalize_depth = max_reorg_depth.saturating_sub(1);

        // Only finalize if we have enough blocks
        if best_number <= finalize_depth.into() {
            log::debug!(
                "✓ Chain not long enough for finalization. Best number: {}, Required: > {}",
                best_number,
                finalize_depth
            );
            return Ok(()); // Chain not long enough yet
        }

        // Calculate block number to finalize
        let finalize_number = best_number - finalize_depth.into();
        log::debug!("Target block number to finalize: {}", finalize_number);

        // Get the hash for that block number in the current canonical chain
        let finalize_hash = self
            .client
            .hash(finalize_number)
            .map_err(|e| {
                log::error!("Failed to get hash for block #{}: {:?}", finalize_number, e);
                sp_consensus::Error::Other(
                    format!("Failed to get hash at #{}: {:?}", finalize_number, e).into(),
                )
            })?
            .ok_or_else(|| {
                log::error!("No block found at #{} for finalization", finalize_number);
                sp_consensus::Error::Other(format!("No block found at #{}", finalize_number).into())
            })?;

        log::debug!("✓ Found hash for finalization target: {:?}", finalize_hash);

        // Get last finalized block before attempting finalization
        let last_finalized_before = self.client.info().finalized_number;
        log::debug!(
            "Last finalized block before attempt: #{}",
            last_finalized_before
        );

        // Finalize the block
        self.client
            .finalize_block(finalize_hash, None, true)
            .map_err(|e| {
                log::error!(
                    "Failed to finalize block #{} ({:?}): {:?}",
                    finalize_number,
                    finalize_hash,
                    e
                );
                sp_consensus::Error::Other(
                    format!("Failed to finalize block #{}: {:?}", finalize_number, e).into(),
                )
            })?;

        // Check if finalization was successful
        let last_finalized_after = self.client.info().finalized_number;

        log::debug!(
            "✓ Finalization stats: best={}, finalized={}, finalize_depth={}, target_finalize={}",
            best_number,
            last_finalized_after,
            finalize_depth,
            finalize_number
        );

        log::info!(
            "✓ Finalized block #{} ({:?})",
            finalize_number,
            finalize_hash
        );

        Ok(())
    }

    fn calculate_chain_work(&self, chain_head: &B::Header) -> Result<U512, sp_consensus::Error> {
        // calculate cumulative work of a chain

        let current_hash = chain_head.hash();
        let current_number = *chain_head.number();

        let total_work = self
            .client
            .runtime_api()
            .get_total_work(current_hash)
            .map_err(|e| {
                log::error!(
                    "Failed to get total work for chain with head #{}: {:?}",
                    current_number,
                    e
                );
                sp_consensus::Error::Other(format!("Failed to get total difficulty {:?}", e).into())
            })?;

        log::info!(
            "⛏️ Total chain work: {:?} for chain with head at #{:?} hash: {:?}",
            total_work,
            current_number,
            current_hash
        );

        Ok(total_work)
    }

    /// Method to find best chain when there's no current best header
    async fn find_best_chain(
        &self,
        leaves: Vec<B::Hash>,
    ) -> Result<B::Header, sp_consensus::Error> {
        log::debug!(
            "Finding best chain among {} leaves when no current best exists",
            leaves.len()
        );

        let mut best_header = None;
        let mut best_work = U512::zero();

        for (idx, leaf_hash) in leaves.iter().enumerate() {
            log::debug!(
                "Checking leaf [{}/{}]: {:?}",
                idx + 1,
                leaves.len(),
                leaf_hash
            );

            let header = self
                .client
                .header(*leaf_hash)
                .map_err(|e| {
                    log::error!(
                        "Blockchain error when getting header for leaf {:?}: {:?}",
                        leaf_hash,
                        e
                    );
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!("Missing header for leaf {:?}", leaf_hash);
                    sp_consensus::Error::Other(format!("Missing header for {:?}", leaf_hash).into())
                })?;

            let header_number = *header.number();
            log::debug!("Found header for leaf at height #{}", header_number);

            let chain_work = self.calculate_chain_work(&header)?;
            log::debug!("Chain work for leaf #{}: {}", header_number, chain_work);

            if chain_work > best_work {
                log::debug!(
                    "Found new best chain candidate: #{} (hash: {:?}) with work: {}",
                    header_number,
                    leaf_hash,
                    chain_work
                );
                best_work = chain_work;
                best_header = Some(header);
            } else {
                log::debug!(
                    "Leaf #{} (hash: {:?}) has less work ({}) than current best ({})",
                    header_number,
                    leaf_hash,
                    chain_work,
                    best_work
                );
            }
        }

        if let Some(ref header) = best_header {
            log::info!(
                "Selected best chain with head at #{} (hash: {:?}) with total work: {}",
                header.number(),
                header.hash(),
                best_work
            );
        } else {
            log::error!("No valid chain found among the leaves");
        }

        best_header.ok_or(sp_consensus::Error::Other("No Valid Chain Found".into()))
    }

    /// Method to find Re-Org depth and fork-point
    fn find_common_ancestor_and_depth(
        &self,
        current_best: &B::Header,
        competing_header: &B::Header,
    ) -> Result<(B::Hash, u32), sp_consensus::Error> {
        let current_best_hash = current_best.hash();
        let competing_hash = competing_header.hash();
        let current_height = *current_best.number();
        let competing_height = *competing_header.number();

        log::debug!(
            "Finding common ancestor between current best #{} ({:?}) and competing #{} ({:?})",
            current_height,
            current_best_hash,
            competing_height,
            competing_hash
        );

        // Quick check for identical headers
        if current_best_hash == competing_hash {
            log::debug!("Headers are identical, no reorganization needed");
            return Ok((current_best_hash, 0));
        }

        let mut current_best_hash = current_best_hash;
        let mut competing_hash = competing_hash;
        let mut current_height = current_height;
        let mut competing_height = competing_height;
        let mut reorg_depth = 0;

        // First, move the headers to the same height
        log::debug!(
            "Phase 1: Aligning heights - current: {}, competing: {}",
            current_height,
            competing_height
        );

        while current_height > competing_height {
            // Check if the blocks are identical during descent
            if current_best_hash == competing_hash {
                // Fork point found early
                log::debug!(
                    "Early fork point found during height alignment: {:?} at height {} with reorg depth {}",
                    current_best_hash,
                    current_height,
                    reorg_depth
                );
                return Ok((current_best_hash, reorg_depth));
            }

            log::debug!(
                "Current chain higher: moving down from #{} ({:?}), reorg_depth: {}",
                current_height,
                current_best_hash,
                reorg_depth
            );

            current_best_hash = *self
                .client
                .header(current_best_hash)
                .map_err(|e| {
                    log::error!(
                        "Blockchain error when getting header for #{}: {:?}",
                        current_height,
                        e
                    );
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!(
                        "Missing header at #{} ({:?})",
                        current_height,
                        current_best_hash
                    );
                    sp_consensus::Error::Other("Missing header".into())
                })?
                .parent_hash();

            current_height -= One::one();
            reorg_depth += 1;

            log::debug!(
                "Moved down current chain to #{} ({:?}), reorg_depth now: {}",
                current_height,
                current_best_hash,
                reorg_depth
            );
        }

        // Similarly, if the competing chain is taller, move it down to the same height
        log::debug!(
            "Phase 2: Aligning heights if competing chain is taller - current: {}, competing: {}",
            current_height,
            competing_height
        );

        while competing_height > current_height {
            reorg_depth += 1;
            log::debug!(
                "Competing chain higher: moving down from #{} ({:?})",
                competing_height,
                competing_hash
            );

            competing_hash = *self
                .client
                .header(competing_hash)
                .map_err(|e| {
                    log::error!(
                        "Blockchain error when getting header for competing chain #{}: {:?}",
                        competing_height,
                        e
                    );
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!(
                        "Missing header for competing chain at #{} ({:?})",
                        competing_height,
                        competing_hash
                    );
                    sp_consensus::Error::Other("Missing header".into())
                })?
                .parent_hash();

            competing_height -= One::one();

            log::debug!(
                "Moved down competing chain to #{} ({:?})",
                competing_height,
                competing_hash
            );
        }

        log::debug!(
            "Phase 3: Both chains now at height {} - finding fork point",
            current_height
        );

        // Now both headers are at the same height
        // Find the fork-point by traversing backwards
        while current_best_hash != competing_hash {
            // If we reach genesis and still no match, no common ancestor
            if current_height.is_zero() {
                log::error!("Reached genesis block without finding common ancestor");
                return Err(sp_consensus::Error::Other(
                    "No common ancestor found".into(),
                ));
            }

            log::debug!(
                "Blocks at #{} differ: current ({:?}) vs competing ({:?})",
                current_height,
                current_best_hash,
                competing_hash
            );

            // Move down one block in the current best chain
            current_best_hash = *self
                .client
                .header(current_best_hash)
                .map_err(|e| {
                    log::error!(
                        "Blockchain error when getting parent at #{}: {:?}",
                        current_height,
                        e
                    );
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!(
                        "Missing header for parent at #{} ({:?})",
                        current_height,
                        current_best_hash
                    );
                    sp_consensus::Error::Other("Missing header".into())
                })?
                .parent_hash();

            // Move down one block in the competing chain
            competing_hash = *self
                .client
                .header(competing_hash)
                .map_err(|e| {
                    log::error!(
                        "Blockchain error when getting parent for competing chain at #{}: {:?}",
                        current_height,
                        e
                    );
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!(
                        "Missing header for competing chain parent at #{} ({:?})",
                        current_height,
                        competing_hash
                    );
                    sp_consensus::Error::Other("Missing header".into())
                })?
                .parent_hash();

            // Both chains are now one block lower
            current_height -= One::one();
            // Each step backwards increases the reorganization depth
            reorg_depth += 1;

            log::debug!(
                "Moved both chains down to #{}, current ({:?}), competing ({:?}), reorg_depth: {}",
                current_height,
                current_best_hash,
                competing_hash,
                reorg_depth
            );
        }

        // Log the fork point and reorg depth for debugging
        log::warn!(
            "Fork-point ----------------------- found: {:?} at height: {:?} with reorg depth: {}",
            current_best_hash,
            current_height,
            reorg_depth
        );

        Ok((current_best_hash, reorg_depth))
    }

    fn is_chain_ignored(&self, hash: &B::Hash) -> Result<bool, sp_consensus::Error> {
        log::debug!("Checking if chain with head {:?} is ignored", hash);

        let key = ignored_chain_key(hash);

        match self.client.get_aux(&key) {
            Ok(Some(_)) => {
                log::debug!("Chain with head {:?} is ignored", hash);
                Ok(true)
            }
            Ok(None) => {
                log::debug!("Chain with head {:?} is not ignored", hash);
                Ok(false)
            }
            Err(e) => {
                log::error!(
                    "Failed to check if chain with head {:?} is ignored: {:?}",
                    hash,
                    e
                );
                Err(sp_consensus::Error::Other(
                    format!("Failed to check ignored chain: {:?}", e).into(),
                ))
            }
        }
    }

    fn add_ignored_chain(&self, hash: B::Hash) -> Result<(), sp_consensus::Error> {
        log::debug!("Adding chain with head {:?} to ignored chains", hash);

        let key = ignored_chain_key(&hash);

        //This storage isn't super advanced. We can only add or remove value, updates are impossible.

        let empty_value = vec![];

        self.client
            .insert_aux(&[(key.as_slice(), empty_value.as_slice())], &[])
            .map_err(|e| {
                log::error!(
                    "Failed to add chain with head {:?} to ignored chains: {:?}",
                    hash,
                    e
                );
                sp_consensus::Error::Other(format!("Failed to add ignored chain: {:?}", e).into())
            })
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
    async fn leaves(&self) -> Result<Vec<B::Hash>, sp_consensus::Error> {
        log::debug!("Getting blockchain leaves");

        let leaves = self.backend.blockchain().leaves().map_err(|e| {
            log::error!("Failed to fetch leaves: {:?}", e);
            sp_consensus::Error::Other(format!("Failed to fetch leaves: {:?}", e).into())
        })?;

        log::debug!("Found {} leaves", leaves.len());

        Ok(leaves)
    }

    async fn best_chain(&self) -> Result<B::Header, sp_consensus::Error> {
        log::debug!(target: "qpow", "------ 🍴️Starting best chain selection process ------");

        let leaves = self.backend.blockchain().leaves().map_err(|e| {
            log::error!("🍴️ Failed to fetch leaves: {:?}", e);
            sp_consensus::Error::Other(format!("Failed to fetch leaves: {:?}", e).into())
        })?;

        log::debug!(target: "qpow", "🍴️ Found {} leaves to evaluate", leaves.len());

        if leaves.is_empty() {
            log::error!("🍴️ Blockchain has no leaves");
            return Err(sp_consensus::Error::Other(
                "Blockchain has no leaves".into(),
            ));
        }

        // Get info about last finalized block
        let finalized_number = self.client.info().finalized_number;
        log::debug!(target: "qpow", "🍴️ Current finalized block: #{}", finalized_number);

        // the current head of the chain - will be needed to compare reorg depth
        let current_best = match self.client.info().best_hash {
            hash if hash != Default::default() => {
                log::debug!(target: "qpow", "🍴️ Current best hash: {:?}", hash);

                self.client
                    .header(hash)
                    .map_err(|e| {
                        log::error!(
                            "🍴️ Blockchain error when getting header for best hash: {:?}",
                            e
                        );
                        sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                    })?
                    .ok_or_else(|| {
                        log::error!("🍴️ Missing header for current best hash: {:?}", hash);
                        sp_consensus::Error::Other("Missing current best header".into())
                    })?
            }
            _ => {
                // If there's no current best, we don't need to find reorg depth
                log::debug!(
                    target: "qpow",
                    "🍴️ No current best hash, finding best chain without reorg constraints"
                );
                return self.find_best_chain(leaves).await;
            }
        };

        let current_best_number = *current_best.number();
        log::debug!(
            target: "qpow",
            "🍴️ Current best block: #{} ({:?})",
            current_best_number,
            current_best.hash()
        );

        let mut best_header = current_best.clone();
        let mut best_work = self.calculate_chain_work(&current_best)?;
        log::debug!(
            target: "qpow",
            "🍴️ Current best chain: {:?} with work: {:?}",
            best_header.hash(),
            best_work
        );

        log::debug!(
            target: "qpow",
            "🍴️ Evaluating {} leaves for potential best chain",
            leaves.len()
        );

        for (idx, leaf_hash) in leaves.iter().enumerate() {
            log::debug!(
                target: "qpow",
                "🍴️ Evaluating leaf [{}/{}]: {:?}",
                idx + 1,
                leaves.len(),
                leaf_hash
            );

            // Skip if it's the current best or already ignored
            if *leaf_hash == best_header.hash() {
                log::debug!(target: "qpow", "🍴️ Skipping leaf {:?} - it's the current best", leaf_hash);
                continue;
            }

            if self.is_chain_ignored(leaf_hash)? {
                log::debug!(
                    target: "qpow",
                    "🍴️ Skipping leaf {:?} - it's in the ignored list",
                    leaf_hash
                );
                continue;
            }

            let header = self
                .client
                .header(*leaf_hash)
                .map_err(|e| {
                    log::error!("🍴️ Blockchain error when getting header for leaf: {:?}", e);
                    sp_consensus::Error::Other(format!("Blockchain error: {:?}", e).into())
                })?
                .ok_or_else(|| {
                    log::error!("🍴️ Missing header for leaf hash: {:?}", leaf_hash);
                    sp_consensus::Error::Other(format!("Missing header for {:?}", leaf_hash).into())
                })?;

            let header_number = *header.number();
            log::debug!(target: "qpow", "🍴️ Found header for leaf at height #{}", header_number);

            let chain_work = self.calculate_chain_work(&header)?;
            log::debug!(target: "qpow", "🍴️ Chain work for leaf #{}: {}", header_number, chain_work);

            let max_reorg_depth = self
                .client
                .runtime_api()
                .get_max_reorg_depth(best_header.hash())
                .expect("Failed to get max reorg depth");
            log::debug!(target: "qpow", "🍴️ Max reorg depth from runtime: {}", max_reorg_depth);

            if chain_work >= best_work {
                // This chain has more work, but we need to check reorg depth
                log::debug!(
                    target: "qpow",
                    "🍴️ Chain with head #{} ({:?}) has at least as much work ({}) as current best ({}), checking reorg depth",
                    header_number,
                    leaf_hash,
                    chain_work,
                    best_work
                );

                let (fork_point, reorg_depth) =
                    self.find_common_ancestor_and_depth(&current_best, &header)?;
                log::debug!(
                    target: "qpow",
                    "🍴️ Found common ancestor with hash {:?} with reorg depth: {}",
                    fork_point,
                    reorg_depth
                );

                if reorg_depth <= max_reorg_depth {
                    // Switch to this chain as it's within the reorg limit
                    log::debug!(
                        target: "qpow",
                        "🍴️ Found better chain: {:?} with work: {:?}, reorg depth: {} (within limit of {})",
                        header.hash(),
                        chain_work,
                        reorg_depth,
                        max_reorg_depth
                    );

                    // Tie breaking mechanism when chains have same amount of work
                    if chain_work == best_work {
                        let current_block_height = best_header.number();
                        let new_block_height = header.number();

                        log::debug!(
                            target: "qpow",
                            "🍴️ Chain work is equal, comparing block heights: current #{}, new #{}",
                            current_block_height,
                            new_block_height
                        );

                        // select the chain with more blocks when chains have equal work
                        if new_block_height > current_block_height {
                            log::debug!(
                                target: "qpow",
                                "🍴️ Switching to chain with more blocks: #{} > #{}",
                                new_block_height,
                                current_block_height
                            );
                            best_header = header;
                        } else {
                            log::debug!(
                                target: "qpow",
                                "🍴️ Keeping current chain as it has at least as many blocks: #{} >= #{}",
                                current_block_height,
                                new_block_height
                            );
                        }
                    } else {
                        log::debug!(
                            target: "qpow",
                            "🍴️ Switching to chain with more work: {} > {}",
                            chain_work,
                            best_work
                        );
                        best_work = chain_work;
                        best_header = header;
                    }
                } else {
                    log::debug!(
                        target: "qpow",
                        "🍴️ Chain with more work exceeds reorg limit: {} > {}. Adding to ignored chains.",
                        reorg_depth,
                        max_reorg_depth
                    );

                    self.add_ignored_chain(*leaf_hash)?;
                    log::warn!(
                        target: "qpow",
                        "🍴️ Permanently ignoring chain with more work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
                        header.hash(),
                        chain_work,
                        reorg_depth,
                        max_reorg_depth
                    );
                }
            } else {
                // This chain has less work - check if it should be ignored
                log::debug!(
                    target: "qpow",
                    "🍴️ Chain has less work ({} < {}), checking if it should be ignored",
                    chain_work,
                    best_work
                );

                let (fork_point, reorg_depth) =
                    self.find_common_ancestor_and_depth(&current_best, &header)?;
                log::debug!(
                    target: "qpow",
                    "🍴️ Found common ancestor with hash {:?} with reorg depth: {}",
                    fork_point,
                    reorg_depth
                );

                if reorg_depth > max_reorg_depth {
                    log::debug!(
                        target: "qpow",
                        "🍴️ Chain exceeds reorg limit: {} > {}. Adding to ignored chains.",
                        reorg_depth,
                        max_reorg_depth
                    );

                    self.add_ignored_chain(*leaf_hash)?;
                    log::debug!(
                        target: "qpow",
                        "🍴️ Permanently ignoring chain with less work: {:?} (work: {:?}) due to excessive reorg depth: {} > {}",
                        leaf_hash,
                        chain_work,
                        reorg_depth,
                        max_reorg_depth
                    );
                } else {
                    log::debug!(
                        target: "qpow",
                        "🍴️ Chain has less work but is within reorg limit: {} <= {}. Keeping in consideration.",
                        reorg_depth,
                        max_reorg_depth
                    );
                }
            }
        }

        if leaves.len() > 1 {
            log::info!(
                "🍴️ Evaluated {} leaves and selected best chain with head: #{} ({:?}) and work: {}",
                leaves.len(),
                best_header.number(),
                best_header.hash(),
                best_work
            );
        } else {
            log::debug!(
                target: "qpow",
                "🍴️ Finished chain selection. Selected best chain with head: #{} ({:?}) and work: {}",
                best_header.number(),
                best_header.hash(),
                best_work
            );
        }

        Ok(best_header)
    }
}

fn ignored_chain_key<T: AsRef<[u8]>>(hash: &T) -> Vec<u8> {
    IGNORED_CHAINS_PREFIX
        .iter()
        .chain(hash.as_ref())
        .copied()
        .collect()
}

pub struct ChainManagement;

impl ChainManagement {
    /// Start a task that listens for block imports and triggers finalization
    pub fn spawn_finalization_task<B, C, BE>(
        select_chain: Arc<HeaviestChain<B, C, BE>>,
        task_manager: &TaskManager,
    ) where
        B: BlockT<Hash = H256>,
        C: ProvideRuntimeApi<B>
            + HeaderBackend<B>
            + BlockBackend<B>
            + AuxStore
            + BlockchainEvents<B>
            + Finalizer<B, BE>
            + Send
            + Sync
            + 'static,
        C::Api: QPoWApi<B>,
        BE: sc_client_api::Backend<B> + 'static,
    {
        log::info!("⛓️ Spawning chain finalization task");

        task_manager
            .spawn_essential_handle()
            .spawn("chain_finalization", None, async move {
                log::info!("⛓️ Chain finalization task spawned");

                let mut import_notification_stream =
                    select_chain.client.import_notification_stream();
                log::debug!("⛓️ Listening for block import notifications");

                while let Some(notification) = import_notification_stream.next().await {
                    if let Err(e) = select_chain.finalize_canonical_at_depth() {
                        log::error!("⛓️ Failed to finalize blocks: {:?}", e);
                    } else {
                        log::debug!(
                            "⛓️ Successfully processed finalization after import of block #{}",
                            notification.header.number()
                        );
                    }
                }

                log::info!("Block import notification stream ended");
            });
    }
}
