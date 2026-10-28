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

use crate::{
	error,
	params::{DatabaseParams, SharedParams},
	CliConfiguration,
};
use clap::Parser;
use sc_service::DatabaseSource;
use std::{
	fmt::Debug,
	fs,
	io::{self, Write},
	path::Path,
};

/// The `purge-chain` command used to remove the whole chain.
#[derive(Debug, Clone, Parser)]
pub struct PurgeChainCmd {
	/// Skip interactive prompt by answering yes automatically.
	#[arg(short = 'y')]
	pub yes: bool,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub shared_params: SharedParams,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub database_params: DatabaseParams,
}

impl PurgeChainCmd {
	/// Run the purge command
	pub fn run(&self, database_config: DatabaseSource) -> error::Result<()> {
		let db_paths = purge_paths(&database_config)?;

		if !self.yes {
			print!("Are you sure to remove {:?}? [y/N]: ", &db_paths);
			io::stdout().flush().expect("failed to flush stdout");

			let mut input = String::new();
			io::stdin().read_line(&mut input)?;
			let input = input.trim();

			match input.chars().next() {
				Some('y') | Some('Y') => {},
				_ => {
					println!("Aborted");
					return Ok(())
				},
			}
		}

		for db_path in db_paths {
			match fs::remove_dir_all(db_path) {
				Ok(_) => println!("{:?} removed.", db_path),
				Err(ref err) if err.kind() == io::ErrorKind::NotFound =>
					eprintln!("{:?} did not exist.", db_path),
				Err(err) => return Err(err.into()),
			}
		}
		Ok(())
	}
}

/// Directories removed by `purge-chain`. `Auto` does not say which backend holds the
/// chain, so both candidate locations are purged.
fn purge_paths(database_config: &DatabaseSource) -> error::Result<Vec<&Path>> {
	fn parent(p: &Path) -> error::Result<&Path> {
		p.parent()
			.ok_or_else(|| error::Error::Input(format!("Database path {p:?} has no parent")))
	}
	match database_config {
		DatabaseSource::Auto { paritydb_path, rocksdb_path, .. } =>
			Ok(vec![parent(paritydb_path)?, parent(rocksdb_path)?]),
		other => {
			let path = other.path().ok_or_else(|| {
				error::Error::Input("Cannot purge custom database implementation".into())
			})?;
			Ok(vec![parent(path)?])
		},
	}
}

impl CliConfiguration for PurgeChainCmd {
	fn shared_params(&self) -> &SharedParams {
		&self.shared_params
	}

	fn database_params(&self) -> Option<&DatabaseParams> {
		Some(&self.database_params)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::Parser;

	fn cmd() -> PurgeChainCmd {
		PurgeChainCmd::parse_from(["purge-chain", "-y"])
	}

	fn layout(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
		let rocksdb_path = root.join("db").join("full");
		let paritydb_path = root.join("paritydb").join("full");
		fs::create_dir_all(&rocksdb_path).unwrap();
		fs::write(rocksdb_path.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
		(paritydb_path, rocksdb_path)
	}

	#[test]
	fn auto_purges_rocksdb_only_layout() {
		let root = tempfile::tempdir().unwrap();
		let (paritydb_path, rocksdb_path) = layout(root.path());
		cmd()
			.run(DatabaseSource::Auto { paritydb_path, rocksdb_path, cache_size: 128 })
			.unwrap();
		assert!(!root.path().join("db").exists());
		assert!(!root.path().join("paritydb").exists());
	}

	#[test]
	fn auto_purges_both_backends() {
		let root = tempfile::tempdir().unwrap();
		let (paritydb_path, rocksdb_path) = layout(root.path());
		fs::create_dir_all(&paritydb_path).unwrap();
		fs::write(paritydb_path.join("metadata"), b"x").unwrap();
		cmd()
			.run(DatabaseSource::Auto { paritydb_path, rocksdb_path, cache_size: 128 })
			.unwrap();
		assert!(!root.path().join("db").exists());
		assert!(!root.path().join("paritydb").exists());
	}

	#[test]
	fn explicit_backend_purges_only_its_own_directory() {
		let root = tempfile::tempdir().unwrap();
		let (paritydb_path, rocksdb_path) = layout(root.path());
		fs::create_dir_all(&paritydb_path).unwrap();
		cmd().run(DatabaseSource::ParityDb { path: paritydb_path }).unwrap();
		assert!(rocksdb_path.join("CURRENT").is_file());
		assert!(!root.path().join("paritydb").exists());
	}
}
