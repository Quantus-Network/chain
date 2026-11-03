use prometheus::{IntCounter, IntGauge, Registry};

/// Prometheus metrics for the public-batch aggregator.
pub struct Metrics {
	pub pooled_proofs: IntGauge,
	pub buckets: IntGauge,
	pub prover_ready: IntGauge,
	pub claimed: IntCounter,
	pub declined: IntCounter,
	pub evicted_settled: IntCounter,
	pub evicted_expired: IntCounter,
	pub evicted_noncanonical: IntCounter,
	pub batches_proved: IntCounter,
	pub batches_submitted: IntCounter,
	pub prove_failures: IntCounter,
}

impl Metrics {
	pub fn register(registry: &Registry) -> Result<Self, prometheus::Error> {
		let gauge = |name: &str, help: &str| -> Result<IntGauge, prometheus::Error> {
			let g = IntGauge::new(name, help)?;
			registry.register(Box::new(g.clone()))?;
			Ok(g)
		};
		let counter = |name: &str, help: &str| -> Result<IntCounter, prometheus::Error> {
			let c = IntCounter::new(name, help)?;
			registry.register(Box::new(c.clone()))?;
			Ok(c)
		};
		Ok(Self {
			pooled_proofs: gauge(
				"quantus_public_batch_pooled_proofs",
				"Private-batch proofs currently held by the public-batch aggregator",
			)?,
			buckets: gauge(
				"quantus_public_batch_buckets",
				"Distinct (block, asset, fee) buckets in the aggregator pool",
			)?,
			prover_ready: gauge(
				"quantus_public_batch_prover_ready",
				"1 once the public-batch prover has finished building its circuit",
			)?,
			claimed: counter(
				"quantus_public_batch_claimed_total",
				"Private-batch submissions claimed from the transaction pool",
			)?,
			declined: counter(
				"quantus_public_batch_declined_total",
				"Private-batch submissions that fell through to the transaction pool",
			)?,
			evicted_settled: counter(
				"quantus_public_batch_evicted_settled_total",
				"Pooled proofs evicted because a nullifier settled on-chain",
			)?,
			evicted_expired: counter(
				"quantus_public_batch_evicted_expired_total",
				"Pooled proofs evicted by the expiry policy",
			)?,
			evicted_noncanonical: counter(
				"quantus_public_batch_evicted_noncanonical_total",
				"Pooled proofs dropped because their block left the canonical chain",
			)?,
			batches_proved: counter(
				"quantus_public_batch_proved_total",
				"Public-batch proofs produced",
			)?,
			batches_submitted: counter(
				"quantus_public_batch_submitted_total",
				"Public-batch proofs submitted to the transaction pool",
			)?,
			prove_failures: counter(
				"quantus_public_batch_prove_failures_total",
				"Public-batch proving attempts that failed",
			)?,
		})
	}
}
