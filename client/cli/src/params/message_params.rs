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

//! Params to configure how a message should be passed into a command.

use crate::error::Error;
use array_bytes::{hex2bytes, hex_bytes2hex_str};
use clap::Args;
use std::io::{BufRead, Read};

/// Maximum message size accepted by CLI sign/verify message params.
///
/// Matches Dilithium's `MAX_MESSAGE_SIZE` so oversized stdin/`--message` input is
/// rejected with a recoverable error before `Pair::sign` can panic on
/// `SignatureError::MessageTooLong`.
pub const MAX_MESSAGE_BYTES: usize = qp_rusty_crystals_dilithium::ml_dsa_87::MAX_MESSAGE_SIZE;

/// Params to configure how a message should be passed into a command.
#[derive(Debug, Clone, Args)]
pub struct MessageParams {
	/// Message to process. Will be read from STDIN otherwise.
	/// The message is assumed to be raw bytes per default. Use `--hex` for hex input. Can
	/// optionally be prefixed with `0x` in the hex case.
	#[arg(long)]
	message: Option<String>,

	/// The message is hex-encoded data.
	#[arg(long)]
	hex: bool,
}

impl MessageParams {
	/// Produces the message by either using its immediate value or reading from stdin.
	///
	/// This function should only be called once and the result cached.
	pub(crate) fn message_from<F, R>(&self, create_reader: F) -> Result<Vec<u8>, Error>
	where
		R: BufRead,
		F: FnOnce() -> R,
	{
		let raw = match &self.message {
			Some(raw) => raw.as_bytes().to_vec(),
			None => {
				// Read at most MAX+1 bytes so unbounded stdin cannot exhaust memory
				// before the size check below.
				let mut raw = Vec::new();
				create_reader().take((MAX_MESSAGE_BYTES as u64) + 1).read_to_end(&mut raw)?;
				raw
			},
		};
		ensure_message_len(raw.len())?;
		let message =
			if self.hex { hex2bytes(hex_bytes2hex_str(&raw)?).map_err(Error::from)? } else { raw };
		ensure_message_len(message.len())?;
		Ok(message)
	}
}

fn ensure_message_len(len: usize) -> Result<(), Error> {
	if len > MAX_MESSAGE_BYTES {
		return Err(Error::Input(format!(
			"message length ({len}) exceeds maximum allowed ({MAX_MESSAGE_BYTES} bytes)"
		)));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Test that decoding an immediate message works.
	#[test]
	fn message_decode_immediate() {
		for (name, input, hex, output) in test_closures() {
			println!("Testing: immediate_{}", name);
			let params = MessageParams { message: Some(input.into()), hex };
			let message = params.message_from(|| std::io::stdin().lock());

			match output {
				Some(output) => {
					let message = message.expect(&format!("{}: should decode but did not", name));
					assert_eq!(message, output, "{}: decoded a wrong message", name);
				},
				None => {
					message.err().expect(&format!("{}: should not decode but did", name));
				},
			}
		}
	}

	/// Test that decoding a message from a stream works.
	#[test]
	fn message_decode_stream() {
		for (name, input, hex, output) in test_closures() {
			println!("Testing: stream_{}", name);
			let params = MessageParams { message: None, hex };
			let message = params.message_from(|| input.as_bytes());

			match output {
				Some(output) => {
					let message = message.expect(&format!("{}: should decode but did not", name));
					assert_eq!(message, output, "{}: decoded a wrong message", name);
				},
				None => {
					message.err().expect(&format!("{}: should not decode but did", name));
				},
			}
		}
	}

	/// Returns (test_name, input, hex, output).
	fn test_closures() -> Vec<(&'static str, &'static str, bool, Option<&'static [u8]>)> {
		vec![
			("decode_no_hex_works", "Hello this is not hex", false, Some(b"Hello this is not hex")),
			("decode_no_hex_with_hex_string_works", "0xffffffff", false, Some(b"0xffffffff")),
			("decode_hex_works", "0x00112233", true, Some(&[0, 17, 34, 51])),
			("decode_hex_without_prefix_works", "00112233", true, Some(&[0, 17, 34, 51])),
			("decode_hex_uppercase_works", "0xaAbbCCDd", true, Some(&[170, 187, 204, 221])),
			("decode_hex_wrong_len_errors", "0x0011223", true, None),
		]
	}

	#[test]
	fn rejects_oversized_stream_message() {
		let params = MessageParams { message: None, hex: false };
		// One byte over the Dilithium max — must fail before any signing panic.
		let oversized = vec![b'a'; MAX_MESSAGE_BYTES + 1];
		let err = params.message_from(|| oversized.as_slice()).unwrap_err();
		assert!(
			matches!(err, Error::Input(ref msg) if msg.contains("exceeds maximum allowed")),
			"unexpected error: {err:?}"
		);
	}
}
