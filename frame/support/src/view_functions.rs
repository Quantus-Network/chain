// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License fsor the specific language governing permissions and
// limitations under the License.

//! Traits for querying pallet view functions.

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemLimit, Encode, Output};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;

/// The unique identifier for a view function.
#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct ViewFunctionId {
	/// The part of the id for dispatching view functions from the top level of the runtime.
	///
	/// Specifies which view function grouping this view function belongs to. This could be a group
	/// of view functions associated with a pallet, or a pallet agnostic group of view functions.
	pub prefix: [u8; 16],
	/// The part of the id for dispatching to a view function within a group.
	pub suffix: [u8; 16],
}

impl From<ViewFunctionId> for [u8; 32] {
	fn from(value: ViewFunctionId) -> Self {
		let mut output = [0u8; 32];
		output[..16].copy_from_slice(&value.prefix);
		output[16..].copy_from_slice(&value.suffix);
		output
	}
}

/// Error type for view function dispatching.
#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum ViewFunctionDispatchError {
	/// View functions are not implemented for this runtime.
	NotImplemented,
	/// A view function with the given `ViewFunctionId` was not found
	NotFound(ViewFunctionId),
	/// Failed to decode the view function input.
	Codec,
}

impl From<codec::Error> for ViewFunctionDispatchError {
	fn from(_: codec::Error) -> Self {
		ViewFunctionDispatchError::Codec
	}
}

/// Implemented by both pallets and the runtime. The runtime is dispatching by prefix using the
/// pallet implementation of `ViewFunctionIdPrefix` then the pallet is dispatching by suffix using
/// the methods implementation of `ViewFunctionIdSuffix`.
///
/// In more details, `ViewFunctionId` = `ViewFunctionIdPrefix` ++ `ViewFunctionIdSuffix`, where
/// `ViewFunctionIdPrefix=twox_128(pallet_name)` and
/// `ViewFunctionIdSuffix=twox_128("fn_name(fnarg_types) -> return_ty")`. The prefix is the same as
/// the storage prefix for pallets. The suffix is generated from the view function method type
/// signature, so is guaranteed to be unique for that pallet implementation.
pub trait DispatchViewFunction {
	fn dispatch_view_function<O: Output>(
		id: &ViewFunctionId,
		input: &mut &[u8],
		output: &mut O,
	) -> Result<(), ViewFunctionDispatchError>;
}

impl DispatchViewFunction for () {
	fn dispatch_view_function<O: Output>(
		_id: &ViewFunctionId,
		_input: &mut &[u8],
		_output: &mut O,
	) -> Result<(), ViewFunctionDispatchError> {
		Err(ViewFunctionDispatchError::NotImplemented)
	}
}

/// Automatically implemented for each pallet by the macro [`pallet`](crate::pallet).
pub trait ViewFunctionIdPrefix {
	fn prefix() -> [u8; 16];
}

/// Automatically implemented for each pallet view function method by the macro
/// [`pallet`](crate::pallet).
pub trait ViewFunctionIdSuffix {
	const SUFFIX: [u8; 16];
}

/// Maximum heap memory a single view-function input is allowed to allocate while decoding.
///
/// View functions are reachable through the `RuntimeViewFunction` runtime API with
/// attacker-controlled input, so decoding is bounded to prevent a small crafted request from
/// forcing a disproportionately large allocation (a decode "bomb").
pub const MAX_VIEW_FUNCTION_DECODE_MEM: usize = 16 * 1024 * 1024;

/// Automatically implemented for each pallet view function method by the macro
/// [`pallet`](crate::pallet).
pub trait ViewFunction: DecodeWithMemLimit {
	fn id() -> ViewFunctionId;
	type ReturnType: Encode;

	fn invoke(self) -> Self::ReturnType;

	fn execute<O: Output>(
		input: &mut &[u8],
		output: &mut O,
	) -> Result<(), ViewFunctionDispatchError> {
		// Use the mem-tracked decode (bounded by `MAX_VIEW_FUNCTION_DECODE_MEM`) instead of a
		// plain `DecodeAll`, and still require the whole input to be consumed.
		let view_function =
			Self::decode_with_mem_limit(input, MAX_VIEW_FUNCTION_DECODE_MEM)?;
		if !input.is_empty() {
			return Err(ViewFunctionDispatchError::Codec)
		}
		let result = view_function.invoke();
		Encode::encode_to(&result, output);
		Ok(())
	}
}

pub mod runtime_api {
	use super::*;

	sp_api::decl_runtime_apis! {
		#[api_version(1)]
		/// Runtime API for executing view functions
		pub trait RuntimeViewFunction {
			/// Execute a view function query.
			fn execute_view_function(
				query_id: ViewFunctionId,
				input: Vec<u8>,
			) -> Result<Vec<u8>, ViewFunctionDispatchError>;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use codec::{Decode, DecodeWithMemTracking};

	#[derive(Encode, Decode, DecodeWithMemTracking)]
	struct DoubleIt {
		value: u32,
	}

	impl ViewFunction for DoubleIt {
		fn id() -> ViewFunctionId {
			ViewFunctionId { prefix: [0u8; 16], suffix: [0u8; 16] }
		}
		type ReturnType = u32;

		fn invoke(self) -> Self::ReturnType {
			self.value.wrapping_mul(2)
		}
	}

	#[test]
	fn execute_decodes_invokes_and_encodes() {
		let input = 21u32.encode();
		let mut output = Vec::new();
		DoubleIt::execute(&mut &input[..], &mut output).unwrap();
		assert_eq!(u32::decode(&mut &output[..]).unwrap(), 42);
	}

	#[test]
	fn execute_rejects_trailing_bytes() {
		// A valid `u32` followed by extra trailing bytes must be rejected rather than
		// silently decoded from a prefix of the input.
		let mut input = 21u32.encode();
		input.extend_from_slice(&[0xff, 0xff]);
		let mut output = Vec::new();
		assert!(matches!(
			DoubleIt::execute(&mut &input[..], &mut output),
			Err(ViewFunctionDispatchError::Codec)
		));
	}

	#[test]
	fn execute_bounds_decode_allocation() {
		// A `Vec<u8>` view function argument whose declared length would allocate far more
		// than the mem limit is rejected instead of being decoded.
		#[derive(Encode, Decode, DecodeWithMemTracking)]
		struct TakesVec {
			data: Vec<u8>,
		}
		impl ViewFunction for TakesVec {
			fn id() -> ViewFunctionId {
				ViewFunctionId { prefix: [1u8; 16], suffix: [1u8; 16] }
			}
			type ReturnType = u32;
			fn invoke(self) -> Self::ReturnType {
				self.data.len() as u32
			}
		}

		// Encode a compact length larger than the mem limit, then provide that many bytes.
		let len = (MAX_VIEW_FUNCTION_DECODE_MEM + 1) as u32;
		let mut input = codec::Compact(len).encode();
		input.extend(core::iter::repeat(0u8).take(len as usize));
		let mut output = Vec::new();
		assert!(matches!(
			TakesVec::execute(&mut &input[..], &mut output),
			Err(ViewFunctionDispatchError::Codec)
		));
	}
}
