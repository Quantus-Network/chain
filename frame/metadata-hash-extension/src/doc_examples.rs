// Documentation examples for `CheckMetadataHash` integration.
//
// This file is embedded into crate docs via `docify` and is not compiled as a module.

#[docify::export]
mod add_metadata_hash_extension {
	frame_support::construct_runtime! {
		pub enum Runtime {
			System: frame_system,
		}
	}

	/// The `TransactionExtension` to the basic transaction logic.
	pub type TxExtension = (
		frame_system::AuthorizeCall<Runtime>,
		frame_system::CheckNonZeroSender<Runtime>,
		frame_system::CheckSpecVersion<Runtime>,
		frame_system::CheckTxVersion<Runtime>,
		frame_system::CheckGenesis<Runtime>,
		frame_system::CheckMortality<Runtime>,
		frame_system::CheckNonce<Runtime>,
		frame_system::CheckWeight<Runtime>,
		// Add the `CheckMetadataHash` extension.
		// The position in this list is not important, so we could also add it to beginning.
		frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
		frame_system::WeightReclaim<Runtime>,
	);

	/// In your runtime this will be your real address type.
	type Address = ();
	/// In your runtime this will be your real signature type.
	type Signature = ();

	/// Unchecked extrinsic type as expected by this runtime.
	pub type UncheckedExtrinsic =
		sp_runtime::generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;
}

#[docify::export]
fn enable_metadata_hash_in_wasm_builder() {
	substrate_wasm_builder::WasmBuilder::init_with_defaults()
		// Requires the `metadata-hash` feature to be activated.
		// You need to pass the main token symbol and its number of decimals.
		.enable_metadata_hash("TOKEN", 12)
		// The runtime will be build twice and the second time the `RUNTIME_METADATA_HASH`
		// environment variable will be set for the `CheckMetadataHash` extension.
		.build()
}
