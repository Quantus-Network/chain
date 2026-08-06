//! Local runtime disallow-list.
//!
//! Operators may refuse specific runtime wasm blobs by listing their Blake2-256
//! code hashes in `{data_path}/disallowed-runtimes.json`. When a block would
//! change `:code` to a listed hash, block import is rejected on this node only.
//! An empty list (the default) accepts all upgrades — forkless upgrades keep
//! working without coordination.

use sc_consensus_qpow::RuntimeCodeGate;
use serde::{Deserialize, Serialize};
use sp_core::hashing::blake2_256;
use std::{
	collections::HashSet,
	fs,
	path::{Path, PathBuf},
};

const LOG_TARGET: &str = "disallowed-runtimes";

/// Default filename under the chain data path.
pub const DEFAULT_FILENAME: &str = "disallowed-runtimes.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DisallowedRuntimesFile {
	/// Blake2-256 hashes of disallowed runtime wasm, as `0x`-prefixed hex.
	#[serde(default)]
	disallowed_code_hashes: Vec<String>,
}

/// Ensures `path` exists with an empty disallow-list.
pub fn ensure_default_file(path: &Path) -> std::io::Result<()> {
	if path.exists() {
		return Ok(());
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let contents = serde_json::to_string_pretty(&DisallowedRuntimesFile::default())
		.expect("empty disallow-list serializes")
		+ "\n";
	fs::write(path, contents)?;
	log::info!(
		target: LOG_TARGET,
		"Created empty runtime disallow-list at {}",
		path.display()
	);
	Ok(())
}

/// Resolve the disallow-list path: CLI override, or `{data_path}/disallowed-runtimes.json`.
pub fn resolve_path(data_path: &Path, cli_override: Option<PathBuf>) -> PathBuf {
	cli_override.unwrap_or_else(|| data_path.join(DEFAULT_FILENAME))
}

/// Gate that re-reads the JSON file on every `:code` change.
pub struct DisallowedRuntimesGate {
	path: PathBuf,
}

impl DisallowedRuntimesGate {
	pub fn new(path: PathBuf) -> Self {
		Self { path }
	}

	fn load_hashes(&self) -> HashSet<[u8; 32]> {
		let raw = match fs::read_to_string(&self.path) {
			Ok(s) => s,
			Err(e) => {
				log::warn!(
					target: LOG_TARGET,
					"Failed to read {}: {e}. Treating disallow-list as empty.",
					self.path.display()
				);
				return HashSet::new();
			},
		};

		let parsed: DisallowedRuntimesFile = match serde_json::from_str(&raw) {
			Ok(v) => v,
			Err(e) => {
				log::warn!(
					target: LOG_TARGET,
					"Failed to parse {}: {e}. Treating disallow-list as empty.",
					self.path.display()
				);
				return HashSet::new();
			},
		};

		let mut out = HashSet::new();
		for entry in parsed.disallowed_code_hashes {
			match parse_code_hash(&entry) {
				Ok(hash) => {
					out.insert(hash);
				},
				Err(reason) => {
					log::warn!(
						target: LOG_TARGET,
						"Ignoring malformed disallow-list entry {entry:?}: {reason}"
					);
				},
			}
		}
		out
	}
}

impl RuntimeCodeGate for DisallowedRuntimesGate {
	fn allow_new_code(&self, code: &[u8]) -> Result<(), String> {
		let hash = blake2_256(code);
		let disallowed = self.load_hashes();
		if disallowed.contains(&hash) {
			let hex = format!("0x{}", hex::encode(hash));
			Err(format!(
				"{hex} is listed in {} — refusing to import this runtime upgrade",
				self.path.display()
			))
		} else {
			Ok(())
		}
	}
}

fn parse_code_hash(s: &str) -> Result<[u8; 32], String> {
	let hex_str = s.strip_prefix("0x").ok_or_else(|| "missing 0x prefix".to_string())?;
	if hex_str.len() != 64 {
		return Err(format!("expected 64 hex chars after 0x, got {}", hex_str.len()));
	}
	let bytes = hex::decode(hex_str).map_err(|e| format!("invalid hex: {e}"))?;
	bytes.try_into().map_err(|_| "expected 32 bytes".to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	#[test]
	fn parse_code_hash_accepts_0x_hex() {
		let h =
			parse_code_hash("0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
				.expect("valid");
		assert_eq!(h[0], 0x00);
		assert_eq!(h[31], 0x1f);
	}

	#[test]
	fn parse_code_hash_rejects_missing_prefix() {
		assert!(parse_code_hash("00".repeat(32).as_str()).is_err());
	}

	#[test]
	fn gate_rejects_listed_hash() {
		let dir = tempfile_dir();
		let path = dir.join(DEFAULT_FILENAME);
		let code = b"fake-runtime-wasm";
		let hash = blake2_256(code);
		let file = DisallowedRuntimesFile {
			disallowed_code_hashes: vec![format!("0x{}", hex::encode(hash))],
		};
		fs::write(&path, serde_json::to_string_pretty(&file).unwrap()).unwrap();

		let gate = DisallowedRuntimesGate::new(path);
		assert!(gate.allow_new_code(code).is_err());
		assert!(gate.allow_new_code(b"other-runtime").is_ok());
	}

	#[test]
	fn ensure_default_file_creates_empty_list() {
		let dir = tempfile_dir();
		let path = dir.join(DEFAULT_FILENAME);
		ensure_default_file(&path).unwrap();
		assert!(path.exists());
		let parsed: DisallowedRuntimesFile =
			serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
		assert!(parsed.disallowed_code_hashes.is_empty());
	}

	fn tempfile_dir() -> PathBuf {
		let mut dir = std::env::temp_dir();
		dir.push(format!("quantus-disallowed-runtimes-{}", std::process::id()));
		dir.push(format!("{}", rand_suffix()));
		fs::create_dir_all(&dir).unwrap();
		dir
	}

	fn rand_suffix() -> u64 {
		use std::time::{SystemTime, UNIX_EPOCH};
		SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
	}

	#[test]
	fn malformed_entries_are_skipped() {
		let dir = tempfile_dir();
		let path = dir.join(DEFAULT_FILENAME);
		let mut f = fs::File::create(&path).unwrap();
		write!(f, r#"{{"disallowed_code_hashes":["not-a-hash","0xzz","0x{}"]}}"#, "ab".repeat(32))
			.unwrap();
		let gate = DisallowedRuntimesGate::new(path);
		// "ab"*32 is valid; others warned and skipped
		let mut expected = [0u8; 32];
		expected.copy_from_slice(&hex::decode("ab".repeat(32)).unwrap());
		let set = gate.load_hashes();
		assert_eq!(set.len(), 1);
		assert!(set.contains(&expected));
	}
}
