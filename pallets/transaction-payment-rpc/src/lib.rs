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
// See the License for the specific language governing permissions and
// limitations under the License.

//! RPC interface for the transaction payment pallet.

use std::sync::Arc;

use codec::{Codec, Decode};
use jsonrpsee::{
	core::RpcResult,
	proc_macros::rpc,
	types::{
		error::{ErrorCode, ErrorObject},
		ErrorObjectOwned,
	},
};
use pallet_transaction_payment_rpc_runtime_api::{FeeDetails, InclusionFee, RuntimeDispatchInfo};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::Bytes;
use sp_rpc::number::NumberOrHex;
use sp_runtime::traits::{Block as BlockT, MaybeDisplay};

pub use pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi as TransactionPaymentRuntimeApi;

/// Maximum encoded extrinsic length accepted by fee-query RPCs before SCALE decode.
///
/// Aligned with this chain's maximum block length (`RuntimeBlockLength` = 5 MiB). Inputs
/// larger than this cannot be included on-chain; without a local cap, the JSON-RPC
/// handler would still decode them into a full `Block::Extrinsic` (e.g. a huge
/// `remark` payload) and burn RPC worker CPU/heap for free.
pub const MAX_ENCODED_EXTRINSIC_LEN: usize = 5 * 1024 * 1024;

/// Rejects fee-query inputs that exceed `max_len` before any extrinsic decode work.
fn ensure_query_extrinsic_size(encoded_xt: &[u8], max_len: usize) -> RpcResult<()> {
	if encoded_xt.len() > max_len {
		return Err(ErrorObject::owned(
			ErrorCode::InvalidParams.code(),
			format!(
				"Encoded extrinsic length ({}) exceeds maximum allowed for fee query ({})",
				encoded_xt.len(),
				max_len
			),
			None::<()>,
		));
	}
	Ok(())
}

#[rpc(client, server)]
pub trait TransactionPaymentApi<BlockHash, ResponseType> {
	#[method(name = "payment_queryInfo")]
	fn query_info(&self, encoded_xt: Bytes, at: Option<BlockHash>) -> RpcResult<ResponseType>;

	#[method(name = "payment_queryFeeDetails")]
	fn query_fee_details(
		&self,
		encoded_xt: Bytes,
		at: Option<BlockHash>,
	) -> RpcResult<FeeDetails<NumberOrHex>>;
}

/// Provides RPC methods to query a dispatchable's class, weight and fee.
pub struct TransactionPayment<C, P> {
	/// Shared reference to the client.
	client: Arc<C>,
	/// Cap on `encoded_xt` accepted by fee-query RPCs before SCALE decode.
	max_encoded_extrinsic_len: usize,
	_marker: std::marker::PhantomData<P>,
}

impl<C, P> TransactionPayment<C, P> {
	/// Creates a new instance of the TransactionPayment Rpc helper.
	pub fn new(client: Arc<C>) -> Self {
		Self::new_with_max_encoded_extrinsic_len(client, MAX_ENCODED_EXTRINSIC_LEN)
	}

	/// Creates a new instance with an explicit pre-decode size cap for `encoded_xt`.
	pub fn new_with_max_encoded_extrinsic_len(
		client: Arc<C>,
		max_encoded_extrinsic_len: usize,
	) -> Self {
		Self { client, max_encoded_extrinsic_len, _marker: Default::default() }
	}
}

/// Error type of this RPC api.
pub enum Error {
	/// The transaction was not decodable.
	DecodeError,
	/// The call to runtime failed.
	RuntimeError,
}

impl From<Error> for i32 {
	fn from(e: Error) -> i32 {
		match e {
			Error::RuntimeError => 1,
			Error::DecodeError => 2,
		}
	}
}

impl<C, Block, Balance>
	TransactionPaymentApiServer<
		<Block as BlockT>::Hash,
		RuntimeDispatchInfo<Balance, sp_weights::Weight>,
	> for TransactionPayment<C, Block>
where
	Block: BlockT,
	C: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
	C::Api: TransactionPaymentRuntimeApi<Block, Balance>,
	Balance: Codec + MaybeDisplay + Copy + TryInto<NumberOrHex> + Send + Sync + 'static,
{
	fn query_info(
		&self,
		encoded_xt: Bytes,
		at: Option<Block::Hash>,
	) -> RpcResult<RuntimeDispatchInfo<Balance, sp_weights::Weight>> {
		ensure_query_extrinsic_size(&encoded_xt, self.max_encoded_extrinsic_len)?;

		let api = self.client.runtime_api();
		let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);

		let encoded_len = encoded_xt.len() as u32;

		let uxt: Block::Extrinsic = Decode::decode(&mut &*encoded_xt).map_err(|e| {
			ErrorObject::owned(
				Error::DecodeError.into(),
				"Unable to query dispatch info.",
				Some(format!("{:?}", e)),
			)
		})?;

		fn map_err(error: impl ToString, desc: &'static str) -> ErrorObjectOwned {
			ErrorObject::owned(Error::RuntimeError.into(), desc, Some(error.to_string()))
		}

		let res = api
			.query_info(at_hash, uxt, encoded_len)
			.map_err(|e| map_err(e, "Unable to query dispatch info."))?;

		Ok(RuntimeDispatchInfo {
			weight: res.weight,
			class: res.class,
			partial_fee: res.partial_fee,
		})
	}

	fn query_fee_details(
		&self,
		encoded_xt: Bytes,
		at: Option<Block::Hash>,
	) -> RpcResult<FeeDetails<NumberOrHex>> {
		ensure_query_extrinsic_size(&encoded_xt, self.max_encoded_extrinsic_len)?;

		let api = self.client.runtime_api();
		let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);

		let encoded_len = encoded_xt.len() as u32;

		let uxt: Block::Extrinsic = Decode::decode(&mut &*encoded_xt).map_err(|e| {
			ErrorObject::owned(
				Error::DecodeError.into(),
				"Unable to query fee details.",
				Some(format!("{:?}", e)),
			)
		})?;
		let fee_details = api.query_fee_details(at_hash, uxt, encoded_len).map_err(|e| {
			ErrorObject::owned(
				Error::RuntimeError.into(),
				"Unable to query fee details.",
				Some(e.to_string()),
			)
		})?;

		let try_into_rpc_balance = |value: Balance| {
			value.try_into().map_err(|_| {
				ErrorObject::owned(
					ErrorCode::InvalidParams.code(),
					format!("{} doesn't fit in NumberOrHex representation", value),
					None::<()>,
				)
			})
		};

		Ok(FeeDetails {
			inclusion_fee: if let Some(inclusion_fee) = fee_details.inclusion_fee {
				Some(InclusionFee {
					base_fee: try_into_rpc_balance(inclusion_fee.base_fee)?,
					len_fee: try_into_rpc_balance(inclusion_fee.len_fee)?,
					adjusted_weight_fee: try_into_rpc_balance(inclusion_fee.adjusted_weight_fee)?,
				})
			} else {
				None
			},
			tip: Default::default(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn accepts_encoded_extrinsic_at_size_limit() {
		let encoded = vec![0u8; 64];
		assert!(ensure_query_extrinsic_size(&encoded, 64).is_ok());
		assert!(ensure_query_extrinsic_size(&encoded, MAX_ENCODED_EXTRINSIC_LEN).is_ok());
	}

	#[test]
	fn rejects_encoded_extrinsic_above_size_limit() {
		let encoded = vec![0u8; 65];
		let err = ensure_query_extrinsic_size(&encoded, 64).unwrap_err();
		assert_eq!(err.code(), ErrorCode::InvalidParams.code());
		assert!(err.message().contains("exceeds maximum allowed for fee query"));
	}

	#[test]
	fn default_cap_matches_chain_max_block_length() {
		// Must stay aligned with `RuntimeBlockLength` in runtime/src/configs/mod.rs.
		assert_eq!(MAX_ENCODED_EXTRINSIC_LEN, 5 * 1024 * 1024);
	}
}
