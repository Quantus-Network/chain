use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};
use wormhole_verifier::WormholeVerifier;

fn main() {
    generate_cargo_keys();

    rerun_if_git_head_changed();

    // Circuit validation and generation
    validate_and_generate_circuits();
}

fn validate_and_generate_circuits() {
    println!("cargo:rerun-if-changed=pallets/wormhole/verifier.bin");
    println!("cargo:rerun-if-changed=pallets/wormhole/common.bin");

    // Validate existing binaries
    let verifier_bytes = include_bytes!("../pallets/wormhole/verifier.bin");
    let common_bytes = include_bytes!("../pallets/wormhole/common.bin");

    // This will fail at build time if the binaries are invalid
    WormholeVerifier::new_from_bytes(verifier_bytes, common_bytes)
        .expect("CRITICAL ERROR: Failed to create WormholeVerifier from embedded data. Check verifier.bin and common.bin");

    println!("cargo:warning=✅ Wormhole circuit binaries validated successfully");
}
