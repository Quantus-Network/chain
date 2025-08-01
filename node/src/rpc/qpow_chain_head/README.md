# QPoW ChainHead RPC Implementation

## Overview

The `qpow_chain_head` module provides Proof-of-Work aware chainHead RPC methods for the Resonance blockchain. This custom implementation addresses compatibility issues with Substrate's default `chainHead_v1` RPC methods when used with PoW consensus systems that have large finality gaps.

## Background

### The Problem

Resonance uses a PoW consensus mechanism with the following characteristics:
- `MaxReorgDepth` of 180 blocks
- Blocks are finalized 179 blocks behind the best block
- This large gap between best and finalized blocks causes the default Substrate `chainHead_v1` implementation to stop subscriptions

The standard Substrate implementation considers such large gaps as abnormal and terminates subscriptions to prevent resource exhaustion, which is appropriate for PoS/GRANDPA systems but not for PoW systems.

### The Solution

Rather than modifying core Substrate code or reducing security parameters, we've implemented custom RPC methods that:
- Mirror the standard `chainHead_v1` API
- Use the `qpowChainHead_v1_*` prefix instead of `chainHead_v1_*`
- Handle large finality gaps gracefully without stopping subscriptions
- Maintain full compatibility with the expected chainHead interface

## Architecture

### Module Structure

```
qpow_chain_head/
├── mod.rs          # Main module with RPC trait definition and implementation
├── api.rs          # API exports
├── events.rs       # Event types for subscriptions
├── subscription.rs # Subscription management logic
└── tests/          # Unit tests
    └── mod.rs
```

### Key Components

1. **QpowChainHeadApi Trait**: Defines the RPC interface with methods:
   - `follow` - Subscribe to chain updates
   - `body` - Get block body
   - `header` - Get block header
   - `call` - Execute runtime API calls
   - `storage` - Query storage
   - `continue` - Continue paginated operations
   - `stopOperation` - Cancel ongoing operations

2. **SubscriptionManager**: Manages active subscriptions, tracking:
   - Subscription IDs
   - Tracked blocks per subscription
   - Last finalized block per subscription

3. **Event Types**: Mirrors standard chainHead events:
   - `Initialized` - Sent when subscription starts
   - `NewBlock` - Notifies about new blocks
   - `BestBlockChanged` - When the best block changes
   - `Finalized` - When blocks are finalized
   - `Stop` - When subscription ends

### Configuration

- `QPOW_MAX_LAGGING_DISTANCE`: Set to 200 blocks (higher than the 179-block finality gap)
- Default max subscriptions: 100 (configurable in `rpc.rs`)

## Usage

### Frontend Integration

Update your frontend to use the new RPC methods:

```javascript
// Instead of:
await api.rpc.chainHead.v1.follow(withRuntime);

// Use:
await api.rpc.qpowChainHead.v1.follow(withRuntime);
```

### RPC Method Mapping

| Standard Method | QPoW Method |
|----------------|-------------|
| `chainHead_v1_follow` | `qpowChainHead_v1_follow` |
| `chainHead_v1_unfollow` | `qpowChainHead_v1_unfollow` |
| `chainHead_v1_body` | `qpowChainHead_v1_body` |
| `chainHead_v1_header` | `qpowChainHead_v1_header` |
| `chainHead_v1_call` | `qpowChainHead_v1_call` |
| `chainHead_v1_storage` | `qpowChainHead_v1_storage` |
| `chainHead_v1_continue` | `qpowChainHead_v1_continue` |
| `chainHead_v1_stopOperation` | `qpowChainHead_v1_stopOperation` |

## Implementation Details

### Subscription Lifecycle

1. Client calls `qpowChainHead_v1_follow`
2. Server creates a unique subscription ID (format: `qpow-{uuid}`)
3. Server sends `Initialized` event with current finalized block
4. Server streams events for:
   - New block imports
   - Best block changes
   - Finalization updates
5. Large finality gaps trigger warnings but don't stop the subscription
6. Subscription ends when client disconnects or calls unfollow

### Key Differences from Standard Implementation

1. **No Stop on Large Gaps**: Continues operating even with gaps > 200 blocks
2. **PoW-Aware Logging**: Warns about large gaps without treating them as errors
3. **Simplified Runtime Detection**: Currently doesn't detect runtime upgrades on new blocks
4. **Subscription ID Format**: Uses `qpow-` prefix for easy identification

## Future Improvements

1. **Implement Missing Features**:
   - Complete body retrieval implementation
   - Runtime API call execution
   - Storage query operations
   - Operation continuation/cancellation

2. **Performance Optimizations**:
   - Add caching for frequently accessed data
   - Implement proper pagination for large result sets

3. **Enhanced Monitoring**:
   - Add metrics for subscription health
   - Track finality gap trends

4. **Runtime Upgrade Detection**:
   - Compare runtime versions between blocks
   - Send runtime update events when changes detected

## Testing

Unit tests are provided in `tests/mod.rs` covering:
- Subscription manager operations
- Event serialization
- Multiple subscription handling
- Block tracking/untracking

To run tests:
```bash
cargo test --package quantus-node qpow_chain_head::tests
```

## Migration Guide

For existing applications using `chainHead_v1`:

1. Update RPC client to support `qpowChainHead_v1_*` methods
2. Replace method calls with qpow equivalents
3. No changes needed to event handling - same format
4. Monitor logs for finality gap warnings

## Security Considerations

- The large finality gap is by design for PoW security
- Subscriptions continue operating under high reorg risk
- Clients should handle potential deep reorganizations
- Monitor resource usage with many active subscriptions

## Maintenance

This implementation should be reviewed when:
- Upgrading Substrate versions
- Changing consensus parameters
- Adding new chainHead features
- Optimizing subscription performance