//! Build script for pallet-wormhole.
//!
//! Generates circuit binaries at build time.
//! This ensures the binaries are always consistent with the circuit crate version and
//! eliminates the need to commit large binary files to the repository.
//!
//! Note: Circuit generation cannot be skipped for this pallet because the binaries are
//! embedded at compile time via `include_bytes!`.

use std::{env, path::Path, time::Instant};

fn env_flag(name: &str) -> bool {
	matches!(
		env::var(name).as_deref(),
		Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES") | Ok("on") | Ok("ON")
	)
}

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
	println!("cargo:rerun-if-env-changed=QP_NUM_LEAF_PROOFS");
	println!("cargo:rerun-if-env-changed=QP_NUM_LAYER0_PROOFS");
	println!("cargo:rerun-if-env-changed=QP_GENERATE_LAYER1");
	println!("cargo:rustc-check-cfg=cfg(wormhole_layer1_verifier)");

	let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
	let num_leaf_proofs: usize = env::var("QP_NUM_LEAF_PROOFS")
		.unwrap_or_else(|_| "16".to_string())
		.parse()
		.expect("QP_NUM_LEAF_PROOFS must be a valid usize");
	let generate_layer1 = env_flag("QP_GENERATE_LAYER1");
	let num_layer0_proofs = if generate_layer1 {
		let value: usize = env::var("QP_NUM_LAYER0_PROOFS")
			.expect("QP_NUM_LAYER0_PROOFS must be set when QP_GENERATE_LAYER1=true")
			.parse()
			.expect("QP_NUM_LAYER0_PROOFS must be a valid usize");
		println!("cargo:rustc-cfg=wormhole_layer1_verifier");
		Some(value)
	} else {
		None
	};

	// cargo:warning= messages are shown during build when the script runs
	println!(
		"cargo:warning=[pallet-wormhole] Generating ZK circuit binaries (num_leaf_proofs={}, num_layer0_proofs={})...",
		num_leaf_proofs,
		num_layer0_proofs.unwrap_or(0)
	);

	let start = Instant::now();

	// Generate verifier artifacts only. Prover artifacts are intentionally not embedded in runtime.
	qp_wormhole_circuit_builder::generate_all_circuit_binaries(
		Path::new(&out_dir),
		false, // include_prover = false
		num_leaf_proofs,
		num_layer0_proofs,
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
	if num_layer0_proofs.is_some() {
		print_bin_hash(out_path, "layer1_common.bin");
		print_bin_hash(out_path, "layer1_verifier.bin");
	}
}
