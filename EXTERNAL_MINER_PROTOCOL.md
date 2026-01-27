# External Miner Protocol Specification

This document defines the QUIC-based protocol for communication between the Quantus Network node and external QPoW miner services.

## Overview

The node delegates the mining task (finding a valid nonce) to external miner services over persistent QUIC connections. The node provides the necessary parameters (header hash, difficulty) and each external miner independently searches for a valid nonce according to the QPoW rules defined in the `qpow-math` crate. Miners push results back when found.

### Key Benefits of QUIC

- **Lower latency**: Results are pushed immediately when found (no polling)
- **Connection resilience**: Built-in connection migration and recovery
- **Multiplexed streams**: Multiple operations on single connection
- **Built-in TLS**: Encrypted by default

## Architecture

### Connection Model

```
                           ┌─────────────────────────────────┐
                           │            Node                 │
                           │   (QUIC Server on port 9833)    │
                           │                                 │
┌──────────┐               │  Broadcasts: NewJob             │
│  Miner 1 │ ──connect───► │  Receives: JobResult            │
└──────────┘               │                                 │
                           │  Supports multiple miners       │
┌──────────┐               │  First valid result wins        │
│  Miner 2 │ ──connect───► │                                 │
└──────────┘               └─────────────────────────────────┘
                           
┌──────────┐                         
│  Miner 3 │ ──connect───►           
└──────────┘                         
```

- **Node** acts as the QUIC server, listening on port 9833 (default)
- **Miners** act as QUIC clients, connecting to the node
- Single bidirectional stream per miner connection
- Connection persists across multiple mining jobs
- Multiple miners can connect simultaneously

### Multi-Miner Operation

When multiple miners are connected:
1. Node broadcasts the same `NewJob` to all connected miners
2. Each miner independently selects a random starting nonce
3. First miner to find a valid solution sends `JobResult`
4. Node uses the first valid result, ignores subsequent results for same job
5. New job broadcast implicitly cancels work on all miners

### Message Types

The protocol uses only **two message types**:

| Direction | Message | Description |
|-----------|---------|-------------|
| Node → Miner | `NewJob` | Submit a mining job (implicitly cancels any previous job) |
| Miner → Node | `JobResult` | Mining result (completed, failed, or cancelled) |

### Wire Format

Messages are length-prefixed JSON:

```
┌─────────────────┬─────────────────────────────────┐
│ Length (4 bytes)│ JSON payload (MinerMessage)     │
│ big-endian u32  │                                 │
└─────────────────┴─────────────────────────────────┘
```

Maximum message size: 16 MB

## Data Types

See the `quantus-miner-api` crate for the canonical Rust definitions.

### MinerMessage (Enum)

```rust
pub enum MinerMessage {
    NewJob(MiningRequest),
    JobResult(MiningResult),
}
```

### MiningRequest

| Field | Type | Description |
|-------|------|-------------|
| `job_id` | String | Unique identifier (UUID recommended) |
| `mining_hash` | String | Header hash (64 hex chars, no 0x prefix) |
| `distance_threshold` | String | Difficulty (U512 as decimal string) |

Note: Nonce range is not specified - each miner independently selects a random starting point.

### MiningResult

| Field | Type | Description |
|-------|------|-------------|
| `status` | ApiResponseStatus | Result status (see below) |
| `job_id` | String | Job identifier |
| `nonce` | Option<String> | Winning nonce (U512 hex, no 0x prefix) |
| `work` | Option<String> | Winning nonce as bytes (128 hex chars) |
| `hash_count` | u64 | Number of nonces checked |
| `elapsed_time` | f64 | Time spent mining (seconds) |

### ApiResponseStatus (Enum)

| Value | Description |
|-------|-------------|
| `completed` | Valid nonce found |
| `failed` | Nonce range exhausted without finding solution |
| `cancelled` | Job was cancelled (new job received) |
| `running` | Job still in progress (not typically sent) |

## Protocol Flow

### Normal Mining Flow

```
Miner                                        Node
  │                                            │
  │──── QUIC Connect ─────────────────────────►│
  │◄─── Connection Established ────────────────│
  │                                            │
  │◄─── NewJob { job_id: "abc", ... } ─────────│
  │                                            │
  │     (picks random nonce, starts mining)    │
  │                                            │
  │──── JobResult { job_id: "abc", ... } ─────►│ (found solution!)
  │                                            │
  │     (node submits block, gets new work)    │
  │                                            │
  │◄─── NewJob { job_id: "def", ... } ─────────│
  │                                            │
```

### Job Cancellation (Implicit)

When a new block arrives before the miner finds a solution, the node simply sends a new `NewJob`. The miner automatically cancels the previous job:

```
Miner                                        Node
  │                                            │
  │◄─── NewJob { job_id: "abc", ... } ─────────│
  │                                            │
  │     (mining "abc")                         │
  │                                            │
  │     (new block arrives at node!)           │
  │                                            │
  │◄─── NewJob { job_id: "def", ... } ─────────│
  │                                            │
  │     (cancels "abc", starts "def")          │
  │                                            │
  │──── JobResult { job_id: "def", ... } ─────►│
```

### Miner Connect During Active Job

When a miner connects while a job is active, it immediately receives the current job:

```
Miner (new)                                  Node
  │                                            │ (already mining job "abc")
  │──── QUIC Connect ─────────────────────────►│
  │◄─── Connection Established ────────────────│
  │◄─── NewJob { job_id: "abc", ... } ─────────│ (current job sent immediately)
  │                                            │
  │     (joins mining effort)                  │
```

### Stale Result Handling

If a result arrives for an old job, the node discards it:

```
Miner                                        Node
  │                                            │
  │◄─── NewJob { job_id: "abc", ... } ─────────│
  │                                            │
  │◄─── NewJob { job_id: "def", ... } ─────────│ (almost simultaneous)
  │                                            │
  │──── JobResult { job_id: "abc", ... } ─────►│ (stale, node ignores)
  │                                            │
  │──── JobResult { job_id: "def", ... } ─────►│ (current, node uses)
```

## Configuration

### Node

```bash
# Listen for external miner connections on port 9833
quantus-node --miner-listen-port 9833
```

### Miner

```bash
# Connect to node
quantus-miner serve --node-addr 127.0.0.1:9833
```

## TLS Configuration

The node generates a self-signed TLS certificate at startup. The miner skips certificate verification by default (insecure mode). For production deployments, consider:

1. **Certificate pinning**: Configure the miner to accept only specific certificate fingerprints
2. **Proper CA**: Use certificates signed by a trusted CA
3. **Network isolation**: Run node and miner on a private network

## Error Handling

### Connection Loss

The miner automatically reconnects with exponential backoff:
- Initial delay: 1 second
- Maximum delay: 30 seconds

The node continues operating with remaining connected miners.

### Validation Errors

If the miner receives an invalid `MiningRequest`, it sends a `JobResult` with status `failed`.

## Notes

- All hex values should be sent **without** the `0x` prefix
- The miner implements validation logic from `qpow_math::is_valid_nonce`
- The node uses the `work` field from `MiningResult` to construct `QPoWSeal`
- ALPN protocol identifier: `quantus-miner`
- Each miner independently generates a random nonce starting point using cryptographically secure randomness
- With a 512-bit nonce space, collision between miners is statistically impossible
