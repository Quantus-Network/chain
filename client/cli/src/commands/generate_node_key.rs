// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Implementation of the `generate-node-key` subcommand

use crate::{build_network_key_dir_or_default, Error, NODE_KEY_DILITHIUM_FILE};
use clap::{Args, Parser};
use libp2p_identity::Keypair as Libp2pKeypair;
use sc_service::BasePath;
use sp_core::blake2_256;
use std::{
	fs,
	io::{self, Write},
	path::PathBuf,
	time::{SystemTime, UNIX_EPOCH},
};

/// Common arguments accross all generate key commands, subkey and node.
#[derive(Debug, Args, Clone)]
pub struct GenerateKeyCmdCommon {
	/// Name of file to save secret key to.
	/// If not given, the secret key is printed to stdout.
	#[arg(long)]
	file: Option<PathBuf>,

	/// The output is in raw binary format.
	/// If not given, the output is written as an hex encoded string.
	#[arg(long)]
	bin: bool,
}

/// The `generate-node-key` command
#[derive(Debug, Clone, Parser)]
#[command(
	name = "generate-node-key",
	about = "Generate a random node key, write it to a file or stdout \
		 	and write the corresponding peer-id to stderr"
)]
pub struct GenerateNodeKeyCmd {
	#[clap(flatten)]
	pub common: GenerateKeyCmdCommon,
	/// Specify the chain specification.
	///
	/// It can be any of the predefined chains like dev, local, staging, polkadot, kusama.
	#[arg(long, value_name = "CHAIN_SPEC")]
	pub chain: Option<String>,
	/// A directory where the key should be saved. If a key already
	/// exists in the directory, it won't be overwritten.
	#[arg(long, conflicts_with_all = ["file", "default_base_path"])]
	base_path: Option<PathBuf>,

	/// Save the key in the default directory. If a key already
	/// exists in the directory, it won't be overwritten.
	#[arg(long, conflicts_with_all = ["base_path", "file"])]
	default_base_path: bool,
}

impl GenerateKeyCmdCommon {
	/// Run the command
	pub fn run(&self) -> Result<(), Error> {
		generate_key(&self.file, self.bin, None, &None, false, None)
	}
}

impl GenerateNodeKeyCmd {
	/// Run the command
	pub fn run(&self, chain_spec_id: &str, executable_name: &String) -> Result<(), Error> {
		generate_key(
			&self.common.file,
			self.common.bin,
			Some(chain_spec_id),
			&self.base_path,
			self.default_base_path,
			Some(executable_name),
		)
	}
}

// Function to get current timestamp, hash it, and return hex string
fn hash_current_time_to_hex() -> [u8; 32] {
	// Get current timestamp (milliseconds since Unix epoch)
	let timestamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.expect("Time went backwards")
		.as_millis() as u64;

	// Convert timestamp to bytes and hash with BLAKE2-256
	blake2_256(&timestamp.to_le_bytes())
}

// Utility function for generating a key based on the provided CLI arguments
//
// `file`  - Name of file to save secret key to
// `bin`
fn generate_key(
	file: &Option<PathBuf>,
	bin: bool,
	chain_spec_id: Option<&str>,
	base_path: &Option<PathBuf>,
	default_base_path: bool,
	executable_name: Option<&String>,
) -> Result<(), Error> {
	// Generate a new Dilithium libp2p keypair
	let kp = Libp2pKeypair::generate_dilithium();
	let encoded = kp.to_protobuf_encoding().map_err(|e| Error::Application(Box::new(e)))?;

	// Always write protobuf bytes to files. For stdout, respect --bin and otherwise hex-encode.
	match (file, base_path, default_base_path) {
		(Some(path), None, false) => {
			// Write with restrictive permissions on unix
			#[cfg(unix)]
			{
				use std::os::unix::fs::OpenOptionsExt;
				let mut f = fs::OpenOptions::new()
					.write(true)
					.create(true)
					.truncate(true)
					.mode(0o600)
					.open(path)?;
				f.write_all(&encoded)?;
			}
			#[cfg(not(unix))]
			{
				let mut f =
					fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
				f.write_all(&encoded)?;
			}
			eprintln!("peer-id: {}", kp.public().to_peer_id());
		},
		(None, Some(_), false) | (None, None, true) => {
			let network_path = build_network_key_dir_or_default(
				base_path.clone().map(BasePath::new),
				chain_spec_id.unwrap_or_default(),
				executable_name.ok_or(Error::Input("Executable name not provided".into()))?,
			);

			fs::create_dir_all(network_path.as_path())?;

			let key_path = network_path.join(NODE_KEY_DILITHIUM_FILE);
			if key_path.exists() {
				eprintln!("Skip generation, a key already exists in {:?}", key_path);
				return Err(Error::KeyAlreadyExistsInPath(key_path));
			} else {
				eprintln!("Generating key in {:?}", key_path);
				#[cfg(unix)]
				{
					use std::os::unix::fs::OpenOptionsExt;
					let mut f = fs::OpenOptions::new()
						.write(true)
						.create(true)
						.truncate(true)
						.mode(0o600)
						.open(&key_path)?;
					f.write_all(&encoded)?;
				}
				#[cfg(not(unix))]
				{
					let mut f = fs::OpenOptions::new()
						.write(true)
						.create(true)
						.truncate(true)
						.open(&key_path)?;
					f.write_all(&encoded)?;
				}
				eprintln!("peer-id: {}", kp.public().to_peer_id());
			}
		},
		(None, None, false) => {
			if bin {
				io::stdout().lock().write_all(&encoded)?;
			} else {
				let hex = array_bytes::bytes2hex("", &encoded);
				writeln!(io::stdout().lock(), "{}", hex)?;
			}
			eprintln!("peer-id: {}", kp.public().to_peer_id());
		},
		_ => {
			// This should not happen, arguments are marked as mutually exclusive.
			return Err(Error::Input("Mutually exclusive arguments provided".into()));
		},
	}

	Ok(())
}

#[cfg(test)]
pub mod tests {
	use crate::DEFAULT_NETWORK_CONFIG_PATH;

	use super::*;
	use std::io::Read;
	use tempfile::Builder;

	#[test]
	fn generate_node_key() {
		let mut file = Builder::new().prefix("keyfile").tempfile().unwrap();
		let file_path = file.path().display().to_string();
		let generate = GenerateNodeKeyCmd::parse_from(&["generate-node-key", "--file", &file_path]);
		assert!(generate.run("test", &String::from("test")).is_ok());
		let mut buf = Vec::new();
		assert!(file.read_to_end(&mut buf).is_ok());
		assert!(libp2p_identity::Keypair::from_protobuf_encoding(&buf).is_ok());
	}

	#[test]
	fn generate_node_key_base_path() {
		let base_dir = Builder::new().prefix("keyfile").tempdir().unwrap();
		let key_path = base_dir
			.path()
			.join("chains/test_id/")
			.join(DEFAULT_NETWORK_CONFIG_PATH)
			.join(NODE_KEY_DILITHIUM_FILE);
		let base_path = base_dir.path().display().to_string();
		let generate =
			GenerateNodeKeyCmd::parse_from(&["generate-node-key", "--base-path", &base_path]);
		assert!(generate.run("test_id", &String::from("test")).is_ok());
		let buf = fs::read(key_path.as_path()).unwrap();
		assert!(libp2p_identity::Keypair::from_protobuf_encoding(&buf).is_ok());

		assert!(generate.run("test_id", &String::from("test")).is_err());
		let new_buf = fs::read(key_path).unwrap();
		assert_eq!(new_buf, buf);
	}
}
