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

//! Policy for which runtime APIs an untrusted (public) caller may invoke
//! through an offchain `executor().call(.., CallContext::Offchain)` RPC
//! surface.
//!
//! Several RPC methods let a caller run an arbitrary runtime API, by name,
//! against a block's state: `state_call` / `state_callAt`, the RPC-v2
//! `archive_v1_call`, and the RPC-v2 `chainHead_v1_call`. None of these are
//! gated by `check_if_safe`, so on a public node they are reachable by an
//! unauthenticated caller. Left unrestricted they forward *any* runtime API to
//! the executor, including unmetered heavy ones — `GenesisBuilder_build_state`
//! (a full serde parse plus genesis storage writes) and
//! `BlockBuilder_apply_extrinsic` (which can reach pallet `pre_dispatch` work
//! kept off the fee-free pool-admission path) — a fee-free remote DoS.
//!
//! This module is the single source of truth for the allowlist that every one
//! of those surfaces applies to untrusted callers: only cheap, read-only
//! runtime APIs that legitimate tooling (subxt, wallets, explorers) relies on.
//! Trusted callers (local connection / `--rpc-methods unsafe`) are not gated by
//! this and keep full runtime-API access.

use jsonrpsee::Extensions;
use sc_rpc_api::DenyUnsafe;

/// Cheap, read-only runtime APIs an untrusted (public) caller may invoke
/// through an offchain runtime-call RPC surface.
///
/// Everything not on this list (block building, execution, transaction
/// validation, genesis construction, offchain worker, …) is refused for
/// untrusted callers. Operators who need a wider public surface can extend this
/// list.
pub const PUBLIC_RUNTIME_CALL_ALLOWLIST: &[&str] = &[
	"Core_version",
	"Metadata_metadata",
	"Metadata_metadata_at_version",
	"Metadata_metadata_versions",
	"AccountNonceApi_account_nonce",
	"TransactionPaymentApi_query_info",
	"TransactionPaymentApi_query_fee_details",
	"TransactionPaymentApi_query_weight_to_fee",
	"TransactionPaymentApi_query_length_to_fee",
	"TransactionPaymentCallApi_query_call_info",
	"TransactionPaymentCallApi_query_call_fee_details",
	"TransactionPaymentCallApi_query_weight_to_fee",
	"TransactionPaymentCallApi_query_length_to_fee",
];

/// Whether an untrusted (public) caller may invoke `method` as an offchain
/// runtime call. See [`PUBLIC_RUNTIME_CALL_ALLOWLIST`].
pub fn is_public_runtime_call_allowed(method: &str) -> bool {
	PUBLIC_RUNTIME_CALL_ALLOWLIST.contains(&method)
}

/// Whether the connection behind `ext` is trusted, following the per-connection
/// [`DenyUnsafe`] policy the RPC server inserts into every request's extensions.
///
/// A missing extension is treated as untrusted: the server always sets it, so
/// its absence is a wiring error and must fail closed rather than widen access.
pub fn is_trusted_caller(ext: &Extensions) -> bool {
	matches!(ext.get::<DenyUnsafe>(), Some(DenyUnsafe::No))
}

/// Whether the caller behind `ext` may invoke `method` as an offchain runtime
/// call: trusted callers may invoke anything, untrusted callers only the
/// [`PUBLIC_RUNTIME_CALL_ALLOWLIST`].
pub fn is_runtime_call_allowed(ext: &Extensions, method: &str) -> bool {
	is_trusted_caller(ext) || is_public_runtime_call_allowed(method)
}

#[cfg(test)]
mod tests {
	use super::*;

	const HEAVY_METHODS: &[&str] = &[
		"GenesisBuilder_build_state",
		"BlockBuilder_apply_extrinsic",
		"Core_execute_block",
		"BlockBuilder_check_inherents",
		"TaggedTransactionQueue_validate_transaction",
		"OffchainWorkerApi_offchain_worker",
	];

	fn ext_with(deny_unsafe: Option<DenyUnsafe>) -> Extensions {
		let mut ext = Extensions::new();
		if let Some(deny_unsafe) = deny_unsafe {
			ext.insert(deny_unsafe);
		}
		ext
	}

	#[test]
	fn allowlisted_reads_are_allowed() {
		for method in PUBLIC_RUNTIME_CALL_ALLOWLIST {
			assert!(is_public_runtime_call_allowed(method), "{method} should be allowed");
		}
	}

	#[test]
	fn heavy_and_execution_apis_are_denied() {
		for method in HEAVY_METHODS {
			assert!(!is_public_runtime_call_allowed(method), "{method} should be denied");
		}
	}

	#[test]
	fn trust_follows_deny_unsafe_and_fails_closed_when_missing() {
		assert!(is_trusted_caller(&ext_with(Some(DenyUnsafe::No))));
		assert!(!is_trusted_caller(&ext_with(Some(DenyUnsafe::Yes))));
		assert!(!is_trusted_caller(&ext_with(None)));
	}

	#[test]
	fn untrusted_callers_are_limited_to_the_allowlist() {
		for ext in [ext_with(Some(DenyUnsafe::Yes)), ext_with(None)] {
			for method in HEAVY_METHODS {
				assert!(!is_runtime_call_allowed(&ext, method), "{method} should be denied");
			}
			for method in PUBLIC_RUNTIME_CALL_ALLOWLIST {
				assert!(is_runtime_call_allowed(&ext, method), "{method} should be allowed");
			}
		}
	}

	#[test]
	fn trusted_callers_keep_the_full_surface() {
		let ext = ext_with(Some(DenyUnsafe::No));
		for method in HEAVY_METHODS.iter().chain(PUBLIC_RUNTIME_CALL_ALLOWLIST) {
			assert!(is_runtime_call_allowed(&ext, method), "{method} should be allowed");
		}
	}
}
