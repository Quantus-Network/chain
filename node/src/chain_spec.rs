use quantus_runtime::{
	genesis_config_presets::{HEISENBERG_RUNTIME_PRESET, PLANCK_RUNTIME_PRESET},
	WASM_BINARY,
};
use sc_service::{ChainType, Properties};
use sc_telemetry::TelemetryEndpoints;
use serde_json::json;

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

pub fn development_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("DEV"));
	properties.insert("ss58Format".into(), json!(189));

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Quantus DevNet wasm not available".to_string())?,
		None,
	)
	.with_name("Quantus DevNet")
	.with_id("dev")
	.with_protocol_id("quantus-devnet")
	.with_chain_type(ChainType::Development)
	.with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}

/// Integration environment chain spec - internal use only.
pub fn heisenberg_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("HEI"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![(
		"/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(),
		0,
	)])
	.expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1-p2p-heisenberg.quantus.cat/tcp/30333/p2p/Qmdts9fu3NCMFnvLdD1dHAHFer8EPzVDXxVnyPxRKA3Gkt"
			.parse()
			.unwrap(),
		"/dns/a2-p2p-heisenberg.quantus.cat/tcp/30333/p2p/QmcKHndoiNRdiT6iVp6ugj8bNse5Vd5WmCoE9YWn9kNaTM"
			.parse()
			.unwrap(),
	];

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Runtime wasm not available".to_string())?,
		None,
	)
	.with_name("Heisenberg")
	.with_id("heisenberg")
	.with_protocol_id("heisenberg")
	.with_boot_nodes(boot_nodes)
	.with_telemetry_endpoints(telemetry_endpoints)
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(HEISENBERG_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}

/// Planck network - same genesis as heisenberg (root and treasury).
pub fn planck_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("PLK"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![(
		"/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(),
		0,
	)])
	.expect("Telemetry endpoints config is valid; qed");

	// Boot nodes: empty for new network, add when available
	let boot_nodes = vec![];

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Runtime wasm not available".to_string())?,
		None,
	)
	.with_name("Planck")
	.with_id("planck")
	.with_protocol_id("planck")
	.with_boot_nodes(boot_nodes)
	.with_telemetry_endpoints(telemetry_endpoints)
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(PLANCK_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}
