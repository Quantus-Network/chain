# External Miner Protocol Specification

This document defines the QUIC-based protocol for communication between the Quantus Network node and an external QPoW miner service.

## Overview

The node delegates the mining task (finding a valid nonce) to an external miner service over a persistent QUIC connection. The node provides the necessary parameters (header hash, difficulty, nonce range) and the external miner searches for a valid nonce according to the QPoW rules defined in the `qpow-math` crate. The miner pushes the result back when found.

### Key Benefits of QUIC

- **Lower latency**: Results are pushed immediately when found (no polling)
- **Connection resilience**: Built-in connection migration and recovery
- **Multiplexed streams**: Multiple operations on single connection
- **Built-in TLS**: Encrypted by default

## Protocol Design

### Connection Model

```
┌─────────────────────────┐         QUIC Connection          ┌─────────────────────────┐
│   Blockchain Node       │◄═══════════════════════════════►│   External Miner        │
│   (QUIC Client)         │                                   │   (QUIC Server)         │
│                         │     Bidirectional Stream          │                         │
│   Sends: NewJob         │  ─────────────────────────────►  │   Receives: NewJob      │
│   Receives: JobResult   │  ◄─────────────────────────────  │   Sends: JobResult      │
└─────────────────────────┘                                   └─────────────────────────┘
```

- **Node** acts as the QUIC client, connecting to the miner
- **Miner** acts as the QUIC server, listening on port 9833 (default)
- Single bidirectional stream per connection
- Connection persists across multiple mining jobs

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
| `nonce_start` | String | Starting nonce (128 hex chars, no 0x prefix) |
| `nonce_end` | String | Ending nonce (128 hex chars, no 0x prefix) |

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
Node                                         Miner
  │                                            │
  │──── QUIC Connect ─────────────────────────►│
  │◄─── Connection Established ────────────────│
  │                                            │
  │──── NewJob { job_id: "abc", ... } ────────►│
  │                                            │ (starts mining)
  │                                            │
  │◄─── JobResult { job_id: "abc", ... } ──────│ (found solution!)
  │                                            │
  │     (node submits block, gets new work)    │
  │                                            │
  │──── NewJob { job_id: "def", ... } ────────►│
  │                                            │
```

### Job Cancellation (Implicit)

When a new block arrives before the miner finds a solution, the node simply sends a new `NewJob`. The miner automatically cancels the previous job:

```
Node                                         Miner
  │                                            │
  │──── NewJob { job_id: "abc", ... } ────────►│
  │                                            │ (mining "abc")
  │                                            │
  │     (new block arrives at node!)           │
  │                                            │
  │──── NewJob { job_id: "def", ... } ────────►│
  │                                            │ (cancels "abc", starts "def")
  │                                            │
  │◄─── JobResult { job_id: "def", ... } ──────│
```

### Stale Result Handling

If a result arrives for an old job, the node discards it:

```
Node                                         Miner
  │                                            │
  │──── NewJob { job_id: "abc", ... } ────────►│
  │                                            │
  │──── NewJob { job_id: "def", ... } ────────►│ (almost simultaneous)
  │                                            │
  │◄─── JobResult { job_id: "abc", ... } ──────│ (stale, node ignores)
  │                                            │
  │◄─── JobResult { job_id: "def", ... } ──────│ (current, node uses)
```

## Configuration

### Node

```bash
# Connect to external miner
quantus-node --external-miner-addr 127.0.0.1:9833
```

### Miner

```bash
# Start QUIC server
quantus-miner serve --quic-port 9833
```

## TLS Configuration

The miner generates a self-signed TLS certificate at startup. The node skips certificate verification by default (insecure mode). For production deployments, consider:

1. **Certificate pinning**: Configure the node to accept only specific certificate fingerprints
2. **Proper CA**: Use certificates signed by a trusted CA
3. **Network isolation**: Run node and miner on a private network

## Error Handling

### Connection Loss

The node automatically reconnects with exponential backoff:
- Initial delay: 1 second
- Maximum delay: 30 seconds

During reconnection, the node falls back to local mining if available.

### Validation Errors

If the miner receives an invalid `MiningRequest`, it sends a `JobResult` with status `failed`.

## Migration from HTTP

If you were using the previous HTTP-based protocol:

| Old (HTTP) | New (QUIC) |
|------------|------------|
| `--external-miner-url http://...` | `--external-miner-addr host:port` |
| `--port 9833` | `--quic-port 9833` |
| `POST /mine` | `MinerMessage::NewJob` |
| `GET /result/{id}` | Results pushed automatically |
| `POST /cancel/{id}` | Implicit (send new job) |

## Notes

- All hex values should be sent **without** the `0x` prefix
- The miner implements validation logic from `qpow_math::is_valid_nonce`
- The node uses the `work` field from `MiningResult` to construct `QPoWSeal`
- ALPN protocol identifier: `quantus-miner`
