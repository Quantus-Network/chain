use quantus_runtime::{
	genesis_config_presets::{
		HEISENBERG_RUNTIME_PRESET, LIVE_TESTNET_RUNTIME_PRESET, SCHRODINGER_RUNTIME_PRESET,
	},
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
/// Configure a new chain spec for the live testnet.
pub fn live_testnet_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("RES"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![
		("/dns/telemetry.res.fm/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
		("/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
	])
	.expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1.t.res.fm/tcp/30201/p2p/QmYpbayBgKbhfHGn2kNWWhh3DHwBnPaLDMYvaGmT78oAP7"
			.parse()
			.unwrap(),
		"/dns/a2.t.res.fm/tcp/30203/p2p/QmeN9H9CBdBESd6wib9xetPiYsCLYPTAJn8sxajWi2Bjkb"
			.parse()
			.unwrap(),
		"/dns/a3.t.res.fm/tcp/30202/p2p/QmQLf3wj7KqqtTjtrq7iQZY5JokQ3k7HHLGe5hNvHSxnFr"
			.parse()
			.unwrap(),
	];

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Resonance wasm not available".to_string())?,
		None,
	)
	.with_name("Resonance")
	.with_id("resonance")
	.with_protocol_id("resonance")
	.with_boot_nodes(boot_nodes)
	.with_telemetry_endpoints(telemetry_endpoints)
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(LIVE_TESTNET_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}

/// Integration environment chain spec - internal use only.
pub fn heisenberg_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("HEI"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![
		("/dns/telemetry.res.fm/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
		("/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
	])
	.expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1.i.res.fm/tcp/30104/p2p/QmQ5UgQyHs3UiGyvruYxXTmfgrJnciobqiUe5peXZqnjvq"
			.parse()
			.unwrap(),
		"/dns/a2.i.res.fm/tcp/30105/p2p/QmWwvxdtnaej2qxn4yqS2z8S1TrVfrxz1zjJ8f8Y8LK8Wq"
			.parse()
			.unwrap(),
		"/dns/a3.i.res.fm/tcp/30215/p2p/QmNZH7cLXAnNTeS6tw29KRmFYygFgL18GXjk6pXGHfRvAe"
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

/// Configure a new chain spec for the schrodinger testnet.
pub fn schrodinger_chain_spec() -> Result<ChainSpec, String> {
	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!(12));
	properties.insert("tokenSymbol".into(), json!("QU"));
	properties.insert("ss58Format".into(), json!(189));

	let telemetry_endpoints = TelemetryEndpoints::new(vec![
		("/dns/telemetry.res.fm/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
		("/dns/shard-telemetry.quantus.cat/tcp/443/x-parity-wss/%2Fsubmit%2F".to_string(), 0),
	])
	.expect("Telemetry endpoints config is valid; qed");

	let boot_nodes = vec![
		"/dns/a1.quantu.se/tcp/30205/p2p/QmdEk5BbraqEHwB6eG6sPkupkogsAKP9uFMFaAo6A6H3ZM"
			.parse()
			.unwrap(),
		"/dns/a2.quantu.se/tcp/30208/p2p/QmPUeRrypSFPA2AVdE2vW7N64DTx9hWfrgQaTqja45d9GL"
			.parse()
			.unwrap(),
		"/dns/a3.quantu.se/tcp/30102/p2p/QmVDQEK3NtJMgUhCNkfUbxvoKPcowkcu16cKCrqqemPbEq"
			.parse()
			.unwrap(),
	];

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Schrodinger wasm not available".to_string())?,
		None,
	)
	.with_name("Quantus Testnet")
	.with_id("schrodinger")
	.with_protocol_id("schrodinger")
	.with_boot_nodes(boot_nodes)
	.with_telemetry_endpoints(telemetry_endpoints)
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(SCHRODINGER_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}
