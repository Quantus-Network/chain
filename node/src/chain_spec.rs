use resonance_runtime::WASM_BINARY;
use sc_service::{ChainType, Properties};
use sc_telemetry::TelemetryEndpoints;
use serde_json::json;
use resonance_runtime::genesis_config_presets::LIVE_TESTNET_RUNTIME_PRESET;

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

pub fn development_chain_spec() -> Result<ChainSpec, String> {

	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(9));
	properties.insert("tokenSymbol".into(), json!("QUAN"));

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Development")
	.with_id("dev")
	.with_protocol_id("resonance-testnet")
	.with_chain_type(ChainType::Development)
	.with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}

pub fn local_chain_spec() -> Result<ChainSpec, String> {

	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(9));
	properties.insert("tokenSymbol".into(), json!("QUAN"));

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Local Testnet")
	.with_id("local_testnet")
	.with_protocol_id("resonance-testnet")
	.with_chain_type(ChainType::Local)
	.with_genesis_config_preset_name(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET)
		.with_properties(properties)
	.build())
}

/// Configure a new chain spec for the live testnet.
pub fn live_testnet_chain_spec() -> Result<ChainSpec, String> {

	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(9));
	properties.insert("tokenSymbol".into(), json!("QUAN"));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![
		("/dns/telemetry.res.fm/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0)
	]).expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1.t.res.fm/tcp/30201/p2p/12D3KooWGmDZ95J13cggsv56mSepAj3WiVPR3foqqh728umZrhPr".parse().unwrap(),
		"/dns/a2.t.res.fm/tcp/30203/p2p/12D3KooWPPv8nrVEN5mjcMruDnAEdcpfppSfSbij2A7FXWNGt8JL".parse().unwrap(),
		"/dns/a3.t.res.fm/tcp/30202/p2p/12D3KooWMpmEQmCB31Dz84YdnxL48aiSFQydEiq5MZv6VtZouXRd".parse().unwrap(),
	];

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Live testnet wasm not available".to_string())?,
		None,
	)
	.with_name("Resonance Testnet")
	.with_id("resonance_testnet")
	.with_protocol_id("resonance-testnet")
	.with_boot_nodes(boot_nodes)
	.with_telemetry_endpoints(telemetry_endpoints)
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(LIVE_TESTNET_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}
