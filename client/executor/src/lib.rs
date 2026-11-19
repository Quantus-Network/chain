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

//! A crate that provides means of executing/dispatching calls into the runtime.
//!
//! There are a few responsibilities of this crate at the moment:
//!
//! - It provides an implementation of a common entrypoint for calling into the runtime, both
//! wasm and compiled.
//! - It defines the environment for the wasm execution, namely the host functions that are to be
//! provided into the wasm runtime module.
//! - It also provides the required infrastructure for executing the current wasm runtime (specified
//! by the current value of `:code` in the provided externalities), i.e. interfacing with
//! wasm engine used, instance cache.

#![warn(missing_docs)]

#[macro_use]
mod executor;
mod wasm_runtime;

pub use codec::Codec;
#[allow(deprecated)]
pub use executor::NativeElseWasmExecutor;
pub use executor::{with_externalities_safe, NativeExecutionDispatch, WasmExecutor};
#[doc(hidden)]
pub use sp_core::traits::Externalities;
pub use sp_version::{NativeVersion, RuntimeVersion};
#[doc(hidden)]
pub use sp_wasm_interface;
pub use sp_wasm_interface::HostFunctions;
pub use wasm_runtime::{read_embedded_version, WasmExecutionMethod};

pub use sc_executor_common::{
	error,
	wasm_runtime::{HeapAllocStrategy, DEFAULT_HEAP_ALLOC_PAGES, DEFAULT_HEAP_ALLOC_STRATEGY},
};
pub use sc_executor_wasmtime::InstantiationStrategy as WasmtimeInstantiationStrategy;

/// `true` when `error` is a runtime call that the runtime ended itself, with a panic or a trap.
///
/// `sp_blockchain::Error::Execution` also carries failures of the host: a module that could
/// not be instantiated, a host function that failed, for example by panicking on a storage
/// read, or a call that ran out of this node's native stack. Those are not counted. An error
/// that crossed the runtime API is looked for inside `ApiError::Application`, where
/// `call_api_at` puts it.
///
/// A runtime built without `sp-io`'s `improved_panic_error_reporting` feature, as the Quantus
/// runtime is, reports a panic as an `unreachable` trap, so traps have to be counted.
pub fn runtime_aborted(error: &sp_blockchain::Error) -> bool {
	match error {
		sp_blockchain::Error::Execution(inner) => matches!(
			inner.downcast_ref::<error::Error>(),
			Some(error::Error::AbortedDueToPanic(_) | error::Error::AbortedDueToTrap(_))
		),
		sp_blockchain::Error::RuntimeApiError(sp_api::ApiError::Application(inner)) =>
			inner.downcast_ref::<sp_blockchain::Error>().is_some_and(runtime_aborted),
		_ => false,
	}
}

/// Extracts the runtime version of a given runtime code.
pub trait RuntimeVersionOf {
	/// Extract [`RuntimeVersion`] of the given `runtime_code`.
	fn runtime_version(
		&self,
		ext: &mut dyn Externalities,
		runtime_code: &sp_core::traits::RuntimeCode,
	) -> error::Result<RuntimeVersion>;
}

#[cfg(test)]
mod runtime_aborted_tests {
	use super::{runtime_aborted, WasmExecutor};
	use sp_core::{
		traits::{CallContext, RuntimeCode, WrappedRuntimeCode},
		H256,
	};
	use sp_runtime::traits::BlakeTwo256;
	use sp_state_machine::{InMemoryBackend, OverlayedChanges, StateMachine, TrieBackendBuilder};

	/// The shape a caller of the runtime API gets back: `call_api_at` converts the client
	/// error to an `ApiError`, and `?` converts it back.
	fn through_runtime_api(error: sp_blockchain::Error) -> sp_blockchain::Error {
		sp_blockchain::Error::from(sp_api::ApiError::from(error))
	}

	/// Calls `method` on a module through the state machine and the Wasm executor, the way
	/// the client's call executor does, and returns the client error for the failure.
	fn failed_runtime_call_on(
		backend: &InMemoryBackend<BlakeTwo256>,
		module: &str,
		method: &str,
	) -> sp_blockchain::Error {
		let code = wat::parse_str(module).expect("valid wat");
		let executor: WasmExecutor = WasmExecutor::builder().build();
		let mut overlay = OverlayedChanges::default();
		let mut extensions = Default::default();
		let runtime_code = RuntimeCode {
			code_fetcher: &WrappedRuntimeCode(code.into()),
			hash: b"test module".to_vec(),
			heap_pages: None,
		};

		let error = StateMachine::new(
			backend,
			&mut overlay,
			&executor,
			method,
			&[],
			&mut extensions,
			&runtime_code,
			CallContext::Onchain,
		)
		.execute()
		.expect_err("the call fails");
		sp_blockchain::Error::from(error)
	}

	fn failed_runtime_call(module: &str, method: &str) -> sp_blockchain::Error {
		failed_runtime_call_on(&InMemoryBackend::default(), module, method)
	}

	const TRAPPING_MODULE: &str = r#"
		(module
			(memory (export "memory") 1)
			(global (export "__heap_base") i32 (i32.const 1024))
			(func (export "execute_block") (param i32 i32) (result i64) unreachable))
	"#;

	const PANICKING_MODULE: &str = r#"
		(module
			(import "env" "ext_panic_handler_abort_on_panic_version_1" (func $abort (param i64)))
			(memory (export "memory") 1)
			(data (i32.const 0) "bad block")
			(global (export "__heap_base") i32 (i32.const 1024))
			(func (export "execute_block") (param i32 i32) (result i64)
				(call $abort (i64.const 0x900000000))
				unreachable))
	"#;

	/// The data segment lies far outside the memory the executor allocates, so every
	/// instantiation fails before any export is called.
	const UNINSTANTIABLE_MODULE: &str = r#"
		(module
			(memory (export "memory") 1)
			(global (export "__heap_base") i32 (i32.const 1024))
			(data (i32.const 2000000000) "x")
			(func (export "execute_block") (param i32 i32) (result i64) unreachable))
	"#;

	/// The Quantus runtime's panic handler logs the message and executes `unreachable`.
	#[test]
	fn trap_inside_the_runtime_is_an_abort() {
		let error = failed_runtime_call(TRAPPING_MODULE, "execute_block");
		assert!(runtime_aborted(&error));
		assert!(runtime_aborted(&through_runtime_api(error)));
	}

	/// A runtime built with `improved_panic_error_reporting` hands the panic message to the
	/// host instead.
	#[test]
	fn panic_inside_the_runtime_is_an_abort() {
		let error = failed_runtime_call(PANICKING_MODULE, "execute_block");
		assert!(error.to_string().contains("due to panic: bad block"), "{error}");
		assert!(runtime_aborted(&error));
		assert!(runtime_aborted(&through_runtime_api(error)));
	}

	#[test]
	fn failed_instantiation_is_not_an_abort() {
		let error = failed_runtime_call(UNINSTANTIABLE_MODULE, "execute_block");
		assert!(error.to_string().contains("failed to instantiate"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	#[test]
	fn missing_export_is_not_an_abort() {
		let error = failed_runtime_call(TRAPPING_MODULE, "missing_export");
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	const RECURSING_MODULE: &str = r#"
		(module
			(memory (export "memory") 1)
			(global (export "__heap_base") i32 (i32.const 1024))
			(func $recurse (result i64) (call $recurse))
			(func (export "execute_block") (param i32 i32) (result i64) (call $recurse)))
	"#;

	/// The node limits the stack in native bytes, which the runtime's semantics do not fix:
	/// the same call can fit on another architecture.
	#[test]
	fn stack_exhaustion_is_not_an_abort() {
		let error = failed_runtime_call(RECURSING_MODULE, "execute_block");
		assert!(error.to_string().contains("call stack exhausted"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	/// Reads the storage key held in the first byte of memory.
	const STORAGE_READING_MODULE: &str = r#"
		(module
			(import "env" "ext_storage_get_version_1" (func $storage_get (param i64) (result i64)))
			(memory (export "memory") 1)
			(global (export "__heap_base") i32 (i32.const 1024))
			(func (export "execute_block") (param i32 i32) (result i64)
				(call $storage_get (i64.const 0x100000000))))
	"#;

	/// A storage read that fails on this node, as on a damaged database, panics in the host
	/// function. The state root below names a trie node the database does not hold.
	#[test]
	fn failed_storage_read_is_not_an_abort() {
		let backend: InMemoryBackend<BlakeTwo256> =
			TrieBackendBuilder::new(Default::default(), H256::repeat_byte(1)).build();
		let error = failed_runtime_call_on(&backend, STORAGE_READING_MODULE, "execute_block");
		assert!(error.to_string().contains("Externalities not allowed to fail"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	/// Writes the storage key held in the first byte of memory, computes the storage root and
	/// traps, as the runtime does when the root differs from the one in the header.
	const STORAGE_ROOT_CHECKING_MODULE: &str = r#"
		(module
			(import "env" "ext_storage_set_version_1" (func $storage_set (param i64 i64)))
			(import "env" "ext_storage_root_version_1" (func $storage_root (result i64)))
			(memory (export "memory") 1)
			(global (export "__heap_base") i32 (i32.const 1024))
			(func (export "execute_block") (param i32 i32) (result i64)
				(call $storage_set (i64.const 0x100000000) (i64.const 0x100000000))
				(drop (call $storage_root))
				unreachable))
	"#;

	/// Clears the prefix held in the first byte of memory, as block initialization does for
	/// event topics. The prefix must not be empty: an empty prefix covers the child storage
	/// keys and is refused without touching the trie.
	const CLEAR_PREFIX_MODULE: &str = r#"
		(module
			(import "env" "ext_storage_clear_prefix_version_1" (func $clear_prefix (param i64)))
			(memory (export "memory") 1)
			(data (i32.const 0) "a")
			(global (export "__heap_base") i32 (i32.const 1024))
			(func (export "execute_block") (param i32 i32) (result i64)
				(call $clear_prefix (i64.const 0x100000000))
				(i64.const 0)))
	"#;

	/// A prefix clear that cannot read the trie, as on a damaged database, panics in the host
	/// function. It must not complete and leave the runtime to disagree with the header root.
	#[test]
	fn failed_prefix_clear_is_not_an_abort() {
		let backend: InMemoryBackend<BlakeTwo256> =
			TrieBackendBuilder::new(Default::default(), H256::repeat_byte(1)).build();
		let error = failed_runtime_call_on(&backend, CLEAR_PREFIX_MODULE, "execute_block");
		assert!(error.to_string().contains("Externalities not allowed to fail"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	/// A storage root that this node cannot compute, as on a damaged database, must not reach
	/// the runtime as a root that differs from the header's. The block writes without
	/// reading, so no storage read fails first.
	#[test]
	fn failed_storage_root_is_not_an_abort() {
		let error = failed_runtime_call(STORAGE_ROOT_CHECKING_MODULE, "execute_block");
		assert!(runtime_aborted(&error), "{error}");

		let backend: InMemoryBackend<BlakeTwo256> =
			TrieBackendBuilder::new(Default::default(), H256::repeat_byte(1)).build();
		let error = failed_runtime_call_on(&backend, STORAGE_ROOT_CHECKING_MODULE, "execute_block");
		assert!(error.to_string().contains("Externalities not allowed to fail"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&through_runtime_api(error)));
	}

	#[test]
	fn client_failure_is_not_an_abort() {
		let missing_code = sp_blockchain::Error::RuntimeCode("failed to read :code");
		assert!(!runtime_aborted(&through_runtime_api(missing_code)));
		let backend = sp_blockchain::Error::Backend("read failed".into());
		assert!(!runtime_aborted(&through_runtime_api(backend)));
		assert!(!runtime_aborted(&sp_blockchain::Error::from(sp_api::ApiError::UnknownBlock(
			"pruned".into()
		))));
	}
}
