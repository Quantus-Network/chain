//! Subscription management for QPoW ChainHead
//!
//! This module handles the core subscription logic for the QPoW-aware chainHead RPC.
//! It manages subscription lifecycles, tracks blocks, and streams events to clients
//! while tolerating the large finality gaps inherent in PoW consensus.

use super::events::*;
use futures::{FutureExt, StreamExt};
use jsonrpsee::{SubscriptionMessage, SubscriptionSink};
use log::{debug, error, warn};
use sc_client_api::BlockchainEvents;
use sp_api::Core;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{
    traits::{Block as BlockT, Header as HeaderT, NumberFor},
    Saturating,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

/// Manages active subscriptions for the QPoW chainHead RPC
///
/// This manager tracks all active subscriptions and their associated data,
/// including which blocks each subscription is tracking and the last
/// finalized block sent to each subscriber.
pub struct SubscriptionManager<Block: BlockT> {
    /// Active subscriptions mapped by subscription ID
    subscriptions: Arc<Mutex<HashMap<String, SubscriptionData<Block>>>>,
}

/// Data associated with a subscription
///
/// Tracks the state of an individual subscription including which blocks
/// are being monitored and the last finalized block sent to the client.
struct SubscriptionData<Block: BlockT> {
    /// Currently tracked blocks that haven't been finalized or pruned
    tracked_blocks: HashSet<Block::Hash>,
    /// The last finalized block hash sent to this subscription
    last_finalized: Option<Block::Hash>,
}

impl<Block: BlockT> SubscriptionManager<Block> {
    /// Create a new subscription manager
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new subscription
    pub fn create_subscription(&self) -> String {
        let id = format!("qpow-{}", Uuid::new_v4());
        let data = SubscriptionData {
            tracked_blocks: HashSet::new(),
            last_finalized: None,
        };

        self.subscriptions.lock().unwrap().insert(id.clone(), data);
        id
    }

    /// Remove a subscription
    pub fn remove_subscription(&self, id: &str) {
        self.subscriptions.lock().unwrap().remove(id);
    }

    /// Check if a subscription is valid
    pub fn is_valid(&self, id: &str) -> bool {
        self.subscriptions.lock().unwrap().contains_key(id)
    }

    /// Get the number of active subscriptions
    pub fn count(&self) -> usize {
        self.subscriptions.lock().unwrap().len()
    }

    /// Add a tracked block to a subscription
    pub fn track_block(&self, subscription_id: &str, hash: Block::Hash) {
        if let Some(data) = self.subscriptions.lock().unwrap().get_mut(subscription_id) {
            data.tracked_blocks.insert(hash);
        }
    }

    /// Remove tracked blocks from a subscription
    pub fn untrack_blocks(&self, subscription_id: &str, hashes: &[Block::Hash]) {
        if let Some(data) = self.subscriptions.lock().unwrap().get_mut(subscription_id) {
            for hash in hashes {
                data.tracked_blocks.remove(hash);
            }
        }
    }

    /// Update the last finalized block for a subscription
    pub fn update_finalized(&self, subscription_id: &str, hash: Block::Hash) {
        if let Some(data) = self.subscriptions.lock().unwrap().get_mut(subscription_id) {
            data.last_finalized = Some(hash);
        }
    }
}

/// Handle a follow subscription for QPoW-aware chainHead
///
/// This function manages the lifecycle of a chainHead follow subscription,
/// streaming events about new blocks, finalization, and chain reorganizations.
/// Unlike the standard Substrate implementation, it continues operating even
/// with large gaps between the best and finalized blocks.
///
/// # Arguments
///
/// * `client` - The substrate client for blockchain operations
/// * `sink` - The subscription sink for sending events
/// * `subscription_id` - Unique identifier for this subscription
/// * `with_runtime` - Whether to include runtime version information
/// * `subscriptions` - The subscription manager
/// * `max_lagging_distance` - Maximum allowed gap between best and finalized
///
/// # Behavior
///
/// The function will:
/// 1. Send an initial `Initialized` event with the current finalized block
/// 2. Stream `NewBlock` events for each imported block
/// 3. Send `BestBlockChanged` when the best block changes
/// 4. Send `Finalized` events when blocks are finalized
/// 5. Continue operating even if finality gap exceeds `max_lagging_distance`
/// 6. Send a `Stop` event and clean up when the subscription ends
pub async fn handle_follow_subscription<Client, Block>(
    client: Arc<Client>,
    sink: SubscriptionSink,
    subscription_id: String,
    with_runtime: bool,
    subscriptions: Arc<SubscriptionManager<Block>>,
    max_lagging_distance: u32,
) where
    Block: BlockT,
    Client: HeaderBackend<Block>
        + BlockchainEvents<Block>
        + ProvideRuntimeApi<Block>
        + Send
        + Sync
        + 'static,
    Client::Api: sp_api::Core<Block>,
    NumberFor<Block>: From<u32> + std::cmp::PartialOrd,
{
    debug!(
        "Starting QPoW chainHead follow subscription: {}",
        subscription_id
    );

    // Get initial chain state
    let info = client.info();
    let finalized_hash = info.finalized_hash;
    let finalized_number = info.finalized_number;
    let best_hash = info.best_hash;
    let best_number = info.best_number;

    debug!(
        "Initial state - Finalized: #{} ({:?}), Best: #{} ({:?})",
        finalized_number, finalized_hash, best_number, best_hash
    );

    // Check the gap between best and finalized
    let gap = best_number.saturating_sub(finalized_number);
    debug!("Initial finality gap: {} blocks", gap);

    // Send initialized event
    let initialized = Initialized {
        finalized_block_hash: finalized_hash,
        finalized_block_runtime: if with_runtime {
            get_runtime_version(&client, finalized_hash)
        } else {
            None
        },
    };

    if let Err(e) = sink
        .send(
            SubscriptionMessage::from_json(&FollowEvent::<Block::Hash>::Initialized(initialized))
                .unwrap(),
        )
        .await
    {
        error!("Failed to send initialized event: {}", e);
        subscriptions.remove_subscription(&subscription_id);
        return;
    }

    // Track the finalized block
    subscriptions.track_block(&subscription_id, finalized_hash);
    subscriptions.update_finalized(&subscription_id, finalized_hash);

    // Import notification stream
    let mut import_stream = client
        .import_notification_stream()
        .take_while(|_| {
            let is_valid = subscriptions.is_valid(&subscription_id);
            futures::future::ready(is_valid)
        })
        .fuse();

    // Finality notification stream
    let mut finality_stream = client
        .finality_notification_stream()
        .take_while(|_| {
            let is_valid = subscriptions.is_valid(&subscription_id);
            futures::future::ready(is_valid)
        })
        .fuse();

    // Main event loop
    loop {
        futures::select! {
            // Handle new block imports
            maybe_notification = import_stream.next() => {
                match maybe_notification {
                    Some(notification) => {
                        let block_hash = notification.hash;
                        let parent_hash = *notification.header.parent_hash();
                        let block_number = *notification.header.number();

                        debug!("New block imported: #{} ({:?})", block_number, block_hash);

                        // Track this block
                        subscriptions.track_block(&subscription_id, block_hash);

                        // Check if we should send runtime update
                        // Note: substrate's BlockImportNotification doesn't have new_runtime field
                        // We'd need to compare with parent block's runtime to detect changes
                        let new_runtime = None;

                        // Send new block event
                        let new_block = NewBlock {
                            block_hash,
                            parent_block_hash: parent_hash,
                            new_runtime,
                        };

                        if let Err(e) = sink.send(SubscriptionMessage::from_json(&FollowEvent::<Block::Hash>::NewBlock(new_block)).unwrap()).await {
                            error!("Failed to send new block event: {}", e);
                            break;
                        }

                        // Check if this is the new best block
                        if notification.is_new_best {
                            let best_block_changed = BestBlockChanged { best_block_hash: block_hash };
                            if let Err(e) = sink.send(SubscriptionMessage::from_json(&FollowEvent::<Block::Hash>::BestBlockChanged(best_block_changed)).unwrap()).await {
                                error!("Failed to send best block changed event: {}", e);
                                break;
                            }
                        }

                        // Check finality gap (but don't stop subscription for large gaps)
                        let current_info = client.info();
                        let gap = current_info.best_number.saturating_sub(current_info.finalized_number);
                        let max_distance = NumberFor::<Block>::from(max_lagging_distance);
                        if gap > max_distance {
                            warn!(
                                "QPoW finality gap ({:?}) exceeds max_lagging_distance ({}), but continuing subscription",
                                gap, max_lagging_distance
                            );
                        }
                    }
                    None => {
                        debug!("Import stream ended");
                        break;
                    }
                }
            }

            // Handle finality updates
            maybe_finality = finality_stream.next() => {
                match maybe_finality {
                    Some(notification) => {
                        let finalized_hash = notification.hash;
                        let finalized_header = notification.header;
                        let finalized_number = *finalized_header.number();

                        debug!("Block finalized: #{} ({:?})", finalized_number, finalized_hash);

                        // Get all blocks that were finalized
                        let finalized_blocks = vec![finalized_hash];

                        // TODO: We should traverse back from the newly finalized block
                        // to the previously finalized block to get all newly finalized blocks.
                        // This would involve walking the chain backwards to find all blocks
                        // that were implicitly finalized by this notification.
                        // For now, we just report the single finalized block.

                        // Determine which blocks are now pruned.
                        // TODO: Implement proper pruned block detection by checking which
                        // tracked blocks are no longer part of the canonical chain after
                        // this finalization. This requires comparing the previous chain
                        // state with the new canonical chain.
                        let pruned_blocks = Vec::new();

                        // Update our tracking
                        subscriptions.update_finalized(&subscription_id, finalized_hash);
                        subscriptions.untrack_blocks(&subscription_id, &pruned_blocks);

                        // Send finalized event
                        let finalized = Finalized {
                            finalized_block_hashes: finalized_blocks,
                            pruned_block_hashes: pruned_blocks,
                        };

                        if let Err(e) = sink.send(SubscriptionMessage::from_json(&FollowEvent::<Block::Hash>::Finalized(finalized)).unwrap()).await {
                            error!("Failed to send finalized event: {}", e);
                            break;
                        }
                    }
                    None => {
                        debug!("Finality stream ended");
                        break;
                    }
                }
            }

            // Handle subscription timeout or cancellation
            _ = jsonrpsee::tokio::time::sleep(Duration::from_secs(60)).fuse() => {
                // Periodic check if subscription is still active
                if !subscriptions.is_valid(&subscription_id) {
                    debug!("Subscription {} no longer valid", subscription_id);
                    break;
                }
            }
        }
    }

    // Send stop event
    let _ = sink
        .send(SubscriptionMessage::from_json(&FollowEvent::<Block::Hash>::Stop).unwrap())
        .await;

    // Clean up subscription
    subscriptions.remove_subscription(&subscription_id);
    debug!(
        "QPoW chainHead follow subscription ended: {}",
        subscription_id
    );
}

/// Get runtime version information for a block
///
/// Retrieves the runtime version at a specific block hash, converting it
/// to the format expected by the chainHead RPC events.
///
/// # Returns
///
/// * `Some(RuntimeEvent)` if the runtime version could be retrieved
/// * `None` if there was an error accessing the runtime API
fn get_runtime_version<Client, Block>(
    client: &Arc<Client>,
    hash: Block::Hash,
) -> Option<RuntimeEvent>
where
    Block: BlockT,
    Client: ProvideRuntimeApi<Block>,
    Client::Api: sp_api::Core<Block>,
{
    match client.runtime_api().version(hash) {
        Ok(version) => Some(RuntimeEvent {
            spec: RuntimeVersionEvent {
                spec_name: version.spec_name.to_string(),
                impl_name: version.impl_name.to_string(),
                spec_version: version.spec_version,
                impl_version: version.impl_version,
                transaction_version: version.transaction_version,
                state_version: version.system_version,
            },
        }),
        Err(e) => {
            warn!("Failed to get runtime version: {:?}", e);
            None
        }
    }
}
