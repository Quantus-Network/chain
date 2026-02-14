use qp_wormhole_verifier::WormholeVerifier;
use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};

fn main() {
	generate_cargo_keys();

	rerun_if_git_head_changed();

	// Validate pre-generated circuit binaries
	validate_circuit_binaries();
}

fn validate_circuit_binaries() {
	// NOTE: cargo:rerun-if-changed paths are relative to the package's Cargo.toml directory.
	// Since this build.rs is in node/, we use ../pallets/wormhole/ to point to the pallet directory.
	println!("cargo:rerun-if-changed=../pallets/wormhole/aggregated_verifier.bin");
	println!("cargo:rerun-if-changed=../pallets/wormhole/aggregated_common.bin");

	// Validate the aggregated circuit binaries
	let agg_verifier_bytes = include_bytes!("../pallets/wormhole/aggregated_verifier.bin");
	let agg_common_bytes = include_bytes!("../pallets/wormhole/aggregated_common.bin");
	WormholeVerifier::new_from_bytes(agg_verifier_bytes, agg_common_bytes).expect(
		"CRITICAL ERROR: Failed to create aggregated WormholeVerifier from embedded data. \
         The aggregated_verifier.bin and aggregated_common.bin files must be regenerated \
         using qp-zk-circuits. Run: quantus developer build-circuits",
	);

	println!("cargo:trace=✅ Wormhole circuit binaries validated successfully");
}
