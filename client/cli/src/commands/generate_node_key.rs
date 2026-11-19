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
use litep2p::crypto::{dilithium::Keypair, PublicKey};
use sc_service::BasePath;
use std::{
	fs,
	io::{self, Write},
	path::{Path, PathBuf},
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

// Utility function for generating a key based on the provided CLI arguments
fn generate_key(
	file: &Option<PathBuf>,
	bin: bool,
	chain_spec_id: Option<&str>,
	base_path: &Option<PathBuf>,
	default_base_path: bool,
	executable_name: Option<&String>,
) -> Result<(), Error> {
	// Generate keypair - litep2p::crypto::dilithium::Keypair stores only the 32-byte seed
	let keypair = Keypair::generate();

	// to_bytes() returns the 32-byte seed, which is what the node expects
	let file_data = if bin {
		keypair.to_bytes().to_vec()
	} else {
		array_bytes::bytes2hex("", keypair.to_bytes()).into_bytes()
	};

	match (file, base_path, default_base_path) {
		(Some(file), None, false) => write_secret_file(file, &file_data)?,
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
				write_secret_file(&key_path, &file_data)?
			}
		},
		(None, None, false) => io::stdout().lock().write_all(&file_data)?,
		(_, _, _) => {
			// This should not happen, arguments are marked as mutually exclusive.
			return Err(Error::Input("Mutually exclusive arguments provided".into()));
		},
	}

	let public_key = PublicKey::from(keypair.public().clone());
	let peer_id = litep2p::PeerId::from_public_key(&public_key);

	eprintln!("{}", peer_id);

	Ok(())
}

/// Writes `data` to `path`, leaving a regular file accessible to its owner only.
fn write_secret_file(path: &Path, data: &[u8]) -> io::Result<()> {
	#[cfg(unix)]
	{
		write_secret_file_with(path, data, restrict_permissions)
	}
	#[cfg(not(unix))]
	{
		let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
		file.write_all(data)
	}
}

/// Opens `path` without truncating it, runs `restrict`, and only then replaces the contents.
///
/// A regular file that is being overwritten keeps its mode, so `restrict` tightens it. That
/// happens before the contents are discarded: when the mode cannot be changed, the existing
/// key is still there.
#[cfg(unix)]
fn write_secret_file_with(
	path: &Path,
	data: &[u8],
	restrict: impl FnOnce(&fs::File) -> io::Result<()>,
) -> io::Result<()> {
	use std::os::unix::fs::OpenOptionsExt;

	let mut file = fs::OpenOptions::new().write(true).create(true).mode(0o600).open(path)?;
	let metadata = file.metadata()?;
	if metadata.is_file() {
		restrict(&file)?;
		file.set_len(0)?;
	}
	file.write_all(data)
}

/// Removes group and other access from a regular file that has it.
#[cfg(unix)]
fn restrict_permissions(file: &fs::File) -> io::Result<()> {
	use std::os::unix::fs::PermissionsExt;

	if file.metadata()?.permissions().mode() & 0o077 != 0 {
		file.set_permissions(fs::Permissions::from_mode(0o600))?;
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
		let mut buf = String::new();
		assert!(file.read_to_string(&mut buf).is_ok());
		// The file should contain a 32-byte seed (64 hex chars)
		let bytes = array_bytes::hex2bytes(&buf).unwrap();
		assert_eq!(bytes.len(), 32, "Expected 32-byte seed, got {} bytes", bytes.len());
		let mut seed_bytes = bytes.clone();
		assert!(Keypair::try_from_bytes(&mut seed_bytes).is_ok());
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
		let buf = fs::read_to_string(key_path.as_path()).unwrap();
		let bytes = array_bytes::hex2bytes(&buf).unwrap();
		assert_eq!(bytes.len(), 32, "Expected 32-byte seed, got {} bytes", bytes.len());

		assert!(generate.run("test_id", &String::from("test")).is_err());
		let new_buf = fs::read_to_string(key_path).unwrap();
		assert_eq!(
			array_bytes::hex2bytes(&new_buf).unwrap(),
			array_bytes::hex2bytes(&buf).unwrap()
		);
	}

	#[cfg(unix)]
	fn mode(path: &Path) -> u32 {
		use std::os::unix::fs::PermissionsExt;
		fs::metadata(path).unwrap().permissions().mode() & 0o777
	}

	#[cfg(unix)]
	#[test]
	fn generated_key_file_is_owner_only() {
		use std::os::unix::fs::PermissionsExt;

		let dir = Builder::new().prefix("keyfile").tempdir().unwrap();

		let new_file = dir.path().join("new");
		let generate = GenerateNodeKeyCmd::parse_from(&[
			"generate-node-key",
			"--file",
			&new_file.display().to_string(),
		]);
		generate.run("test", &String::from("test")).unwrap();
		assert_eq!(mode(&new_file), 0o600);

		let existing_file = dir.path().join("existing");
		fs::write(&existing_file, "old key").unwrap();
		fs::set_permissions(&existing_file, fs::Permissions::from_mode(0o644)).unwrap();
		let generate = GenerateNodeKeyCmd::parse_from(&[
			"generate-node-key",
			"--file",
			&existing_file.display().to_string(),
		]);
		generate.run("test", &String::from("test")).unwrap();
		assert_eq!(mode(&existing_file), 0o600);
		assert_eq!(fs::read(&existing_file).unwrap().len(), 64);
	}

	/// The mode is tightened before the existing contents are discarded. A mode change that
	/// fails must leave the key that was already there.
	#[cfg(unix)]
	#[test]
	fn failed_permission_change_keeps_the_existing_key() {
		use std::os::unix::fs::PermissionsExt;

		let dir = Builder::new().prefix("keyfile").tempdir().unwrap();
		let path = dir.path().join("existing");
		fs::write(&path, "old key").unwrap();
		fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();

		let error = write_secret_file_with(&path, b"new key", |_| {
			Err(io::Error::new(io::ErrorKind::PermissionDenied, "chmod denied"))
		})
		.expect_err("a refused mode change must fail the write");

		assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
		assert_eq!(fs::read(&path).unwrap(), b"old key");
		assert_eq!(mode(&path), 0o666);
	}

	#[cfg(unix)]
	#[test]
	fn generated_key_file_in_base_path_is_owner_only() {
		let base_dir = Builder::new().prefix("keyfile").tempdir().unwrap();
		let base_path = base_dir.path().display().to_string();
		let generate =
			GenerateNodeKeyCmd::parse_from(&["generate-node-key", "--base-path", &base_path]);
		generate.run("test_id", &String::from("test")).unwrap();

		let key_path = base_dir
			.path()
			.join("chains/test_id/")
			.join(DEFAULT_NETWORK_CONFIG_PATH)
			.join(NODE_KEY_DILITHIUM_FILE);
		assert_eq!(mode(&key_path), 0o600);
	}
}
