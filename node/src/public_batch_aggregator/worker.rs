use std::{collections::HashSet, sync::Arc, time::Duration};

use codec::Decode;
use frame_support::storage::storage_prefix;
use futures::{FutureExt, StreamExt};
use jsonrpsee::tokio;
use plonky2::plonk::proof::ProofWithPublicInputs;
use qp_wormhole_aggregator::{
	pool::{BatchKey, BucketStats},
	public_batch::prover::lib::{PublicBatchInputs, PublicBatchProver},
};
use qp_wormhole_inputs::BytesDigest;
use qp_zk_circuits_common::circuit::{C, D, F};
use quantus_runtime::{opaque::Block, Runtime, RuntimeCall, RuntimeEvent, UncheckedExtrinsic};
use sc_client_api::{BlockchainEvents, StorageProvider};
use sc_service::{SpawnTaskHandle, TaskManager};
use sc_transaction_pool_api::{TransactionPool, TransactionSource};
use sp_blockchain::HeaderBackend;
use sp_core::{storage::StorageKey, H256};
use sp_runtime::OpaqueExtrinsic;

use super::{interceptor::parse_private_batch_inputs, Aggregator, AggregatorConfig, LOG_TARGET};
use crate::service::FullClient;

type Proof = ProofWithPublicInputs<F, C, D>;
type TxPool = sc_transaction_pool::TransactionPoolHandle<Block, FullClient>;

/// Spawn the aggregator's background work: the one-off prover build and the
/// per-block eviction / proving policy loop.
///
/// The loop is an essential task: the operator opted into aggregation, so a
/// prover that cannot be built must stop the node rather than silently pool
/// proofs it will never prove.
pub fn spawn(
	aggregator: Arc<Aggregator>,
	client: Arc<FullClient>,
	tx_pool: Arc<TxPool>,
	task_manager: &TaskManager,
	block_time: Duration,
) {
	let spawn_handle = task_manager.spawn_handle();
	task_manager.spawn_essential_handle().spawn(
		"public-batch-aggregator",
		Some("public-batch"),
		run(aggregator, client, tx_pool, spawn_handle, block_time),
	);
}

async fn run(
	aggregator: Arc<Aggregator>,
	client: Arc<FullClient>,
	tx_pool: Arc<TxPool>,
	spawn_handle: SpawnTaskHandle,
	block_time: Duration,
) {
	log::info!(
		target: LOG_TARGET,
		"building public-batch prover ({} private batches x {} leaves); proving starts once it is ready",
		pallet_wormhole::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
		pallet_wormhole::circuit_config::NUM_LEAF_PROOFS,
	);
	let mut build = tokio::task::spawn_blocking(build_prover).fuse();
	let mut imports = client.import_notification_stream().fuse();
	let mut ticker = tokio::time::interval(block_time);

	loop {
		futures::select! {
			built = build => {
				match built {
					Ok(Ok(prover)) => {
						log::info!(target: LOG_TARGET, "public-batch prover ready");
						aggregator.set_prover(prover);
					},
					Ok(Err(e)) => {
						log::error!(target: LOG_TARGET, "failed to build public-batch prover: {e}");
						return;
					},
					Err(e) => {
						log::error!(target: LOG_TARGET, "public-batch prover build task failed: {e}");
						return;
					},
				}
			},
			notification = imports.next() => {
				let Some(notification) = notification else {
					log::warn!(target: LOG_TARGET, "block import stream ended");
					return;
				};
				if notification.is_new_best {
					on_new_best(&aggregator, &client, notification.hash);
				}
			},
			_ = ticker.tick().fuse() => {},
		}
		maybe_prove(&aggregator, &client, &tx_pool, &spawn_handle);
	}
}

fn build_prover() -> Result<PublicBatchProver, String> {
	PublicBatchProver::new_from_bytes(
		pallet_wormhole::embedded_artifacts::PRIVATE_BATCH_COMMON,
		pallet_wormhole::embedded_artifacts::PRIVATE_BATCH_VERIFIER,
		pallet_wormhole::embedded_artifacts::DUMMY_PRIVATE_BATCH_PROOF,
		(
			pallet_wormhole::circuit_config::NUM_LEAF_PROOFS,
			pallet_wormhole::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
		),
	)
	.map_err(|e| format!("{e:#}"))
}

/// Settle-driven, expiry and canonicality eviction for one new best block.
fn on_new_best(aggregator: &Aggregator, client: &FullClient, hash: H256) {
	let settled = settled_nullifiers(client, hash);
	let mut pool = aggregator.pool.lock();

	let evicted_settled = if settled.is_empty() { 0 } else { pool.evict_settled(&settled) };
	let evicted_expired = pool.evict_older_than(aggregator.config.proof_expiry);

	let mut evicted_noncanonical = 0;
	for stats in pool.bucket_stats() {
		if !bucket_block_is_canonical(client, &stats.key) {
			evicted_noncanonical += pool.remove_bucket(&stats.key).len();
		}
	}

	if evicted_settled + evicted_expired + evicted_noncanonical > 0 {
		log::debug!(
			target: LOG_TARGET,
			"evicted {evicted_settled} settled, {evicted_expired} expired, {evicted_noncanonical} non-canonical proofs; {} pooled",
			pool.len()
		);
	}
	if let Some(m) = &aggregator.metrics {
		m.evicted_settled.inc_by(evicted_settled as u64);
		m.evicted_expired.inc_by(evicted_expired as u64);
		m.evicted_noncanonical.inc_by(evicted_noncanonical as u64);
	}
	aggregator.update_pool_gauges(&pool);
}

/// Nullifiers settled by `ProofVerified` events in block `hash`.
fn settled_nullifiers(client: &FullClient, hash: H256) -> HashSet<BytesDigest> {
	let key = StorageKey(storage_prefix(b"System", b"Events").to_vec());
	let raw = match client.storage(hash, &key) {
		Ok(Some(data)) => data.0,
		Ok(None) => return HashSet::new(),
		Err(e) => {
			log::warn!(target: LOG_TARGET, "failed to read events of {hash:?}: {e}");
			return HashSet::new();
		},
	};
	let records = match Vec::<frame_system::EventRecord<RuntimeEvent, H256>>::decode(&mut &raw[..])
	{
		Ok(records) => records,
		Err(e) => {
			log::warn!(target: LOG_TARGET, "failed to decode events of {hash:?}: {e}");
			return HashSet::new();
		},
	};
	records
		.into_iter()
		.filter_map(|record| match record.event {
			RuntimeEvent::Wormhole(pallet_wormhole::Event::ProofVerified {
				nullifiers, ..
			}) => Some(nullifiers),
			_ => None,
		})
		.flatten()
		.map(BytesDigest::new_unchecked)
		.collect()
}

/// Whether the bucket's proving block is still on the canonical chain.
fn bucket_block_is_canonical(client: &FullClient, key: &BatchKey) -> bool {
	let block_hash = H256::from(*key.block_hash);
	match client.number(block_hash) {
		Ok(Some(number)) => matches!(client.hash(number), Ok(Some(h)) if h == block_hash),
		// Unknown to this node: leave it alone rather than drop custody.
		_ => true,
	}
}

/// Whether `nullifier` is settled in state at `at`.
pub(super) fn nullifier_used(
	client: &FullClient,
	at: H256,
	nullifier: &BytesDigest,
) -> Result<bool, String> {
	let key = pallet_wormhole::UsedNullifiers::<Runtime>::hashed_key_for(**nullifier);
	client
		.storage(at, &StorageKey(key))
		.map(|value| value.is_some())
		.map_err(|e| format!("UsedNullifiers read failed: {e}"))
}

/// Whether a bucket should be proved now under `cfg`.
fn eligible(stats: &BucketStats, cfg: &AggregatorConfig) -> bool {
	if stats.is_dummy() {
		return false;
	}
	let in_flight = stats.last_snapshot_age.is_some_and(|age| age < cfg.reprove_after);
	if in_flight {
		return false;
	}
	stats.is_full() || (stats.num_proofs >= cfg.min_proofs && stats.oldest_age >= cfg.max_wait)
}

/// Pick the most valuable eligible bucket and prove it on a blocking task.
fn maybe_prove(
	aggregator: &Arc<Aggregator>,
	client: &Arc<FullClient>,
	tx_pool: &Arc<TxPool>,
	spawn_handle: &SpawnTaskHandle,
) {
	let Some(prover) = aggregator.prover() else { return };
	if !aggregator.try_begin_proving() {
		return;
	}

	let snapshot = {
		let mut pool = aggregator.pool.lock();
		let candidate = pool
			.bucket_stats()
			.into_iter()
			.filter(|stats| eligible(stats, &aggregator.config))
			.max_by_key(|stats| (stats.is_full(), stats.total_volume, stats.oldest_age));
		candidate.and_then(|stats| pool.snapshot_batch(&stats.key).map(|proofs| (stats, proofs)))
	};
	let Some((stats, proofs)) = snapshot else {
		aggregator.end_proving();
		return;
	};

	log::info!(
		target: LOG_TARGET,
		"proving public batch: {} private batches (bucket holds {}), block 0x{}",
		proofs.len(),
		stats.num_proofs,
		hex::encode(&stats.key.block_hash[..4]),
	);

	let aggregator = aggregator.clone();
	let client = client.clone();
	let tx_pool = tx_pool.clone();
	spawn_handle.spawn("public-batch-prove", Some("public-batch"), async move {
		prove_and_submit(&aggregator, &client, &tx_pool, prover, proofs).await;
		aggregator.end_proving();
	});
}

async fn prove_and_submit(
	aggregator: &Aggregator,
	client: &FullClient,
	tx_pool: &TxPool,
	prover: Arc<PublicBatchProver>,
	proofs: Vec<Proof>,
) {
	let segments: Vec<Vec<BytesDigest>> = proofs
		.iter()
		.filter_map(|proof| parse_private_batch_inputs(proof).ok())
		.map(|inputs| inputs.nullifiers)
		.collect();

	let aggregator_address = aggregator.aggregator_digest;
	let started = std::time::Instant::now();
	let proved = tokio::task::spawn_blocking(move || {
		prover.prove_batch(PublicBatchInputs { proofs, aggregator_address })
	})
	.await;
	let proof = match proved {
		Ok(Ok(proof)) => proof,
		Ok(Err(e)) => {
			log::warn!(target: LOG_TARGET, "public-batch proving failed: {e:#}");
			if let Some(m) = &aggregator.metrics {
				m.prove_failures.inc();
			}
			return;
		},
		Err(e) => {
			log::warn!(target: LOG_TARGET, "public-batch proving task failed: {e}");
			if let Some(m) = &aggregator.metrics {
				m.prove_failures.inc();
			}
			return;
		},
	};
	if let Some(m) = &aggregator.metrics {
		m.batches_proved.inc();
	}
	log::info!(
		target: LOG_TARGET,
		"public batch proved in {:.1}s",
		started.elapsed().as_secs_f64()
	);

	// Segments that settled while we were proving are denied on-chain; a batch
	// with none left would be rejected at block inclusion, so don't submit it.
	let best = client.info().best_hash;
	let live_segments = segments
		.iter()
		.filter(|nullifiers| {
			!nullifiers.iter().any(|n| nullifier_used(client, best, n).unwrap_or(false))
		})
		.count();
	if live_segments == 0 {
		log::info!(target: LOG_TARGET, "every segment settled during proving; discarding batch");
		return;
	}

	let call = RuntimeCall::Wormhole(pallet_wormhole::Call::verify_public_batch {
		proof_bytes: proof.to_bytes(),
	});
	let xt = OpaqueExtrinsic::from(UncheckedExtrinsic::new_bare(call));
	match tx_pool.submit_one(best, TransactionSource::Local, xt).await {
		Ok(hash) => {
			log::info!(
				target: LOG_TARGET,
				"submitted public batch {hash:?} ({live_segments}/{} live segments)",
				segments.len()
			);
			if let Some(m) = &aggregator.metrics {
				m.batches_submitted.inc();
			}
		},
		Err(e) => {
			log::warn!(target: LOG_TARGET, "public-batch submission rejected: {e}");
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use qp_wormhole_aggregator::pool::PoolLimits;
	use sp_core::crypto::AccountId32;

	fn config() -> AggregatorConfig {
		AggregatorConfig {
			aggregator_address: AccountId32::new([1u8; 32]),
			min_proofs: 3,
			max_wait: Duration::from_secs(60),
			reprove_after: Duration::from_secs(120),
			proof_expiry: Duration::from_secs(3600),
			pool_limits: PoolLimits::default(),
		}
	}

	fn stats(num_proofs: usize, oldest_age: Duration) -> BucketStats {
		BucketStats {
			key: BatchKey {
				block_hash: BytesDigest::new_unchecked([7u8; 32]),
				asset_id: 0,
				volume_fee_bps: 4,
			},
			num_proofs,
			batch_size: 53,
			oldest_age,
			total_volume: 0,
			last_snapshot_age: None,
		}
	}

	#[test]
	fn full_bucket_is_proved_immediately() {
		assert!(eligible(&stats(53, Duration::ZERO), &config()));
	}

	#[test]
	fn partial_bucket_waits_for_max_wait() {
		assert!(!eligible(&stats(3, Duration::from_secs(59)), &config()));
		assert!(eligible(&stats(3, Duration::from_secs(60)), &config()));
	}

	#[test]
	fn partial_bucket_needs_min_proofs() {
		assert!(!eligible(&stats(2, Duration::from_secs(600)), &config()));
	}

	#[test]
	fn recently_snapshot_bucket_is_left_alone() {
		let mut in_flight = stats(53, Duration::ZERO);
		in_flight.last_snapshot_age = Some(Duration::from_secs(10));
		assert!(!eligible(&in_flight, &config()));

		in_flight.last_snapshot_age = Some(Duration::from_secs(120));
		assert!(eligible(&in_flight, &config()));
	}

	#[test]
	fn dummy_bucket_is_never_proved() {
		let mut dummy = stats(53, Duration::ZERO);
		dummy.key.block_hash = BytesDigest::default();
		assert!(!eligible(&dummy, &config()));
	}
}
