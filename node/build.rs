use substrate_build_script_utils::{generate_cargo_keys, rerun_if_git_head_changed};

fn main() {
	sanitize_git_commit_hash_override();

	generate_cargo_keys();

	rerun_if_git_head_changed();

	// Note: Wormhole circuit binaries are now generated at build time by pallet-wormhole's
	// build.rs. Validation happens there and at runtime when the verifier is initialized.
}

/// Neutralize Cargo-directive injection through `SUBSTRATE_CLI_GIT_COMMIT_HASH`.
///
/// `generate_cargo_keys()` (upstream `substrate-build-script-utils`) trusts this
/// override after only trimming it and interpolates it into `cargo:rustc-env=`
/// lines. Cargo's build-script protocol is line-oriented, so a value containing
/// an embedded newline followed by another `cargo:` instruction (`rustc-cfg`,
/// `rustc-link-arg`, ...) would be executed as an additional directive — giving
/// whoever sets the variable (e.g. a CI job sourcing it from release metadata)
/// authority over the compiled node artifact instead of just its version text.
///
/// The value's only legitimate shape is a (short or full) hex Git commit hash,
/// so enforce that allowlist here, before the helper reads the variable. An
/// empty value is also inert and kept as-is (upstream then omits the commit
/// suffix entirely). Anything else is replaced with `unknown`, matching the
/// helper's own fallback when no hash is available.
fn sanitize_git_commit_hash_override() {
	const VAR: &str = "SUBSTRATE_CLI_GIT_COMMIT_HASH";
	// Longest accepted value: a full SHA-256 Git object name.
	const MAX_LEN: usize = 64;

	let Ok(hash) = std::env::var(VAR) else { return };
	let hash = hash.trim();
	let is_hex = hash.len() <= MAX_LEN && hash.bytes().all(|b| b.is_ascii_hexdigit());
	if !is_hex {
		println!("cargo:warning={VAR} is not a hex Git commit hash; using 'unknown'");
		std::env::set_var(VAR, "unknown");
	}
}
