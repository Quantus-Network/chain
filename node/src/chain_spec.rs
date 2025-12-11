use quantus_runtime::{
	genesis_config_presets::{DIRAC_RUNTIME_PRESET, HEISENBERG_RUNTIME_PRESET},
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

/// Configure a new chain spec for the dirac testnet.
pub fn dirac_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("QU"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![(
		"/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(),
		0,
	)])
	.expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1-p2p-dirac.quantus.cat/tcp/30333/p2p/QmUpQe7KmRW9WiKamEJm1ocJjx38x5TECTtgcvvhAovebA"
			.parse()
			.unwrap(),
		"/dns/a2-p2p-dirac.quantus.cat/tcp/30333/p2p/QmV2q5givrE3Dxhu7Fv21MkZ5T3CdFWnmxt8ktweRYJ9AE"
			.parse()
			.unwrap(),
		"/ip4/72.61.118.55/tcp/30333/p2p/QmXkZyXejhpJ6FG9sPG2CYtpyvtpgADKLY5k24jY8DQYwh"
			.parse()
			.unwrap(),
	];

	Ok(ChainSpec::builder(WASM_BINARY.ok_or_else(|| "Dirac wasm not available".to_string())?, None)
		.with_name("Quantus Dirac Testnet")
		.with_id("dirac")
		.with_protocol_id("dirac")
		.with_boot_nodes(boot_nodes)
		.with_telemetry_endpoints(telemetry_endpoints)
		.with_chain_type(ChainType::Live)
		.with_genesis_config_preset_name(DIRAC_RUNTIME_PRESET)
		.with_properties(properties)
		.build())
}
