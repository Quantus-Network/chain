# Quantus Network Mining Guide

Get started mining on the Quantus Network testnet in minutes.

## Table of Contents

### Getting Started
- [Important: Wormhole Address System](#important-wormhole-address-system)
- [System Requirements](#system-requirements)
- [Setup](#setup)
  - [Manual Installation](#manual-installation)
  - [Docker Installation](#docker-installation)
- [External Miner Setup](#external-miner-setup)

### Configuration & Monitoring
- [Configuration Options](#configuration-options)
- [Monitoring Your Node](#monitoring-your-node)
- [Testnet Information](#testnet-information)

### Support & Best Practices
- [Troubleshooting](#troubleshooting)
- [Mining Economics](#mining-economics)
- [Security Best Practices](#security-best-practices)
- [Next Steps](#next-steps)

### Technical Documentation (For Developers)
- [External Miner Protocol Specification](#external-miner-protocol-specification)
  - [Overview](#overview)
  - [Architecture](#architecture)
  - [Data Types](#data-types)
  - [Protocol Flow](#protocol-flow)
  - [Configuration](#configuration-1)
  - [TLS Configuration](#tls-configuration)
  - [Error Handling](#error-handling)
  - [Implementation Notes](#implementation-notes)

---

> **Note:** This guide contains both practical setup instructions for miners and technical protocol specifications for developers. If you're building custom miner implementations, see the [External Miner Protocol Specification](#external-miner-protocol-specification) section at the end.

## Important: Wormhole Address System

**⚠️ Mining rewards are automatically sent to wormhole addresses derived from your preimage.**

- You provide a 32-byte preimage when starting mining
- The system derives your wormhole address using Poseidon hashing
- All mining rewards are sent to this derived wormhole address
- This ensures all miners use privacy-preserving wormhole addresses

## System Requirements

### Minimum Requirements
- **CPU**: 2+ cores
- **RAM**: 4GB
- **Storage**: 100GB available space
- **Network**: Stable internet connection (3+ Mbps)
- **OS**: Linux (Ubuntu 20.04+), macOS (10.15+), or Windows WSL2

> ⚠️ Connections below 3 Mbps will likely fail to keep the node synced with the network.

### Recommended Requirements
- **CPU**: 4+ cores (higher core count improves mining performance - coming soon)
- **RAM**: 8GB+
- **Storage**: 500GB+ SSD
- **Network**: Broadband connection (10+ Mbps)

## Setup

### Manual Installation

If you prefer manual installation or the script doesn't work for your system:

1. **Download Binary**

   Get the latest binary [GitHub Releases](https://github.com/Quantus-Network/chain/releases/latest)

2. **Generate Node Identity**
   ```bash
   ./quantus-node key generate-node-key --file ~/.quantus/node_key.p2p
   ```

3. **Generate Wormhole Address & Preimage**
   
   ```bash
   ./quantus-node key quantus --scheme wormhole
   ```
   
   This generates a wormhole key pair and shows:
   - `Address`: Your wormhole address (where rewards will be sent)
   - `inner_hash`: Your 32-byte preimage (use this for mining)
   
   **Save the preimage** - you'll need it for the `--rewards-address` parameter.

4. **Run the node (Planck network)**

Minimal command - see --help for many more options
```sh
./quantus-node \
    --validator \
    --chain planck \
    --node-key-file ~/.quantus/node_key.p2p \
    --rewards-inner-hash <YOUR_PREIMAGE_FROM_STEP_3> \
    --max-blocks-per-request 64 \
    --sync full
```

**Note:** Use the `inner_hash` from step 3 as your `--rewards-inner-hash`. The node will derive your wormhole address and log it on startup.
### Docker Installation

For users who prefer containerized deployment or have only Docker installed:

#### Quick Start with Docker

Follow these steps to get your Docker-based validator node running:

**Step 1: Prepare a Local Directory for Node Data**

Create a dedicated directory on your host machine to store persistent node data, such as your P2P key and rewards address file.
```bash
mkdir -p ./quantus_node_data
```
This command creates a directory named `quantus_node_data` in your current working directory.

**Optional Linux**
On linux you may need to make sure this directory has generous permissions so Docker can access it

```bash
chmod 755 quantus_node_data
```

**Step 2: Generate Your Node Identity (P2P Key)**

Your node needs a unique P2P identity to connect to the network. Generate this key into your data directory:
```bash
# If on Apple Silicon, you may need to add --platform linux/amd64
docker run --rm --platform linux/amd64 \
  -v "$(pwd)/quantus_node_data":/var/lib/quantus_data_in_container \
  ghcr.io/quantus-network/quantus-node:latest \
  key generate-node-key --file /var/lib/quantus_data_in_container/node_key.p2p
```
Replace `quantus-node:v0.0.4` with your desired image (e.g., `ghcr.io/quantus-network/quantus-node:latest`).
This command saves `node_key.p2p` into your local `./quantus_node_data` directory.

**Step 3: Generate Your Wormhole Address**

```bash
# If on Apple Silicon, you may need to add --platform linux/amd64
docker run --rm ghcr.io/quantus-network/quantus-node:latest key quantus --scheme wormhole
```

This generates a wormhole key pair. Save the `inner_hash` value - this is your preimage for mining.

**Step 4: Run the Validator Node**

```bash
# If on Apple Silicon, you may need to add --platform linux/amd64
# Replace YOUR_PREIMAGE with the inner_hash from step 3
docker run -d \
  --name quantus-node \
  --restart unless-stopped \
  -v "$(pwd)/quantus_node_data":/var/lib/quantus \
  -p 30333:30333 \
  -p 9944:9944 \
  ghcr.io/quantus-network/quantus-node:latest \
  --validator \
  --base-path /var/lib/quantus \
  --chain planck \
  --node-key-file /var/lib/quantus/node_key.p2p \
  --rewards-inner-hash <YOUR_PREIMAGE>
```

*Note for Apple Silicon (M1/M2/M3) users:* As mentioned above, if you are using an `amd64` based Docker image on an ARM-based Mac, you will likely need to add the `--platform linux/amd64` flag to your `docker run` commands.

Your node should now be starting up! You can check its logs using `docker logs -f quantus-node`.

#### Docker Management Commands

**View logs**
```bash
docker logs -f quantus-node
```

**Stop node**
```bash
docker stop quantus-node
```

**Start node again**
```bash
docker start quantus-node
```

**Remove container**
```bash
docker stop quantus-node && docker rm quantus-node
```

#### Updating Your Docker Node

When a new version is released:

```bash
# Stop and remove current container
docker stop quantus-node && docker rm quantus-node

# Pull latest image
docker pull ghcr.io/quantus-network/quantus-node:latest

# Start new container (data is preserved in ~/.quantus)
./run-node.sh --mode validator --rewards YOUR_ADDRESS_HERE
```

#### Docker-Specific Configuration

**Custom data directory**
```bash
./run-node.sh --data-dir /path/to/custom/data --name "my-node"
```

**Specific version**
```bash
docker run -d \
  --name quantus-node \
  --restart unless-stopped \
  -p 30333:30333 \
  -p 9944:9944 \
  -v ~/.quantus:/var/lib/quantus \
  ghcr.io/quantus-network/quantus-node:latest \
  --validator \
  --base-path /var/lib/quantus \
  --chain planck \
  --rewards-address YOUR_ADDRESS_HERE
```

**Docker system requirements**
- Docker 20.10+ or compatible runtime
- All other system requirements same as binary installation

## External Miner Setup

For high-performance mining, you can offload the mining process to a separate service, freeing up node resources.

### Prerequisites

1. **Build Node:**
   ```bash
   # From workspace root
   cargo build --release -p quantus-node
   ```

2. **Get External Miner:**
   ```bash
   git clone https://github.com/Quantus-Network/quantus-miner
   cd quantus-miner
   cargo build --release
   ```

### Setup with Wormhole Addresses

1. **Generate Your Wormhole Address**:
   ```bash
   ./quantus-node key quantus --scheme wormhole
   ```
   Save the `inner_hash` value.

2. **Start External Miner** (in separate terminal):
   ```bash
   RUST_LOG=info ./target/release/quantus-miner
   ```
   *(Default: `http://127.0.0.1:9833`)*

3. **Start Node with External Miner** (in another terminal):
   ```bash
   # Replace <YOUR_PREIMAGE> with the inner_hash from step 1
   RUST_LOG=info,sc_consensus_pow=debug ./target/release/quantus-node \
    --validator \
    --chain planck \
    --external-miner-url http://127.0.0.1:9833 \
    --rewards-inner-hash <YOUR_PREIMAGE>
   ```

For developers building custom miner implementations, see the [External Miner Protocol Specification](#external-miner-protocol-specification) section below.

## Configuration Options

### Node Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `--node-key-file` | Path to P2P identity file | Required |
| `--rewards-inner-hash` | Wormhole preimage (inner_hash from key generation) | Required |
| `--chain` | Chain specification | `planck` |
| `--port` | P2P networking port | `30333` |
| `--prometheus-port` | Metrics endpoint port | `9616` |
| `--name` | Node display name | Auto-generated |
| `--base-path` | Data directory | `~/.local/share/quantus-node` |



## Monitoring Your Node

### Check Node Status

**View Logs**
```bash
# Real-time logs
tail -f ~/.local/share/quantus-node/chains/planck/network/quantus-node.log

# Or run with verbose logging
RUST_LOG=info quantus-node [options]
```

**Prometheus Metrics**
Visit `http://localhost:9616/metrics` to view detailed node metrics.

**RPC Endpoint**
Use the RPC endpoint at `http://localhost:9944` to query blockchain state:

```bash
# Check latest block
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"chain_getBlock","params":[]}' \
  http://localhost:9944
```

### Check Mining Rewards

**View Balance at Your Wormhole Address**
```bash
# Replace YOUR_WORMHOLE_ADDRESS with your wormhole address from key generation
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"faucet_getAccountInfo","params":["YOUR_WORMHOLE_ADDRESS"]}' \
  http://localhost:9944
```

**Find Your Wormhole Address**
- From key generation: Use the `Address` field from `./quantus-node key quantus --scheme wormhole`
- From node logs: Check startup logs for "Mining rewards will be sent to wormhole address"

## Testnet Information

- **Chain**: Planck network
- **Consensus**: Quantum Proof of Work (QPoW)
- **Block Time**: ~6 seconds target
- **Network Explorer**: Coming soon
- **Faucet**: See Telegram

## Troubleshooting

### Common Issues

**Port Already in Use**
```bash
# Use different ports
quantus-node --port 30334 --prometheus-port 9617 [other options]
```

**Database Corruption**
```bash
# Purge and resync
quantus-node purge-chain --chain planck
```

**Mining Not Working**
1. Check that `--validator` flag is present
2. Verify your preimage from `inner_hash` field in key generation
3. Ensure node is synchronized (check logs for "Imported #XXXX")

**Wormhole Address Issues**
1. **Can't find rewards**: Check the `Address` field from your key generation
2. **Invalid preimage**: Use the exact `inner_hash` value from key generation  
3. **Wrong address**: Rewards go to the wormhole address, not the preimage

**Connection Issues**
1. Check firewall settings (allow port 30333)
2. Verify internet connection
3. Try different bootnodes if connectivity problems persist

### Getting Help

- **GitHub Issues**: [Report bugs and issues](https://github.com/Quantus-Network/chain/issues)
- **Discord**: [Join our community](#) (link coming soon)
- **Documentation**: [Technical docs](https://github.com/Quantus-Network/chain/blob/main/README.md)

### Logs and Diagnostics

**Enable Debug Logging**
```bash
RUST_LOG=debug,sc_consensus_pow=trace quantus-node [options]
```

**Export Node Info**
```bash
# Node identity
quantus-node key inspect-node-key --file ~/.quantus/node_key.p2p

# Network info
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"system_networkState","params":[]}' \
  http://localhost:9944
```

## Mining Economics

### Wormhole Address Rewards System

- **Automatic Wormhole Addresses**: All mining rewards go to your wormhole address
- **Privacy by Design**: Your reward address is derived from your preimage
- **Block Rewards**: Earned by successfully mining blocks  
- **Transaction Fees**: Collected from transactions in mined blocks
- **Network Incentives**: Additional rewards for network participation

### Expected Performance

Mining performance depends on:
- CPU performance (cores and clock speed)
- Network latency to other nodes
- Node synchronization status
- Competition from other miners

## Security Best Practices

### Key Management

- **Backup Your Keys**: Securely store your wormhole key pair from key generation
- **Backup Node Keys**: Store copies of your node identity keys safely  
- **Secure Storage**: Keep preimages and private keys in encrypted storage

### Node Security

- **Firewall**: Only expose necessary ports (30333 for P2P)
- **Updates**: Keep your node binary updated
- **Monitoring**: Watch for unusual network activity or performance

### Testnet Disclaimer

This is testnet software for testing purposes only:
- Tokens have no monetary value
- Network may be reset periodically
- Expect bugs and breaking changes
- Do not use for production workloads

## Next Steps

1. **Join the Community**: Connect with other miners and developers
2. **Monitor Performance**: Track your mining efficiency and rewards at your wormhole address
3. **Experiment**: Try different configurations and optimizations  
4. **Contribute**: Help improve the network by reporting issues and feedback

Happy mining! 🚀

---

# External Miner Protocol Specification

This section defines the QUIC-based protocol for communication between the Quantus Network node and external miner services. **This is technical documentation for developers building custom miner implementations.**

## Overview

The node delegates the mining task (finding a valid nonce) to external miner services over persistent QUIC connections. The node provides the necessary parameters (header hash, difficulty) and each external miner independently searches for a valid nonce according to the PoW rules defined in the `qpow-math` crate (using Poseidon2 hash function). Miners push results back when found.

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

The protocol uses **three message types**:

| Direction | Message | Description |
|-----------|---------|-------------|
| Miner → Node | `Ready` | Sent immediately after connecting to establish the stream |
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
    Ready,                      // Miner → Node: establish stream
    NewJob(MiningRequest),      // Node → Miner: submit job
    JobResult(MiningResult),    // Miner → Node: return result
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
| `miner_id` | Option<u64> | Miner ID (set by node, not miner) |

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
  │──── Ready ────────────────────────────────►│ (establish stream)
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
  │                                            │
  │──── Ready ────────────────────────────────►│ (establish stream)
  │                                            │
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

## Implementation Notes

- All hex values should be sent **without** the `0x` prefix
- The miner implements validation logic from `qpow_math::is_valid_nonce`
- The node uses the `work` field from `MiningResult` to construct `QPoWSeal`
- ALPN protocol identifier: `quantus-miner`
- Each miner independently generates a random nonce starting point using cryptographically secure randomness
- With a 512-bit nonce space, collision between miners is statistically impossible

---

*For technical support and updates, visit the [Quantus Network GitHub repository](https://github.com/Quantus-Network/chain).*
