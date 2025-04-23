use resonance_runtime::WASM_BINARY;
use sc_service::ChainType;
use resonance_runtime::genesis_config_presets::LIVE_TESTNET_RUNTIME_PRESET;

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

pub fn development_chain_spec() -> Result<ChainSpec, String> {

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Development")
	.with_id("dev")
	.with_chain_type(ChainType::Development)
	.with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
	.build())
}

pub fn local_chain_spec() -> Result<ChainSpec, String> {

	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Local Testnet")
	.with_id("local_testnet")
	.with_chain_type(ChainType::Local)
	.with_genesis_config_preset_name(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET)
	.build())
}

/// Configure a new chain spec for the live testnet.
pub fn live_testnet_chain_spec() -> Result<ChainSpec, String> {
	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Live testnet wasm not available".to_string())?,
		None,
	)
	.with_name("Resonance Testnet") // Your desired name
	.with_id("resonance_testnet")   // Your desired ID
	.with_chain_type(ChainType::Live) // Set chain type to Live
	// Use the genesis preset we defined in runtime/src/genesis_config_presets.rs
	.with_genesis_config_preset_name(LIVE_TESTNET_RUNTIME_PRESET)
	.build())
}
