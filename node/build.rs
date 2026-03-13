use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};

fn main() {
	generate_cargo_keys();

	rerun_if_git_head_changed();

	// Note: Wormhole circuit binaries are now generated at build time by pallet-wormhole's
	// build.rs. Validation happens there and at runtime when the verifier is initialized.
}
