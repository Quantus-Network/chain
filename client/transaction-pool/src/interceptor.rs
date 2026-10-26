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

//! Node-local hook that can claim extrinsics before they enter the pool.
//!
//! An interceptor sees every extrinsic offered to [`crate::TransactionPoolWrapper`]
//! through `submit_at` / `submit_one`, from the network and from local RPC alike.
//! An extrinsic it claims never enters the pool: it is not validated by the
//! runtime, not propagated to peers, and not available to block authoring. The
//! submitter still receives the extrinsic hash as a successful submission, so
//! peers are not penalised and the handler's known-transaction cache dedupes
//! re-gossip as usual.
//!
//! Watched submissions (`submit_and_watch`) are never offered to the
//! interceptor: a watcher promises status updates for *that* extrinsic hash,
//! which a component that takes custody of the extrinsic cannot honour.

use async_trait::async_trait;
use sc_transaction_pool_api::TransactionSource;
use sp_runtime::traits::Block as BlockT;

/// Outcome of offering an extrinsic to a [`SubmissionInterceptor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interception {
	/// Not claimed; the pool admits the extrinsic normally.
	Pass,
	/// Claimed; the pool must not see the extrinsic.
	Claimed,
}

/// A component that may take custody of extrinsics before pool admission.
#[async_trait]
pub trait SubmissionInterceptor<Block: BlockT>: Send + Sync {
	/// Offer `xt`, submitted against `at` from `source`, to the interceptor.
	///
	/// Implementations must be cheap to decline: this runs on the submission
	/// path for every extrinsic the node receives.
	async fn intercept(
		&self,
		at: Block::Hash,
		source: TransactionSource,
		xt: &Block::Extrinsic,
	) -> Interception;
}
