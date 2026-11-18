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

use super::{code_provider::CodeProvider, ClientConfig};
use sc_client_api::{
	backend, call_executor::CallExecutor, execution_extensions::ExecutionExtensions, HeaderBackend,
	TrieCacheContext,
};
use sc_executor::{RuntimeVersion, RuntimeVersionOf};
use sp_api::ProofRecorder;
use sp_core::traits::{CallContext, CodeExecutor};
use sp_externalities::Extensions;
use sp_runtime::{
	generic::BlockId,
	traits::{Block as BlockT, HashingFor},
};
use sp_state_machine::{backend::AsTrieBackend, OverlayedChanges, StateMachine, StorageProof};
use std::{cell::RefCell, sync::Arc};

/// A runtime call that failed under a local Wasm override.
///
/// The on-chain runtime might have completed the same call, so the failure says nothing about
/// the block being imported. Wrapping the executor's error keeps
/// `sc_executor::runtime_aborted` from reading it as an abort of the on-chain runtime.
#[derive(Debug)]
struct FailedUnderLocalOverride(Box<dyn sp_state_machine::Error>);

impl std::fmt::Display for FailedUnderLocalOverride {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{} (the runtime is a local Wasm override)", self.0)
	}
}

fn execution_error(
	error: Box<dyn sp_state_machine::Error>,
	local_override: bool,
) -> sp_blockchain::Error {
	if local_override {
		sp_blockchain::Error::Execution(Box::new(FailedUnderLocalOverride(error)))
	} else {
		error.into()
	}
}

/// Call executor that executes methods locally, querying all required
/// data from local backend.
pub struct LocalCallExecutor<Block: BlockT, B, E> {
	backend: Arc<B>,
	executor: E,
	code_provider: CodeProvider<Block, B, E>,
	execution_extensions: Arc<ExecutionExtensions<Block>>,
}

impl<Block: BlockT, B, E> LocalCallExecutor<Block, B, E>
where
	E: CodeExecutor + RuntimeVersionOf + Clone + 'static,
	B: backend::Backend<Block>,
{
	/// Creates new instance of local call executor.
	pub fn new(
		backend: Arc<B>,
		executor: E,
		client_config: ClientConfig<Block>,
		execution_extensions: ExecutionExtensions<Block>,
	) -> sp_blockchain::Result<Self> {
		let code_provider = CodeProvider::new(&client_config, executor.clone(), backend.clone())?;

		Ok(LocalCallExecutor {
			backend,
			executor,
			code_provider,
			execution_extensions: Arc::new(execution_extensions),
		})
	}
}

impl<Block: BlockT, B, E> Clone for LocalCallExecutor<Block, B, E>
where
	E: Clone,
{
	fn clone(&self) -> Self {
		LocalCallExecutor {
			backend: self.backend.clone(),
			executor: self.executor.clone(),
			code_provider: self.code_provider.clone(),
			execution_extensions: self.execution_extensions.clone(),
		}
	}
}

impl<B, E, Block> CallExecutor<Block> for LocalCallExecutor<Block, B, E>
where
	B: backend::Backend<Block>,
	E: CodeExecutor + RuntimeVersionOf + Clone + 'static,
	Block: BlockT,
{
	type Error = E::Error;

	type Backend = B;

	fn execution_extensions(&self) -> &ExecutionExtensions<Block> {
		&self.execution_extensions
	}

	fn call(
		&self,
		at_hash: Block::Hash,
		method: &str,
		call_data: &[u8],
		context: CallContext,
	) -> sp_blockchain::Result<Vec<u8>> {
		let mut changes = OverlayedChanges::default();
		let at_number =
			self.backend.blockchain().expect_block_number_from_id(&BlockId::Hash(at_hash))?;
		let state = self.backend.state_at(at_hash, context.into())?;

		let state_runtime_code = sp_state_machine::backend::BackendRuntimeCode::new(&state);
		let runtime_code =
			state_runtime_code.runtime_code().map_err(sp_blockchain::Error::RuntimeCode)?;

		let (runtime_code, _, local_override) =
			self.code_provider.maybe_override_code(runtime_code, &state, at_hash)?;

		let mut extensions = self.execution_extensions.extensions(at_hash, at_number);

		let mut sm = StateMachine::new(
			&state,
			&mut changes,
			&self.executor,
			method,
			call_data,
			&mut extensions,
			&runtime_code,
			context,
		)
		.set_parent_hash(at_hash);

		sm.execute().map_err(|error| execution_error(error, local_override))
	}

	fn contextual_call(
		&self,
		at_hash: Block::Hash,
		method: &str,
		call_data: &[u8],
		changes: &RefCell<OverlayedChanges<HashingFor<Block>>>,
		recorder: &Option<ProofRecorder<Block>>,
		call_context: CallContext,
		extensions: &RefCell<Extensions>,
	) -> Result<Vec<u8>, sp_blockchain::Error> {
		let state = self.backend.state_at(at_hash, call_context.into())?;

		let changes = &mut *changes.borrow_mut();

		// It is important to extract the runtime code here before we create the proof
		// recorder to not record it. We also need to fetch the runtime code from `state` to
		// make sure we use the caching layers.
		let state_runtime_code = sp_state_machine::backend::BackendRuntimeCode::new(&state);

		let runtime_code =
			state_runtime_code.runtime_code().map_err(sp_blockchain::Error::RuntimeCode)?;
		let (runtime_code, _, local_override) =
			self.code_provider.maybe_override_code(runtime_code, &state, at_hash)?;
		let mut extensions = extensions.borrow_mut();

		match recorder {
			Some(recorder) => {
				let trie_state = state.as_trie_backend();

				let backend = sp_state_machine::TrieBackendBuilder::wrap(&trie_state)
					.with_recorder(recorder.clone())
					.build();

				let mut state_machine = StateMachine::new(
					&backend,
					changes,
					&self.executor,
					method,
					call_data,
					&mut extensions,
					&runtime_code,
					call_context,
				)
				.set_parent_hash(at_hash);
				state_machine.execute()
			},
			None => {
				let mut state_machine = StateMachine::new(
					&state,
					changes,
					&self.executor,
					method,
					call_data,
					&mut extensions,
					&runtime_code,
					call_context,
				)
				.set_parent_hash(at_hash);
				state_machine.execute()
			},
		}
		.map_err(|error| execution_error(error, local_override))
	}

	fn runtime_version(&self, at_hash: Block::Hash) -> sp_blockchain::Result<RuntimeVersion> {
		let state = self.backend.state_at(at_hash, backend::TrieCacheContext::Untrusted)?;
		let state_runtime_code = sp_state_machine::backend::BackendRuntimeCode::new(&state);

		let runtime_code =
			state_runtime_code.runtime_code().map_err(sp_blockchain::Error::RuntimeCode)?;
		self.code_provider
			.maybe_override_code(runtime_code, &state, at_hash)
			.map(|(_, version, _)| version)
	}

	fn prove_execution(
		&self,
		at_hash: Block::Hash,
		method: &str,
		call_data: &[u8],
	) -> sp_blockchain::Result<(Vec<u8>, StorageProof)> {
		let at_number =
			self.backend.blockchain().expect_block_number_from_id(&BlockId::Hash(at_hash))?;
		let state = self.backend.state_at(at_hash, TrieCacheContext::Untrusted)?;

		let trie_backend = state.as_trie_backend();

		let state_runtime_code = sp_state_machine::backend::BackendRuntimeCode::new(trie_backend);
		let runtime_code =
			state_runtime_code.runtime_code().map_err(sp_blockchain::Error::RuntimeCode)?;
		let runtime_code = self.code_provider.maybe_override_code(runtime_code, &state, at_hash)?.0;

		sp_state_machine::prove_execution_on_trie_backend(
			trie_backend,
			&mut Default::default(),
			&self.executor,
			method,
			call_data,
			&runtime_code,
			&mut self.execution_extensions.extensions(at_hash, at_number),
		)
		.map_err(Into::into)
	}
}

impl<B, E, Block> RuntimeVersionOf for LocalCallExecutor<Block, B, E>
where
	E: RuntimeVersionOf,
	Block: BlockT,
{
	fn runtime_version(
		&self,
		ext: &mut dyn sp_externalities::Externalities,
		runtime_code: &sp_core::traits::RuntimeCode,
	) -> Result<sp_version::RuntimeVersion, sc_executor::error::Error> {
		RuntimeVersionOf::runtime_version(&self.executor, ext, runtime_code)
	}
}

impl<Block, B, E> sp_version::GetRuntimeVersionAt<Block> for LocalCallExecutor<Block, B, E>
where
	B: backend::Backend<Block>,
	E: CodeExecutor + RuntimeVersionOf + Clone + 'static,
	Block: BlockT,
{
	fn runtime_version(&self, at: Block::Hash) -> Result<sp_version::RuntimeVersion, String> {
		CallExecutor::runtime_version(self, at).map_err(|e| e.to_string())
	}
}

impl<Block, B, E> sp_version::GetNativeVersion for LocalCallExecutor<Block, B, E>
where
	B: backend::Backend<Block>,
	E: CodeExecutor + sp_version::GetNativeVersion + Clone + 'static,
	Block: BlockT,
{
	fn native_version(&self) -> &sp_version::NativeVersion {
		self.executor.native_version()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use codec::Encode;
	use sc_client_api::{
		backend::{Backend, BlockImportOperation, NewBlockState},
		in_mem,
	};
	use sc_executor::{runtime_aborted, WasmExecutor};
	use sp_core::storage::{well_known_keys, Storage};
	use sp_runtime::{
		generic,
		traits::{BlakeTwo256, Header as _},
		OpaqueExtrinsic, StateVersion,
	};
	use std::path::Path;

	type Block = generic::Block<generic::Header<u64, BlakeTwo256>, OpaqueExtrinsic>;

	/// A runtime with spec version 1 whose `execute_block` is `body`.
	fn runtime(body: &str) -> Vec<u8> {
		// `RuntimeVersion` as the executor reads it from the custom section, ending with an
		// empty list of APIs.
		let version = ("test", "test", 1u32, 1u32, 1u32, Vec::<([u8; 8], u32)>::new()).encode();
		let version: String = version.iter().map(|byte| format!("\\{byte:02x}")).collect();
		let module = format!(
			r#"
			(module
				(@custom "runtime_version" "{version}")
				(memory (export "memory") 1)
				(global (export "__heap_base") i32 (i32.const 1024))
				(func (export "execute_block") (param i32 i32) (result i64) {body}))
			"#
		);
		wat::parse_str(module).expect("valid wat")
	}

	/// Calls `execute_block` at a genesis block whose `:code` is `onchain_code`.
	fn execute_block(
		onchain_code: Vec<u8>,
		overrides: Option<&Path>,
	) -> sp_blockchain::Result<Vec<u8>> {
		let backend = Arc::new(in_mem::Backend::<Block>::new());
		let mut operation = backend.begin_operation().unwrap();
		let storage = Storage {
			top: [(well_known_keys::CODE.to_vec(), onchain_code)].into(),
			children_default: Default::default(),
		};
		let state_root = operation.set_genesis_state(storage, true, StateVersion::V1).unwrap();
		let genesis = <Block as BlockT>::Header::new(
			0,
			Default::default(),
			state_root,
			Default::default(),
			Default::default(),
		);
		let genesis_hash = genesis.hash();
		operation
			.set_block_data(genesis, Some(Vec::new()), None, None, NewBlockState::Final)
			.unwrap();
		backend.commit_operation(operation).unwrap();

		let executor = WasmExecutor::default();
		let config = ClientConfig::<Block> {
			wasm_runtime_overrides: overrides.map(Path::to_path_buf),
			..Default::default()
		};
		let extensions = ExecutionExtensions::new(None, Arc::new(executor.clone()));
		LocalCallExecutor::new(backend, executor, config, extensions).unwrap().call(
			genesis_hash,
			"execute_block",
			&[],
			CallContext::Onchain,
		)
	}

	const ACCEPTS: &str = "(i64.const 0)";
	const ABORTS: &str = "unreachable";

	#[test]
	fn abort_of_the_onchain_runtime_is_a_runtime_abort() {
		let error = execute_block(runtime(ABORTS), None).expect_err("the runtime aborts");
		assert!(runtime_aborted(&error), "{error}");
	}

	/// An override is this node's own choice of code. That it aborts does not show that the
	/// runtime the rest of the network runs would.
	#[test]
	fn abort_of_a_local_override_is_not_a_runtime_abort() {
		let overrides = tempfile::tempdir().unwrap();
		std::fs::write(overrides.path().join("test.wasm"), runtime(ABORTS)).unwrap();

		let error = execute_block(runtime(ACCEPTS), Some(overrides.path()))
			.expect_err("the override aborts");
		assert!(error.to_string().contains("local Wasm override"), "{error}");
		assert!(!runtime_aborted(&error));
		assert!(!runtime_aborted(&sp_blockchain::Error::from(sp_api::ApiError::from(error))));

		assert!(execute_block(runtime(ACCEPTS), None).is_ok());
	}
}
