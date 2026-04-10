//! Build script for pallet-wormhole.
//!
//! Generates circuit binaries (aggregated_verifier.bin, aggregated_common.bin) at build time.
//! This ensures the binaries are always consistent with the circuit crate version and
//! eliminates the need to commit large binary files to the repository.
//!
//! Set `SKIP_CIRCUIT_BUILD=1` to skip circuit generation (useful for CI jobs
//! that don't need the circuits, like clippy/doc checks).

use std::{env, path::Path, time::Instant};

/// Compute Poseidon2 hash of bytes and return hex string
fn poseidon_hex(data: &[u8]) -> String {
	let hash = qp_poseidon_core::hash_bytes(data);
	hex::encode(&hash[..16]) // first 16 bytes for shorter display
}

/// Print hash of a generated binary file
fn print_bin_hash(dir: &Path, filename: &str) {
	let path = dir.join(filename);
	if let Ok(data) = std::fs::read(&path) {
		println!(
			"cargo:warning=  {}: {} bytes, hash: {}",
			filename,
			data.len(),
			poseidon_hex(&data)
		);
	}
}

fn main() {
	// Allow skipping circuit generation for CI jobs that don't need it
	if env::var("SKIP_CIRCUIT_BUILD").is_ok() {
		println!("cargo:warning=[pallet-wormhole] Skipping circuit generation (SKIP_CIRCUIT_BUILD is set)");
		return;
	}

	let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
	let num_leaf_proofs: usize = env::var("QP_NUM_LEAF_PROOFS")
		.unwrap_or_else(|_| "16".to_string())
		.parse()
		.expect("QP_NUM_LEAF_PROOFS must be a valid usize");

	// cargo:warning= messages are shown during build when the script runs
	println!(
		"cargo:warning=[pallet-wormhole] Generating ZK circuit binaries (num_leaf_proofs={})...",
		num_leaf_proofs
	);

	let start = Instant::now();

	// Generate all circuit binaries (leaf + layer-0 aggregated, no prover, no layer-1)
	qp_wormhole_circuit_builder::generate_all_circuit_binaries(
		Path::new(&out_dir),
		false, // include_prover = false
		num_leaf_proofs,
		None, // num_layer0_proofs - no layer-1 aggregation
	)
	.expect("Failed to generate circuit binaries");

	let elapsed = start.elapsed();
	println!(
		"cargo:warning=[pallet-wormhole] ZK circuit binaries generated in {:.2}s",
		elapsed.as_secs_f64()
	);

	// Print hashes of generated binaries
	let out_path = Path::new(&out_dir);
	print_bin_hash(out_path, "common.bin");
	print_bin_hash(out_path, "verifier.bin");
	print_bin_hash(out_path, "dummy_proof.bin");
	print_bin_hash(out_path, "aggregated_common.bin");
	print_bin_hash(out_path, "aggregated_verifier.bin");
}
