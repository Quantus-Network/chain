# QPoW ChainHead RPC Usage Examples

This document demonstrates how to use the QPoW-aware chainHead RPC methods designed for Resonance's Proof-of-Work consensus mechanism.

## Overview

The `qpowChainHead_v1_*` methods mirror the standard `chainHead_v1_*` API but are specifically adapted to handle Resonance's large finality gap (179 blocks). Unlike the standard chainHead implementation, these methods won't trigger stop events when the gap between the best block and finalized block exceeds typical thresholds.

## Key Differences from Standard chainHead_v1

1. **Max Lagging Distance**: Set to 200 blocks (vs substrate's default of ~5-10 blocks)
2. **Continuous Operation**: Won't stop the subscription due to large finality gaps
3. **PoW-Aware Events**: Events are structured with PoW finality in mind

## RPC Methods

### qpowChainHead_v1_follow

Start following the chain with PoW-aware finality handling.

```javascript
// JavaScript/TypeScript example for qapi-console
const subscription = await api.rpc.qpowChainHead.follow(true); // true = include runtime updates

subscription.on('data', (event) => {
  switch (event.event) {
    case 'initialized':
      console.log('Subscription initialized');
      console.log('Finalized block:', event.finalizedBlockHash);
      // Note: finalized block may be 179 blocks behind best!
      break;
      
    case 'newBlock':
      console.log('New block:', event.blockHash);
      console.log('Parent:', event.parentBlockHash);
      break;
      
    case 'bestBlockChanged':
      console.log('New best block:', event.bestBlockHash);
      break;
      
    case 'finalized':
      console.log('Blocks finalized:', event.finalizedBlockHashes);
      // In PoW, finalization happens in batches as blocks age
      break;
      
    case 'stop':
      console.log('Subscription stopped');
      // This should rarely happen with qpow methods
      break;
  }
});
```

### qpowChainHead_v1_header

Get the header of a specific block.

```javascript
const subscriptionId = 'qpow-xxxx-xxxx-xxxx'; // From follow subscription
const blockHash = '0x...';

const headerHex = await api.rpc.qpowChainHead.header(subscriptionId, blockHash);
if (headerHex) {
  console.log('Block header (hex):', headerHex);
}
```

### qpowChainHead_v1_body

Get the body (extrinsics) of a block.

```javascript
const operationId = await api.rpc.qpowChainHead.body(subscriptionId, blockHash);

// The operation runs asynchronously, results come through the follow subscription
// as operationBodyDone events
```

### qpowChainHead_v1_call

Call a runtime API method at a specific block.

```javascript
const operationId = await api.rpc.qpowChainHead.call(
  subscriptionId,
  blockHash,
  'Core_version',  // Runtime API method
  '0x'             // Parameters (empty for version)
);

// Results come through the follow subscription as operationCallDone events
```

### qpowChainHead_v1_storage

Query storage at a specific block.

```javascript
const queries = [
  {
    key: '0x26aa394eea5630e07c48ae0c9558cef7b99d880ec681799c0cf30e8886371da9', // System.Account prefix
    type: 'descendantsValues'
  }
];

const operationId = await api.rpc.qpowChainHead.storage(
  subscriptionId,
  blockHash,
  queries,
  null  // No child trie
);

// Results come through the follow subscription as operationStorageItems events
```

## Integration with qapi-console

To integrate these methods with qapi-console, you'll need to update the light client to use the qpow-prefixed methods:

```typescript
// In qapi-console's light client implementation
class ResonanceLightClient {
  async connect() {
    // Instead of using chainHead_v1_follow
    this.subscription = await this.api.rpc.qpowChainHead.follow(true);
    
    this.subscription.on('data', (event) => {
      this.handleChainEvent(event);
    });
  }
  
  handleChainEvent(event) {
    switch (event.event) {
      case 'initialized':
        // Handle the large gap between finalized and best
        console.log('Note: Finalized block may be up to 179 blocks behind best');
        this.updateFinalizedBlock(event.finalizedBlockHash);
        break;
        
      case 'finalized':
        // In PoW, multiple blocks may be finalized at once
        for (const hash of event.finalizedBlockHashes) {
          this.markBlockFinalized(hash);
        }
        break;
        
      // ... handle other events
    }
  }
}
```

## Handling the Finality Gap

When working with Resonance's PoW consensus, keep in mind:

1. **Finalized blocks lag significantly**: The finalized block is typically 179 blocks behind the best block
2. **Batch finalization**: Multiple blocks may be finalized in a single event
3. **State queries**: You can query state at any block, but only finalized blocks are guaranteed to not be reorganized

```javascript
// Example: Safely querying state
async function safeStateQuery(api, key) {
  const info = await api.rpc.chain.getInfo();
  const finalizedHash = info.finalizedHash;
  
  // Query at finalized block for guaranteed consistency
  const result = await api.rpc.qpowChainHead.storage(
    subscriptionId,
    finalizedHash,
    [{ key, type: 'value' }]
  );
  
  return result;
}
```

## Error Handling

The qpow methods include the same error cases as standard chainHead, but with adjusted thresholds:

```javascript
subscription.on('error', (error) => {
  if (error.message.includes('Max subscriptions reached')) {
    // Too many active subscriptions
    console.error('Cannot create more subscriptions');
  } else {
    console.error('Subscription error:', error);
  }
});
```

## Migration from chainHead_v1

To migrate from standard chainHead to qpow methods:

1. Replace all `chainHead_v1_` method calls with `qpowChainHead_v1_`
2. Adjust any assumptions about finality lag (expect up to 179 blocks)
3. Handle batch finalization events (multiple blocks at once)
4. Remove any workarounds for subscription stop events

```javascript
// Before
const sub = await api.rpc.chainHead.follow(true);

// After  
const sub = await api.rpc.qpowChainHead.follow(true);
```

## Performance Considerations

- The qpow methods handle larger block backlogs, so initial synchronization may take longer
- More blocks remain in the "unfinalized" state, increasing memory usage
- Finalization events may include more blocks at once, requiring batch processing

## Future Enhancements

Potential improvements to the qpow chainHead implementation:

1. Add PoW-specific metrics (difficulty, hash rate)
2. Include mining-related events
3. Optimize for PoW's different finalization pattern
4. Add methods for querying the reorganization buffer