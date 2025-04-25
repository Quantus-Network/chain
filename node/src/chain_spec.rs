use resonance_runtime::WASM_BINARY;
use sc_service::{ChainType, Properties};
use serde_json::json;
use resonance_runtime::genesis_config_presets::LIVE_TESTNET_RUNTIME_PRESET;

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

pub fn development_chain_spec() -> Result<ChainSpec, String> {

	let mut properties = Properties::new();
	properties.insert("tokenDecimals".into(), json!("9"));
	properties.insert("tokenSymbol".into(), json!("REZ"));

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
	properties.insert("tokenDecimals".into(), json!("9"));
	properties.insert("tokenSymbol".into(), json!("REZ"));

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
	properties.insert("tokenSymbol".into(), json!("REZ"));

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Live testnet wasm not available".to_string())?,
		None,
	)
	.with_name("Resonance Testnet")
	.with_id("resonance_testnet")
	.with_protocol_id("resonance-testnet")
	.with_chain_type(ChainType::Live)
	.with_genesis_config_preset_name(LIVE_TESTNET_RUNTIME_PRESET)
	.with_properties(properties)
	.build())
}
