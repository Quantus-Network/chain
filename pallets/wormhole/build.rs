//! Build script for pallet-wormhole.
//!
//! Generates circuit binaries (aggregated_verifier.bin, aggregated_common.bin) at build time.
//! This ensures the binaries are always consistent with the circuit crate version and
//! eliminates the need to commit large binary files to the repository.

use std::{env, path::Path, time::Instant};

fn main() {
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
}
