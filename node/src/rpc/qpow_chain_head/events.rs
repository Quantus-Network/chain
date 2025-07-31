//! Event types for QPoW ChainHead subscriptions

use serde::{Deserialize, Serialize};

/// Follow subscription event
/// Events emitted by chainHead follow subscription
/// 
/// This enum represents all possible events that can be sent to a client
/// during a qpowChainHead_v1_follow subscription. Each variant corresponds
/// to a specific type of blockchain update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "event")]
pub enum FollowEvent<Hash> {
    /// The subscription was initialized with the current state
    /// The subscription was initialized
    /// 
    /// Sent as the first event after a successful subscription.
    /// Contains the current finalized block information.
    Initialized(Initialized<Hash>),
    
    /// A new block was imported
    /// 
    /// Sent whenever a new block is imported into the chain,
    /// regardless of whether it becomes the new best block.
    NewBlock(NewBlock<Hash>),
    
    /// The best block changed
    /// 
    /// Sent when a different block becomes the new best block.
    /// This can happen due to forks being resolved or new blocks
    /// extending the best chain.
    BestBlockChanged(BestBlockChanged<Hash>),
    
    /// Blocks were finalized
    /// 
    /// Sent when one or more blocks achieve finality.
    /// In PoW systems, this happens when blocks are sufficiently
    /// deep in the chain (179 blocks in Resonance).
    Finalized(Finalized<Hash>),
    
    /// The subscription has stopped
    /// 
    /// Terminal event indicating the subscription has ended.
    /// No further events will be sent after this.
    Stop,
}

/// Initialized event data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Initialized<Hash> {
    /// The current finalized block hash
    pub finalized_block_hash: Hash,
    /// The current finalized block runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_block_runtime: Option<RuntimeEvent>,
}

/// Runtime event information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvent {
    /// The runtime version
    pub spec: RuntimeVersionEvent,
}

/// Runtime version information
/// 
/// Detailed runtime version information matching Substrate's RuntimeVersion
/// structure. Used to track runtime upgrades and compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVersionEvent {
    /// Runtime spec name
    /// 
    /// Identifies the runtime specification. Changes indicate
    /// incompatible runtime upgrades.
    pub spec_name: String,
    
    /// Runtime implementation name
    /// 
    /// Identifies the runtime implementation. Typically includes
    /// the client name that built the runtime.
    pub impl_name: String,
    
    /// Runtime spec version
    /// 
    /// Version of the runtime specification. Incremented on
    /// breaking changes to the runtime API.
    pub spec_version: u32,
    
    /// Runtime implementation version
    /// 
    /// Version of the runtime implementation. Can change without
    /// breaking compatibility.
    pub impl_version: u32,
    
    /// Runtime transaction version
    /// 
    /// Version of the transaction format. Incremented when the
    /// transaction format changes in a breaking way.
    pub transaction_version: u32,
    
    /// Runtime state version
    /// 
    /// Version of the state representation. Currently corresponds
    /// to the system_version field in Substrate's RuntimeVersion.
    pub state_version: u8,
}

/// New block event data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewBlock<Hash> {
    /// The block hash
    /// 
    /// Hash of the newly imported block.
    pub block_hash: Hash,
    
    /// Parent block hash
    /// 
    /// Hash of this block's parent. Used to determine the block's
    /// position in the chain tree.
    pub parent_block_hash: Hash,
    
    /// New runtime if it changed
    /// 
    /// Present only if this block includes a runtime upgrade
    /// and `with_runtime` was true in the follow request.
    pub new_runtime: Option<RuntimeEvent>,
}

/// Best block changed event
/// 
/// Emitted when the best block changes, indicating a new chain tip.
/// This can happen when a new block extends the best chain or when
/// a fork becomes the new best chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BestBlockChanged<Hash> {
    /// New best block hash
    /// 
    /// The hash of the block that is now considered the best
    /// (highest weighted) block in the chain.
    pub best_block_hash: Hash,
}

/// Finalized event data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finalized<Hash> {
    /// List of finalized block hashes
    pub finalized_block_hashes: Vec<Hash>,
    /// Pruned block hashes (blocks that are no longer available)
    pub pruned_block_hashes: Vec<Hash>,
}

/// Operation event for async operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "result")]
pub enum OperationEvent {
    /// Operation is still in progress
    OperationContinue(OperationContinue),
    /// Operation completed with body
    OperationBodyDone(OperationBodyDone),
    /// Operation completed with call result
    OperationCallDone(OperationCallDone),
    /// Operation completed with storage items
    OperationStorageItems(OperationStorageItems),
    /// Operation completed
    OperationStorageDone,
    /// Operation resulted in an error
    OperationError(OperationError),
}

/// Operation continue event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationContinue {
    /// Operation ID
    pub operation_id: String,
}

/// Operation body done event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationBodyDone {
    /// Operation ID
    pub operation_id: String,
    /// The block body as hex-encoded array of extrinsics
    pub value: Vec<String>,
}

/// Operation call done event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationCallDone {
    /// Operation ID
    pub operation_id: String,
    /// The result of the runtime call as hex-encoded bytes
    pub output: String,
}

/// Operation storage items event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationStorageItems {
    /// Operation ID
    pub operation_id: String,
    /// Storage items
    pub items: Vec<StorageResult>,
}

/// Storage result item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageResult {
    /// The storage key
    pub key: String,
    /// The result value (depends on query type)
    #[serde(flatten)]
    pub result: StorageResultValue,
}

/// Storage result value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum StorageResultValue {
    /// Value result
    Value { value: Option<String> },
    /// Hash result
    Hash { hash: Option<String> },
    /// Merkle value result
    ClosestDescendantMerkleValue { 
        merkle_value: Option<String> 
    },
}

/// Operation error event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    /// Operation ID
    pub operation_id: String,
    /// Error message
    pub error: String,
}

/// Stop event error types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopError {
    /// The maximum distance between the finalized and latest block was exceeded
    MaxLaggingDistanceExceeded,
    /// Internal error occurred
    InternalError { message: String },
}