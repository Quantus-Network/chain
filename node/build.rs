use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};
use wormhole_verifier::WormholeVerifier;

fn main() {
    generate_cargo_keys();

    rerun_if_git_head_changed();

    // Circuit validation and generation
    validate_and_generate_circuits();
}

fn validate_and_generate_circuits() {
    // println!("cargo:rerun-if-changed=pallets/wormhole/verifier.bin");
    // println!("cargo:rerun-if-changed=pallets/wormhole/common.bin");

    // Generate circuit binaries from the zk-circuits dependency
    generate_circuit_binaries();

    // Validate generated binaries
    let verifier_bytes = include_bytes!("../pallets/wormhole/verifier.bin");
    let common_bytes = include_bytes!("../pallets/wormhole/common.bin");

    // This will fail at build time if the binaries are invalid
    WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes)
        .expect("CRITICAL ERROR: Failed to create WormholeVerifier from embedded data. Check verifier.bin and common.bin");

    println!("cargo:warning=✅ Wormhole circuit binaries generated and validated successfully");
}

fn generate_circuit_binaries() {
    println!("cargo:warning=🔧 Generating wormhole circuit binaries from zk-circuits...");

    // Call the circuit-builder to generate binaries directly in the pallet directory
    // We don't need the prover binary for the chain, only verifier and common
    circuit_builder::generate_circuit_binaries("../pallets/wormhole", false)
        .expect("Failed to generate circuit binaries");

    println!("cargo:warning=✅ Circuit binaries generated successfully");
}
