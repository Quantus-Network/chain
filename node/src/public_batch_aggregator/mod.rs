//! Opt-in public-batch aggregation (`--public-batch-aggregator`).
//!
//! A node running with this flag takes custody of private-batch wormhole proofs
//! offered to its transaction pool (from gossip and from local RPC), holds them
//! in a [`ProofPool`], and periodically folds a bucket of them into one
//! public-batch proof that it submits as a regular unsigned extrinsic. The
//! aggregator rebate in `pallet_wormhole::verify_public_batch` is paid to
//! [`AggregatorConfig::aggregator_address`].
//!
//! # Admission
//!
//! Pool admission for unsigned wormhole proofs is deliberately verify-free and
//! reads no per-nullifier state (see `ValidateUnsigned` in `pallet_wormhole`),
//! so "it was offered to the pool" guarantees nothing. Before a private batch is
//! claimed it must pass, in order:
//!
//! 1. the runtime's own `validate_transaction` (structural checks, canonical encoding, asset / fee
//!    / block binding, mintability);
//! 2. a `UsedNullifiers` read for every nullifier at the submission block — valid proofs from chain
//!    history would otherwise occupy pool slots that nothing could ever evict;
//! 3. [`ProofPool::push`]: full ZK verification (budgeted), pool-wide nullifier dedupe, capacity
//!    limits.
//!
//! Anything that fails falls through to the normal transaction pool, so an
//! honest proof the aggregator cannot batch still settles as a private batch.
//!
//! # Custody and eviction
//!
//! A claimed proof may exist nowhere else in the network, so proving never
//! removes it from the pool ([`ProofPool::snapshot_batch`] clones). Proofs
//! leave the pool when their nullifiers settle on-chain (`ProofVerified`
//! events on every new best block), when their block falls off the canonical
//! chain, or when they outlive [`AggregatorConfig::proof_expiry`].
//!
//! Submitters that watch their extrinsic (`author_submitAndWatchExtrinsic`,
//! which is what subxt and quantus-cli use) get a status stream that follows
//! the proof's nullifiers rather than the extrinsic itself: `Ready` on claim,
//! `InBlock` / `Finalized` for the extrinsic that settled them, `Dropped` if
//! the proof is evicted unsettled (see [`watchers`]).
//!
//! # Policy
//!
//! One public batch is proved at a time. A bucket is eligible once it is full,
//! or once it holds at least `min_proofs` and its oldest proof has waited
//! `max_wait`. A bucket whose batch was already snapshot for proving is left
//! alone for `reprove_after`, giving the submitted batch time to settle.

mod interceptor;
mod metrics;
mod watchers;
mod worker;

pub use interceptor::PrivateBatchInterceptor;
pub use worker::spawn;

use std::{
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	time::Duration,
};

use parking_lot::{Mutex, RwLock};
use plonky2::plonk::circuit_data::CommonCircuitData;
use qp_wormhole_aggregator::{
	common::utils::load_verifier_data_from_bytes,
	pool::{PoolLimits, ProofPool},
	public_batch::prover::lib::PublicBatchProver,
};
use qp_wormhole_inputs::BytesDigest;
use qp_zk_circuits_common::circuit::{D, F};
use sp_core::crypto::AccountId32;

use self::{metrics::Metrics, watchers::Watchers};

pub const LOG_TARGET: &str = "public-batch";

/// Operator configuration for the aggregator.
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
	/// Account credited with the public-batch rebate.
	pub aggregator_address: AccountId32,
	/// Smallest bucket worth proving once `max_wait` has elapsed.
	pub min_proofs: usize,
	/// How long the oldest proof of a partial bucket waits before it is proved.
	pub max_wait: Duration,
	/// How long to leave a bucket alone after snapshotting it for proving.
	pub reprove_after: Duration,
	/// Age after which a pooled proof is dropped regardless of settlement.
	pub proof_expiry: Duration,
	/// Capacity limits of the proof pool.
	pub pool_limits: PoolLimits,
}

/// State shared between the interceptor and the worker.
pub struct Aggregator {
	config: AggregatorConfig,
	aggregator_digest: BytesDigest,
	pool: Mutex<ProofPool>,
	/// Status streams of watched submissions that were claimed into `pool`.
	watchers: Mutex<Watchers>,
	/// Set once the background circuit build finishes.
	prover: RwLock<Option<Arc<PublicBatchProver>>>,
	proving: AtomicBool,
	private_batch_common: CommonCircuitData<F, D>,
	metrics: Option<Metrics>,
}

impl Aggregator {
	/// Build the pool from the private-batch verifier embedded in `pallet_wormhole`.
	///
	/// Cheap: the public-batch prover is built separately by
	/// [`worker::spawn`] because constructing its circuit takes tens of
	/// seconds.
	pub fn new(
		config: AggregatorConfig,
		registry: Option<&prometheus::Registry>,
	) -> Result<Arc<Self>, String> {
		let aggregator_digest =
			BytesDigest::try_from(<[u8; 32]>::from(config.aggregator_address.clone())).map_err(
				|e| format!("aggregator address cannot be encoded as circuit field elements: {e}"),
			)?;

		let verifier = load_verifier_data_from_bytes(
			pallet_wormhole::embedded_artifacts::PRIVATE_BATCH_COMMON,
			pallet_wormhole::embedded_artifacts::PRIVATE_BATCH_VERIFIER,
			"private_batch",
		)
		.map_err(|e| format!("failed to load embedded private-batch verifier: {e}"))?;
		let private_batch_common = verifier.common.clone();

		let pool = ProofPool::new(
			verifier,
			pallet_wormhole::circuit_config::NUM_LEAF_PROOFS,
			pallet_wormhole::circuit_config::NUM_PRIVATE_BATCH_PROOFS,
			config.pool_limits,
		)
		.map_err(|e| format!("failed to create proof pool: {e}"))?;

		let metrics = match registry {
			Some(registry) => Some(
				Metrics::register(registry)
					.map_err(|e| format!("failed to register aggregator metrics: {e}"))?,
			),
			None => None,
		};

		Ok(Arc::new(Self {
			config,
			aggregator_digest,
			pool: Mutex::new(pool),
			watchers: Mutex::new(Watchers::default()),
			prover: RwLock::new(None),
			proving: AtomicBool::new(false),
			private_batch_common,
			metrics,
		}))
	}

	fn prover(&self) -> Option<Arc<PublicBatchProver>> {
		self.prover.read().clone()
	}

	fn set_prover(&self, prover: PublicBatchProver) {
		*self.prover.write() = Some(Arc::new(prover));
		if let Some(m) = &self.metrics {
			m.prover_ready.set(1);
		}
	}

	/// Mark a proving run as started; `false` if one is already in flight.
	fn try_begin_proving(&self) -> bool {
		self.proving
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_ok()
	}

	fn end_proving(&self) {
		self.proving.store(false, Ordering::Release);
	}

	fn update_pool_gauges(&self, pool: &ProofPool) {
		if let Some(m) = &self.metrics {
			m.pooled_proofs.set(pool.len() as i64);
			m.buckets.set(pool.num_buckets() as i64);
		}
	}
}
