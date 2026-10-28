use codec::{Decode, Encode};
use primitive_types::{H256, U512};
use sc_client_api::{AuxStore, Finalizer};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{Backend as _, HeaderBackend, HeaderMetadata};
use sp_consensus::Error as ConsensusError;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::traits::{Block as BlockT, Header, NumberFor, One, Zero};
use std::{collections::HashSet, fmt};

const ACHIEVED_WORK_PREFIX: &[u8] = b"QPow:AchievedWork:";

fn achieved_work_key<H: AsRef<[u8]>>(block_hash: H) -> Vec<u8> {
	[ACHIEVED_WORK_PREFIX, block_hash.as_ref()].concat()
}

#[derive(Debug)]
pub enum ChainManagementError {
	ChainLookup(String),
	FinalizationFailed(String),
	RuntimeApiError(String),
}

impl fmt::Display for ChainManagementError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::ChainLookup(msg) => write!(f, "Chain lookup error: {}", msg),
			Self::FinalizationFailed(msg) => write!(f, "Finalization failed: {}", msg),
			Self::RuntimeApiError(msg) => write!(f, "Runtime API error: {}", msg),
		}
	}
}

impl std::error::Error for ChainManagementError {}

impl From<ChainManagementError> for ConsensusError {
	fn from(err: ChainManagementError) -> Self {
		match err {
			ChainManagementError::ChainLookup(msg) => ConsensusError::ChainLookup(msg),
			other => ConsensusError::Other(Box::new(other)),
		}
	}
}

/// Store cumulative achieved work for a block in auxiliary storage.
/// This is used for chain selection based on achieved difficulty.
pub fn store_cumulative_achieved_work<B: BlockT, C: AuxStore>(
	client: &C,
	block_hash: B::Hash,
	cumulative_work: U512,
) -> Result<(), sp_blockchain::Error> {
	let key = achieved_work_key(block_hash);
	client.insert_aux(&[(&key[..], &cumulative_work.encode()[..])], &[])?;
	log::debug!(
		target: "qpow",
		"Stored cumulative achieved work {} for block {:?}",
		cumulative_work,
		block_hash
	);
	Ok(())
}

/// Get cumulative achieved work for a block from auxiliary storage.
/// Returns U512::zero() if not found (e.g., for genesis block before initialization).
pub fn get_cumulative_achieved_work<B: BlockT, C: AuxStore>(
	client: &C,
	block_hash: B::Hash,
) -> Result<U512, sp_blockchain::Error> {
	let key = achieved_work_key(block_hash);
	match client.get_aux(&key)? {
		Some(bytes) => {
			let work = U512::decode(&mut &bytes[..]).map_err(|e| {
				sp_blockchain::Error::Backend(format!(
					"Failed to decode cumulative work for {:?}: {:?}",
					block_hash, e
				))
			})?;
			Ok(work)
		},
		None => {
			log::trace!(
				target: "qpow",
				"No cumulative achieved work found for block {:?}, returning zero",
				block_hash
			);
			Ok(U512::zero())
		},
	}
}

/// Delete cumulative achieved work for a block from auxiliary storage.
/// Used to clean up entries for finalized blocks that no longer need fork choice data.
pub fn delete_cumulative_achieved_work<B: BlockT, C: AuxStore>(
	client: &C,
	block_hash: B::Hash,
) -> Result<(), sp_blockchain::Error> {
	let key = achieved_work_key(block_hash);
	client.insert_aux(&[], &[&key[..]])?;
	log::trace!(
		target: "qpow",
		"Deleted cumulative achieved work for block {:?}",
		block_hash
	);
	Ok(())
}

/// Every block strictly above `number` that is an ancestor-or-self of one of
/// `leaves`, across all branches.
///
/// Any metadata lookup failure is returned: silently skipping a leaf would leave
/// its achieved-work entries behind after the backend removes the blocks.
fn blocks_above<B, M>(
	metadata: &M,
	leaves: impl IntoIterator<Item = B::Hash>,
	number: NumberFor<B>,
) -> Result<Vec<B::Hash>, sp_blockchain::Error>
where
	B: BlockT,
	M: HeaderMetadata<B, Error = sp_blockchain::Error>,
{
	let mut seen = HashSet::new();
	let mut out = Vec::new();
	for leaf in leaves {
		let mut hash = leaf;
		loop {
			let meta = metadata.header_metadata(hash)?;
			if meta.number <= number || !seen.insert(hash) {
				break;
			}
			out.push(hash);
			hash = meta.parent;
		}
	}
	Ok(out)
}

/// Revert up to `blocks` non-finalized blocks and drop the cumulative achieved
/// work of every block the backend actually removed.
///
/// `sc_client_db::Backend::revert` starts from the highest leaf on any branch,
/// may stop early (e.g. once it reaches state it can no longer roll back), and
/// afterwards prunes only the leaf blocks above where it stopped, leaving
/// orphaned fork ancestors in the database. The set of removed blocks is
/// therefore only known after the fact, so the candidates (everything above the
/// lowest block the backend could rewind to) are enumerated first and only
/// those whose header is gone afterwards have their entry deleted. Entries for
/// surviving blocks, including the resulting best, are kept: they are the
/// parent work source for the next import on top of them.
///
/// Logs match `sc_service::chain_ops::revert_chain`, which this replaces for
/// the `revert` subcommand.
pub fn revert<B, BE>(backend: &BE, blocks: NumberFor<B>) -> Result<(), sp_blockchain::Error>
where
	B: BlockT,
	BE: sc_client_api::Backend<B>,
{
	let blockchain = backend.blockchain();
	let info = blockchain.info();
	let revertible = blocks.min(info.best_number - info.finalized_number);
	let revert_up_to_number = info.best_number - revertible;
	let candidates = blocks_above::<B, _>(blockchain, blockchain.leaves()?, revert_up_to_number)?;

	let (reverted, _) = backend.revert(blocks, false)?;
	let info = blockchain.info();
	if reverted.is_zero() {
		log::info!("There aren't any non-finalized blocks to revert.");
	} else {
		log::info!(
			"Reverted {} blocks. Best: #{} ({})",
			reverted,
			info.best_number,
			info.best_hash
		);
		if reverted > blocks {
			log::info!(
				"Number of reverted blocks is higher than requested \
				because of reverted leaves higher than the best block."
			);
		}
	}

	let mut keys = Vec::new();
	for hash in candidates {
		if blockchain.header(hash)?.is_none() {
			keys.push(achieved_work_key(hash));
		}
	}
	let keys: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
	AuxStore::insert_aux(backend, &[], keys.iter())?;
	log::info!(
		target: "qpow",
		"Removed cumulative achieved work for {} reverted blocks",
		keys.len()
	);
	Ok(())
}

/// Initialize the genesis block's achieved work if not already set.
/// Genesis block has achieved work = 1 (no mining, but represents the start of the chain).
/// This should be called during node startup.
pub fn initialize_genesis_achieved_work<B: BlockT, C: AuxStore + HeaderBackend<B>>(
	client: &C,
) -> Result<(), sp_blockchain::Error> {
	// Get genesis hash
	let genesis_hash = client
		.hash(Zero::zero())?
		.ok_or_else(|| sp_blockchain::Error::Backend("Genesis block not found".to_string()))?;

	// Check if already initialized
	let existing = get_cumulative_achieved_work::<B, C>(client, genesis_hash)?;
	if existing != U512::zero() {
		log::debug!(
			target: "qpow",
			"Genesis achieved work already initialized to {}",
			existing
		);
		return Ok(());
	}

	// Initialize genesis achieved work to 1
	let genesis_work = U512::one();
	store_cumulative_achieved_work::<B, C>(client, genesis_hash, genesis_work)?;
	log::info!(
		target: "qpow",
		"Initialized genesis block {:?} achieved work to {}",
		genesis_hash,
		genesis_work
	);

	Ok(())
}

/// Get chain work using achieved difficulty from auxiliary storage.
/// This is the new chain selection metric based on actual work done.
pub fn get_chain_work<B, C>(client: &C, at_hash: B::Hash) -> Result<U512, sp_consensus::Error>
where
	B: BlockT,
	C: AuxStore,
{
	get_cumulative_achieved_work::<B, C>(client, at_hash).map_err(|e| {
		sp_consensus::Error::Other(
			format!("Failed to get cumulative achieved work: {:?}", e).into(),
		)
	})
}

pub fn is_heavier<N: PartialOrd>(
	candidate_work: U512,
	candidate_number: N,
	current_work: U512,
	current_number: N,
) -> bool {
	candidate_work > current_work ||
		(candidate_work == current_work && candidate_number > current_number)
}

/// Finalizes blocks that are `max_reorg_depth` blocks behind the current best block,
/// keeping exactly `max_reorg_depth` blocks reorganizable to match the configured window.
/// This should be called synchronously after each block import to ensure finalization
/// happens before the next block is imported.
///
/// Cleans up achieved work entries for canonical blocks that are now finalized.
/// Note: Non-canonical fork blocks are not cleaned up since we cannot enumerate them
/// by height. This is acceptable because fork blocks require valid PoW to create,
/// making accumulation attacks expensive, and the entries are small (~96 bytes each).
pub fn finalize_canonical_at_depth<B, C, BE>(client: &C) -> Result<(), ConsensusError>
where
	B: BlockT<Hash = H256>,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + AuxStore + Finalizer<B, BE>,
	C::Api: QPoWApi<B>,
	BE: sc_client_api::Backend<B>,
{
	log::debug!("✓✓✓ Starting finalization process");

	// Get the current best block
	let best_hash = client.info().best_hash;
	log::debug!("Current best hash: {:?}", best_hash);

	if best_hash == Default::default() {
		log::debug!("✓ No blocks to finalize - best hash is default");
		return Ok(()); // No blocks to finalize
	}

	let best_header = client
		.header(best_hash)
		.map_err(|e| {
			log::error!("Failed to get header for best hash: {:?}, error: {:?}", best_hash, e);
			ChainManagementError::ChainLookup(format!("Blockchain error: {:?}", e))
		})?
		.ok_or_else(|| {
			log::error!("Missing header for best hash: {:?}", best_hash);
			ChainManagementError::ChainLookup("Missing current best header".into())
		})?;

	let best_number = *best_header.number();
	log::debug!("Current best block number: {:?}", best_number);

	let max_reorg_depth = client.runtime_api().get_max_reorg_depth(best_hash).map_err(|e| {
		log::error!("Failed to get max reorg depth: {:?}", e);
		ChainManagementError::RuntimeApiError(format!("Failed to get max reorg depth: {:?}", e))
	})?;

	// Keep the full maximum reorganization window unfinalized.
	let finalize_depth = max_reorg_depth;

	// Only finalize if we have enough blocks
	if best_number <= finalize_depth.into() {
		log::debug!(
			"✓ Chain not long enough for finalization. Best number: {:?}, Required: > {}",
			best_number,
			finalize_depth
		);
		return Ok(()); // Chain not long enough yet
	}

	// Calculate block number to finalize
	let finalize_number = best_number - finalize_depth.into();
	log::debug!("Target block number to finalize: {:?}", finalize_number);

	// Get the hash for that block number in the current canonical chain
	let finalize_hash = client
		.hash(finalize_number)
		.map_err(|e| {
			log::error!("Failed to get hash for block #{:?}: {:?}", finalize_number, e);
			ChainManagementError::ChainLookup(format!(
				"Failed to get hash at #{:?}: {:?}",
				finalize_number, e
			))
		})?
		.ok_or_else(|| {
			log::error!("No block found at #{:?} for finalization", finalize_number);
			ChainManagementError::ChainLookup(format!("No block found at #{:?}", finalize_number))
		})?;

	log::debug!("✓ Found hash for finalization target: {:?}", finalize_hash);

	// Get last finalized block before attempting finalization
	let last_finalized_before = client.info().finalized_number;
	log::debug!("Last finalized block before attempt: {:?}", last_finalized_before);

	// Finalize the block
	client.finalize_block(finalize_hash, None, true).map_err(|e| {
		log::error!(
			"Failed to finalize block #{:?} ({:?}): {:?}",
			finalize_number,
			finalize_hash,
			e
		);
		ChainManagementError::FinalizationFailed(format!(
			"Failed to finalize block #{:?}: {:?}",
			finalize_number, e
		))
	})?;

	// Check if finalization was successful
	let last_finalized_after = client.info().finalized_number;

	log::debug!(
		"✓ Finalization stats: best={:?}, finalized={:?}, finalize_depth={}, target_finalize={:?}",
		best_number,
		last_finalized_after,
		finalize_depth,
		finalize_number
	);

	log::debug!("✓ Finalized block #{:?} ({:?})", finalize_number, finalize_hash);

	// Clean up achieved work entries for blocks that are now below the finalized tip.
	// Delete entries from last_finalized_before up to (not including) last_finalized_after.
	// The finalized tip's entry must be preserved as it's the parent work source for children.
	//
	// The loop naturally handles edge cases:
	// - If finalization didn't advance (after <= before): zero iterations
	// - If multi-step jump (e.g., bursty sync): cleans all intermediate entries
	let mut height_to_clean = last_finalized_before;
	while height_to_clean < last_finalized_after {
		if let Ok(Some(hash_to_clean)) = client.hash(height_to_clean) {
			if let Err(e) = delete_cumulative_achieved_work::<B, C>(client, hash_to_clean) {
				// Non-fatal: log warning but don't fail the finalization
				log::warn!(
					target: "qpow",
					"Failed to clean up achieved work for block #{:?}: {:?}",
					height_to_clean,
					e
				);
			}
		}
		height_to_clean += One::one();
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use sc_client_api::{
		backend::{Backend as _, BlockImportOperation as _, NewBlockState, TrieCacheContext},
		StateBackend,
	};
	use sc_client_db::Backend;
	use sp_blockchain::CachedHeaderMetadata;
	use sp_runtime::{
		testing::{Block as RawBlock, Header, MockCallU64, TestXt},
		traits::{BlakeTwo256, Hash as HashT},
		StateVersion,
	};
	use std::collections::HashMap;

	type TestBlock = RawBlock<TestXt<MockCallU64, ()>>;

	/// Insert a block with its own (trivial) state so the backend can roll it back.
	/// `salt` keeps same-height fork blocks distinct.
	fn insert_block(
		backend: &Backend<TestBlock>,
		number: u64,
		parent: H256,
		salt: u64,
		state: NewBlockState,
	) -> H256 {
		let mut op = backend.begin_operation().unwrap();
		backend
			.begin_state_operation(&mut op, if number == 0 { Default::default() } else { parent })
			.unwrap();
		let state_root = if number == 0 {
			BlakeTwo256::hash(&[0u8])
		} else {
			let key = salt.to_le_bytes();
			let (root, overlay) = backend
				.state_at(parent, TrieCacheContext::Untrusted)
				.unwrap()
				.storage_root(std::iter::once((&key[..], Some(&key[..]))), StateVersion::V1);
			op.update_db_storage(overlay).unwrap();
			root
		};
		let header = Header {
			parent_hash: parent,
			number,
			state_root,
			extrinsics_root: H256::from_low_u64_be(salt),
			digest: Default::default(),
		};
		op.set_block_data(header.clone(), Some(Vec::new()), None, None, state).unwrap();
		backend.commit_operation(op).unwrap();
		header.hash()
	}

	fn has_work(backend: &Backend<TestBlock>, hash: H256) -> bool {
		AuxStore::get_aux(backend, &achieved_work_key(hash)).unwrap().is_some()
	}

	fn has_header(backend: &Backend<TestBlock>, hash: H256) -> bool {
		backend.blockchain().header(hash).unwrap().is_some()
	}

	/// Genesis plus a canonical chain of `len` blocks, each with an achieved-work entry.
	fn canonical_chain(backend: &Backend<TestBlock>, len: u64) -> Vec<H256> {
		let mut hashes =
			vec![insert_block(backend, 0, Default::default(), 0, NewBlockState::Final)];
		for n in 1..=len {
			hashes.push(insert_block(backend, n, hashes[n as usize - 1], n, NewBlockState::Best));
		}
		for (n, hash) in hashes.iter().enumerate() {
			store_cumulative_achieved_work::<TestBlock, _>(backend, *hash, U512::from(100 + n))
				.unwrap();
		}
		hashes
	}

	#[test]
	fn revert_drops_work_for_reverted_blocks_and_keeps_the_rest() {
		let backend = Backend::<TestBlock>::new_test(1024, 1024);
		let chain = canonical_chain(&backend, 5);

		revert::<TestBlock, _>(&backend, 2).unwrap();

		assert_eq!(backend.blockchain().info().best_hash, chain[3]);
		for (n, hash) in chain.iter().enumerate() {
			assert_eq!(has_header(&backend, *hash), n <= 3, "header #{n}");
			assert_eq!(has_work(&backend, *hash), n <= 3, "work #{n}");
		}
		assert_eq!(
			get_cumulative_achieved_work::<TestBlock, _>(&backend, chain[3]).unwrap(),
			U512::from(103)
		);
	}

	#[test]
	fn partial_backend_revert_keeps_work_for_surviving_blocks() {
		// With a short canonicalization delay the oldest blocks are already
		// canonicalized, so the backend stops rolling back before reaching the
		// requested depth. Whatever survives must keep its achieved work.
		let backend = Backend::<TestBlock>::new_test(1024, 2);
		let chain = canonical_chain(&backend, 5);

		revert::<TestBlock, _>(&backend, 5).unwrap();

		let best = backend.blockchain().info().best_number as usize;
		assert!(best > 0 && best < 5, "backend must have stopped early, best=#{best}");
		for (n, hash) in chain.iter().enumerate() {
			assert_eq!(has_header(&backend, *hash), n <= best, "header #{n}");
			assert_eq!(has_work(&backend, *hash), n <= best, "work #{n}");
		}
	}

	#[test]
	fn revert_covers_taller_fork_that_diverges_below_the_target() {
		// Canonical G -> A1 -> A2 -> A3 (best), fork A1 -> F2 -> F3 -> F4 (taller leaf).
		// Reverting one block targets A2: the backend removes A3 and the fork leaf F4
		// but leaves F2/F3 orphaned in the database, so only A3 and F4 lose their work.
		let backend = Backend::<TestBlock>::new_test(1024, 1024);
		let canonical = canonical_chain(&backend, 3);
		let mut fork = vec![canonical[1]];
		for n in 2..=4u64 {
			let hash =
				insert_block(&backend, n, fork[n as usize - 2], 100 + n, NewBlockState::Normal);
			store_cumulative_achieved_work::<TestBlock, _>(&backend, hash, U512::from(200 + n))
				.unwrap();
			fork.push(hash);
		}
		let (a3, f2, f3, f4) = (canonical[3], fork[1], fork[2], fork[3]);
		assert_eq!(backend.blockchain().info().best_hash, a3);

		revert::<TestBlock, _>(&backend, 1).unwrap();

		assert_eq!(backend.blockchain().info().best_hash, canonical[2]);
		for hash in [a3, f4] {
			assert!(!has_header(&backend, hash));
			assert!(!has_work(&backend, hash), "removed block must lose its work");
		}
		for hash in [canonical[0], canonical[1], canonical[2], f2, f3] {
			assert!(has_header(&backend, hash));
			assert!(has_work(&backend, hash), "surviving block must keep its work");
		}
	}

	struct MetadataMap(HashMap<H256, CachedHeaderMetadata<TestBlock>>);

	impl HeaderMetadata<TestBlock> for MetadataMap {
		type Error = sp_blockchain::Error;

		fn header_metadata(
			&self,
			hash: H256,
		) -> Result<CachedHeaderMetadata<TestBlock>, Self::Error> {
			self.0
				.get(&hash)
				.cloned()
				.ok_or_else(|| sp_blockchain::Error::UnknownBlock(format!("{hash:?}")))
		}

		fn insert_header_metadata(&self, _: H256, _: CachedHeaderMetadata<TestBlock>) {}

		fn remove_header_metadata(&self, _: H256) {}
	}

	fn metadata_tree() -> (MetadataMap, Vec<H256>, Vec<H256>) {
		let mut map = HashMap::new();
		let mut add = |number: u64, parent: H256, salt: u64| {
			let header = Header {
				parent_hash: parent,
				number,
				state_root: Default::default(),
				extrinsics_root: H256::from_low_u64_be(salt),
				digest: Default::default(),
			};
			let hash = header.hash();
			map.insert(hash, CachedHeaderMetadata::from(&header));
			hash
		};
		let g = add(0, Default::default(), 0);
		let a1 = add(1, g, 1);
		let a2 = add(2, a1, 2);
		let a3 = add(3, a2, 3);
		let f2 = add(2, a1, 102);
		let f3 = add(3, f2, 103);
		(MetadataMap(map), vec![g, a1, a2, a3], vec![f2, f3])
	}

	#[test]
	fn blocks_above_walks_every_branch_once() {
		let (map, canonical, fork) = metadata_tree();
		let found = blocks_above::<TestBlock, _>(&map, [canonical[3], fork[1]], 1).unwrap();
		let expected: HashSet<_> = [canonical[2], canonical[3], fork[0], fork[1]].into();
		assert_eq!(found.len(), expected.len());
		assert_eq!(found.into_iter().collect::<HashSet<_>>(), expected);
	}

	#[test]
	fn blocks_above_propagates_lookup_failures() {
		let (mut map, canonical, fork) = metadata_tree();
		map.0.remove(&fork[0]);
		let err = blocks_above::<TestBlock, _>(&map, [canonical[3], fork[1]], 1).unwrap_err();
		assert!(matches!(err, sp_blockchain::Error::UnknownBlock(_)), "{err:?}");
	}
}
