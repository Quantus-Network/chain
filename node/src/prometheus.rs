use prometheus::{Registry, Opts, GaugeVec};


pub struct ResonanceBusinessMetrics;


impl ResonanceBusinessMetrics {
    pub fn register_gauge_vec(registry: &Registry) -> GaugeVec {
        let gauge_vec = GaugeVec::new(
            Opts::new("qpow_metrics", "QPOW Metrics"),
            &["data_group"]
        ).unwrap();

        registry.register(Box::new(gauge_vec.clone())).unwrap();
        gauge_vec
    }
}