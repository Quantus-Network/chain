//! QPoW-aware chainHead RPC implementation
//!
//! This module provides chainHead RPC methods specifically adapted for Resonance's
//! Proof-of-Work consensus mechanism, which has a large finality lag (179 blocks).
//! These methods mirror the standard chainHead_v1 API but handle the PoW-specific
//! finality characteristics.
//!
//! # Implementation Status
//!
//! - ✅ `follow` - Fully implemented subscription management
//! - ✅ `header` - Retrieves and encodes block headers
//! - 🚧 `body` - Returns placeholder operation ID (not implemented)
//! - 🚧 `call` - Returns placeholder operation ID (not implemented)
//! - 🚧 `storage` - Returns placeholder operation ID (not implemented)
//! - 🚧 `continue` - Validates subscription only (not implemented)
//! - 🚧 `stopOperation` - Validates subscription only (not implemented)

#![allow(clippy::todo)] // TODOs are intentional for unimplemented features
#![allow(clippy::too_many_arguments)] // RPC methods may have many parameters

use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
    types::error::ErrorObject,
    PendingSubscriptionSink,
};
use serde::{Deserialize, Serialize};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::Block as BlockT;
use std::{marker::PhantomData, sync::Arc};

pub mod api;
pub mod events;
pub mod subscription;

#[cfg(test)]
mod tests;

use subscription::*;

/// The maximum lagging distance for QPoW consensus
/// This is set higher than substrate's default to accommodate our finality lag
const QPOW_MAX_LAGGING_DISTANCE: u32 = 200;

/// QPoW ChainHead RPC API - chainHead methods adapted for PoW consensus
#[rpc(server)]
pub trait QpowChainHeadApi<Hash> {
    /// Start following the chain with PoW-aware finality handling
    ///
    /// This subscription will emit events about new blocks, finalized blocks,
    /// and other chain updates. Unlike the standard chainHead_v1_follow,
    /// this handles large gaps between best and finalized blocks.
    #[subscription(
        name = "qpowChainHead_v1_follow",
        unsubscribe = "qpowChainHead_v1_unfollow",
        item = FollowEvent<Hash>
    )]
    async fn follow(&self, with_runtime: bool) -> SubscriptionResult;

    /// Get the body of a block
    #[method(name = "qpowChainHead_v1_body")]
    async fn body(&self, follow_subscription: String, hash: Hash) -> RpcResult<OperationId>;

    /// Get the header of a block
    #[method(name = "qpowChainHead_v1_header")]
    async fn header(&self, follow_subscription: String, hash: Hash) -> RpcResult<Option<String>>;

    /// Call a runtime API
    #[method(name = "qpowChainHead_v1_call")]
    async fn call(
        &self,
        follow_subscription: String,
        hash: Hash,
        function: String,
        _call_parameters: String,
    ) -> RpcResult<OperationId>;

    /// Get storage items
    #[method(name = "qpowChainHead_v1_storage")]
    async fn storage(
        &self,
        follow_subscription: String,
        hash: Hash,
        _items: Vec<StorageQuery>,
        _child_trie: Option<String>,
    ) -> RpcResult<OperationId>;

    /// Continue an operation
    #[method(name = "qpowChainHead_v1_continue")]
    async fn continue_operation(
        &self,
        follow_subscription: String,
        operation_id: OperationId,
    ) -> RpcResult<()>;

    /// Stop an operation
    #[method(name = "qpowChainHead_v1_stopOperation")]
    async fn stop_operation(
        &self,
        follow_subscription: String,
        operation_id: OperationId,
    ) -> RpcResult<()>;
}

/// Type alias for subscription result
pub type SubscriptionResult = Result<(), jsonrpsee::core::StringError>;

/// Operation ID for tracking async operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct OperationId(pub String);

/// Storage query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageQuery {
    /// The storage key
    pub key: String,
    /// The type of query
    #[serde(rename = "type")]
    pub query_type: StorageQueryType,
}

/// Storage query type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageQueryType {
    /// Get the value at the key
    Value,
    /// Get the hash of the value at the key
    Hash,
    /// Get all keys with the given prefix
    ClosestDescendantMerkleValue,
    /// Get all key-value pairs with the given prefix
    DescendantsValues,
    /// Get all key-hash pairs with the given prefix
    DescendantsHashes,
}

/// QPoW ChainHead RPC implementation
pub struct QpowChainHead<Client, Block: BlockT> {
    /// Substrate client
    client: Arc<Client>,
    /// Maximum number of ongoing subscriptions
    max_subscriptions: usize,
    /// Subscription manager
    subscriptions: Arc<subscription::SubscriptionManager<Block>>,
    /// Phantom data for Block
    _phantom: PhantomData<Block>,
}

impl<Client, Block: BlockT> QpowChainHead<Client, Block> {
    /// Create a new QpowChainHead RPC handler
    pub fn new(client: Arc<Client>, max_subscriptions: usize) -> Self {
        Self {
            client,
            max_subscriptions,
            subscriptions: Arc::new(SubscriptionManager::new()),
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<Client, Block, Hash> QpowChainHeadApiServer<Hash> for QpowChainHead<Client, Block>
where
    Block: BlockT<Hash = Hash>,
    Hash: Send + Sync + 'static + Serialize + std::fmt::Debug,
    Client: HeaderBackend<Block>
        + ProvideRuntimeApi<Block>
        + sc_client_api::BlockchainEvents<Block>
        + Send
        + Sync
        + 'static,
    Client::Api: sp_api::Core<Block>,
{
    async fn follow(
        &self,
        sink: PendingSubscriptionSink,
        with_runtime: bool,
    ) -> SubscriptionResult {
        // Check if we've reached max subscriptions
        if self.subscriptions.count() >= self.max_subscriptions {
            return Err(jsonrpsee::core::StringError::from(
                "Max subscriptions reached",
            ));
        }

        // Create a new subscription
        let subscription_id = self.subscriptions.create_subscription();

        // Accept the subscription
        let sink = match sink.accept().await {
            Ok(sink) => sink,
            Err(_) => {
                self.subscriptions.remove_subscription(&subscription_id);
                return Ok(());
            }
        };

        // Start the follow subscription handler
        let client = self.client.clone();
        let subscriptions = self.subscriptions.clone();

        jsonrpsee::tokio::spawn(async move {
            subscription::handle_follow_subscription(
                client,
                sink,
                subscription_id,
                with_runtime,
                subscriptions,
                QPOW_MAX_LAGGING_DISTANCE,
            )
            .await;
        });

        Ok(())
    }

    async fn body(&self, follow_subscription: String, hash: Hash) -> RpcResult<OperationId> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // TODO: Implement body retrieval
        let operation_id = OperationId(format!("body-{:?}", hash));
        Ok(operation_id)
    }

    async fn header(&self, follow_subscription: String, hash: Hash) -> RpcResult<Option<String>> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // Get the header
        let header = self.client.header(hash).map_err(|e| {
            ErrorObject::owned(-32000, format!("Failed to get header: {}", e), None::<()>)
        })?;

        // Serialize to hex
        if let Some(header) = header {
            use codec::Encode;
            Ok(Some(hex::encode(header.encode())))
        } else {
            Ok(None)
        }
    }

    async fn call(
        &self,
        follow_subscription: String,
        hash: Hash,
        function: String,
        call_parameters: String,
    ) -> RpcResult<OperationId> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // TODO: Implement runtime API call
        let _ = call_parameters; // Will be used when implementing actual call
        let operation_id = OperationId(format!("call-{:?}-{}", hash, function));
        Ok(operation_id)
    }

    async fn storage(
        &self,
        follow_subscription: String,
        hash: Hash,
        items: Vec<StorageQuery>,
        child_trie: Option<String>,
    ) -> RpcResult<OperationId> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // TODO: Implement storage queries
        let _ = items; // Will be used when implementing actual storage queries
        let _ = child_trie; // Will be used for child trie queries
        let operation_id = OperationId(format!("storage-{:?}", hash));
        Ok(operation_id)
    }

    async fn continue_operation(
        &self,
        follow_subscription: String,
        operation_id: OperationId,
    ) -> RpcResult<()> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // TODO: Implement operation continuation
        let _ = operation_id; // Will be used when implementing operation tracking
        Ok(())
    }

    async fn stop_operation(
        &self,
        follow_subscription: String,
        operation_id: OperationId,
    ) -> RpcResult<()> {
        // Verify subscription exists
        if !self.subscriptions.is_valid(&follow_subscription) {
            return Err(ErrorObject::owned(
                -32602,
                "Invalid follow subscription",
                None::<()>,
            ));
        }

        // TODO: Implement operation stopping
        let _ = operation_id; // Will be used when implementing operation tracking
        Ok(())
    }
}
