use futures::StreamExt;
use primitive_types::U512;
use prometheus::{GaugeVec, Opts, Registry};
use quantus_runtime::opaque::Block;
use sc_client_api::BlockchainEvents;
use sp_api::ProvideRuntimeApi;
use sp_consensus_qpow::QPoWApi;
use std::sync::Arc;

pub struct ResonanceBusinessMetrics;

impl ResonanceBusinessMetrics {
	/// Pack a U512 into an f64 by taking the highest-order 64 bits (8 bytes).
	fn pack_u512_to_f64(value: U512) -> f64 {
		// Convert U512 to big-endian bytes (64 bytes)
		let bytes = value.to_big_endian();

		// Take the highest-order 8 bytes (first 8 bytes in big-endian)
		let mut highest_8_bytes = [0u8; 8];
		highest_8_bytes.copy_from_slice(&bytes[0..8]);

		// Convert to u64
		let highest_64_bits = u64::from_be_bytes(highest_8_bytes);

		// Cast to f64
		highest_64_bits as f64
	}

	/// Register QPoW metrics gauge vector with Prometheus
	pub fn register_gauge_vec(registry: &Registry) -> GaugeVec {
		let gauge_vec =
			GaugeVec::new(Opts::new("qpow_metrics", "QPOW Metrics"), &["data_group"]).unwrap();

		registry.register(Box::new(gauge_vec.clone())).unwrap();
		gauge_vec
	}

	/// Update QPoW metrics in Prometheus for a given block
	pub fn update_qpow_metrics<C>(
		client: &C,
		block_hash: <Block as sp_runtime::traits::Block>::Hash,
		gauge: &GaugeVec,
	) where
		C: ProvideRuntimeApi<Block>,
		C::Api: sp_consensus_qpow::QPoWApi<Block>,
	{
		// Get values via the runtime API - we'll handle potential errors gracefully
		let block_time_sum =
			client.runtime_api().get_block_time_sum(block_hash).unwrap_or_else(|e| {
				log::warn!("Failed to get median_block_time: {:?}", e);
				0
			});

		let median_block_time =
			client.runtime_api().get_median_block_time(block_hash).unwrap_or_else(|e| {
				log::warn!("Failed to get median_block_time: {:?}", e);
				0
			});

		let distance_threshold =
			client.runtime_api().get_distance_threshold(block_hash).unwrap_or_else(|e| {
				log::warn!("Failed to get distance_threshold: {:?}", e);
				U512::zero()
			});

		let last_block_time =
			client.runtime_api().get_last_block_time(block_hash).unwrap_or_else(|e| {
				log::warn!("Failed to get last_block_time: {:?}", e);
				0
			});

		let last_block_duration =
			client.runtime_api().get_last_block_duration(block_hash).unwrap_or_else(|e| {
				log::warn!("Failed to get last_block_duration: {:?}", e);
				0
			});

		let chain_height = client.runtime_api().get_chain_height(block_hash).unwrap_or_else(|e| {
			log::warn!("Failed to get chain_height: {:?}", e);
			0
		});

		let difficulty = client.runtime_api().get_difficulty(block_hash).unwrap_or_else(|e| {
			log::warn!("Failed to get difficulty: {:?}", e);
			U512::zero()
		});

		// Update the metrics with the values we retrieved
		gauge.with_label_values(&["chain_height"]).set(chain_height as f64);
		gauge.with_label_values(&["block_time_sum"]).set(block_time_sum as f64);
		gauge.with_label_values(&["median_block_time"]).set(median_block_time as f64);
		gauge
			.with_label_values(&["distance_threshold"])
			.set(Self::pack_u512_to_f64(distance_threshold));
		gauge.with_label_values(&["difficulty"]).set(difficulty.low_u64() as f64);
		gauge.with_label_values(&["last_block_time"]).set(last_block_time as f64);
		gauge
			.with_label_values(&["last_block_duration"])
			.set(last_block_duration as f64);
	}

	/// Start a monitoring task for QPoW metrics
	pub fn start_monitoring_task<C>(
		client: Arc<C>,
		prometheus_registry: Option<Registry>,
		task_manager: &sc_service::TaskManager,
	) where
		C: ProvideRuntimeApi<Block> + BlockchainEvents<Block> + Send + Sync + 'static,
		C::Api: sp_consensus_qpow::QPoWApi<Block>,
	{
		// Get or create the gauge vector from the registry
		let prometheus_gauge_vec = prometheus_registry.as_ref().map(Self::register_gauge_vec);

		// Spawn the monitoring task
		task_manager
			.spawn_essential_handle()
			.spawn("monitoring_qpow", None, async move {
				log::info!("⚙️  QPoW Monitoring task spawned");

				let mut sub = client.import_notification_stream();
				while let Some(notification) = sub.next().await {
					let block_hash = notification.hash;
					if let Some(ref gauge) = prometheus_gauge_vec {
						Self::update_qpow_metrics(&*client, block_hash, gauge);
					} else {
						log::warn!("QPoW Monitoring: Prometheus registry not found");
					}
				}
			});
	}
}
