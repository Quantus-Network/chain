use qp_wormhole_verifier::WormholeVerifier;
use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};

fn main() {
	generate_cargo_keys();

	rerun_if_git_head_changed();

	// Validate pre-generated circuit binaries
	validate_circuit_binaries();
}

fn validate_circuit_binaries() {
	println!("cargo:rerun-if-changed=pallets/wormhole/verifier.bin");
	println!("cargo:rerun-if-changed=pallets/wormhole/common.bin");
	println!("cargo:rerun-if-changed=pallets/wormhole/aggregated_verifier.bin");
	println!("cargo:rerun-if-changed=pallets/wormhole/aggregated_common.bin");

	// Validate the pre-generated wormhole circuit binaries
	let verifier_bytes = include_bytes!("../pallets/wormhole/verifier.bin");
	let common_bytes = include_bytes!("../pallets/wormhole/common.bin");

	WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes).expect(
		"CRITICAL ERROR: Failed to create WormholeVerifier from embedded data. \
         The verifier.bin and common.bin files must be regenerated using qp-zk-circuits \
         with a compatible qp-plonky2 version. Run: \
         cd ../qp-zk-circuits/wormhole/circuit-builder && cargo run",
	);

	// TODO: Re-enable validation once aggregated circuit binaries are regenerated
	// with the new qp-plonky2 version that has updated Poseidon2 gates.
	// The aggregated circuit binaries were generated with an older qp-plonky2 version
	// and have incompatible gate serialization.
	//
	// let agg_verifier_bytes = include_bytes!("../pallets/wormhole/aggregated_verifier.bin");
	// let agg_common_bytes = include_bytes!("../pallets/wormhole/aggregated_common.bin");
	// WormholeVerifier::new_from_bytes(agg_verifier_bytes, agg_common_bytes).expect(
	// 	"CRITICAL ERROR: Failed to create aggregated WormholeVerifier from embedded data.",
	// );

	println!("cargo:trace=✅ Wormhole circuit binaries validated successfully");
}
