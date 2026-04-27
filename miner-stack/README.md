# Quantus Node & External Miner - Docker Setup

Minimal Docker Compose setup for running a Quantus validator node together with the
external [`quantus-miner`](https://github.com/Quantus-Network/quantus-miner)
service.

## Architecture

```
┌──────────────────────────┐                  ┌──────────────────────────┐
│      quantus-node        │   QUIC :9833     │     quantus-miner        │
│   (QUIC server, mining   │ ◀──────────────  │   (QUIC client, computes │
│    job broadcaster)      │                  │    hashes on CPU/GPU)    │
└──────────────────────────┘                  └──────────────────────────┘
            ▲
            │ p2p/rpc/prometheus
            ▼
       Quantus network
```

Since `quantus-miner v1.x` and the QUIC-based external miner protocol, the
**node acts as the QUIC server** on `--miner-listen-port 9833` and one or
more **miners connect to it as clients**. When `--miner-listen-port` is set,
the node disables local mining and exclusively delegates work to connected
miners.

## Prerequisites

You need a wormhole inner hash (a 32-byte preimage). Mining rewards are sent to
the wormhole address derived from this preimage by the runtime, not directly
to your SS58 address.

Generate a wormhole keypair and copy the `inner_hash` value:

```bash
docker run --rm ghcr.io/quantus-network/quantus-node:latest \
  key quantus --scheme wormhole
```

The output contains:

- `Address` — your wormhole address (where rewards will be sent)
- `inner_hash` — your 32-byte preimage (use this as `REWARDS_INNER_HASH`)

**Save the mnemonic and inner_hash somewhere safe.** Without the inner_hash you
cannot prove ownership of the rewards.

## Quick Start

### 1. Configure

```bash
cp .env.example .env
nano .env
```

Edit at minimum:

- `REWARDS_INNER_HASH` — your wormhole preimage (`0x...`, 64 hex chars)
- `NODE_NAME` — your node name (visible in network)

### 2. Start

```bash
# Start node + miner
docker compose up -d

# Or, with monitoring (Prometheus + Grafana + node-exporter)
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d
```

### 3. Monitor

```bash
docker compose logs -f quantus-node
docker compose logs -f quantus-miner
```

If monitoring is enabled:

- **Grafana**: http://localhost:3000
  - Default login: `quantus` / `quantus` (change via `GRAFANA_USER` / `GRAFANA_PASSWORD` in `.env`)
- **Prometheus**: http://localhost:9090
- **Node Prometheus exporter**: http://localhost:9615/metrics
- **Miner Prometheus exporter**: http://localhost:9900/metrics

## Configuration

All configuration goes through `.env`:

```bash
# Required
REWARDS_INNER_HASH=0xyour_inner_hash_here
CHAIN=planck
NODE_NAME=my-quantus-node

# Optional - image versions
# NODE_VERSION=latest
# MINER_VERSION=latest

# Optional - mining workers
# CPU_WORKERS=4         # CPU worker threads (default: auto-detect)
# GPU_DEVICES=0         # GPU devices to use (0 = CPU-only)
# MINER_LOG=info        # RUST_LOG for the miner

# Optional - networking
# IN_PEERS=256
# OUT_PEERS=256

# Optional - host port mapping
# P2P_PORT=30333         # libp2p networking
# RPC_PORT=9944          # JSON-RPC
# PROMETHEUS_PORT=9615   # node Prometheus
# MINER_LISTEN_PORT=9833 # QUIC port for miner connections (UDP)
# MINER_METRICS_PORT=9900# miner Prometheus
```

> The node maps `9833/udp` to the host so that miners on other machines can
> connect. If you only run the bundled miner inside this Compose project, you
> can comment out that mapping — the miner reaches the node via the internal
> Docker network at `172.28.0.10:9833` (the node has a pinned IPv4 in the
> `quantus` bridge network because `quantus-miner` parses `--node-addr` as a
> `SocketAddr` and cannot resolve service names).

## Commands

### Basic (node + miner)

```bash
docker compose up -d        # Start
docker compose down         # Stop
docker compose logs -f      # Tail all logs
docker compose restart      # Restart all
docker compose ps           # Status
```

### With monitoring

```bash
# Start everything (node + miner + monitoring)
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d

# Stop everything
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml down

# Stop only monitoring (keep node + miner running)
docker compose -f docker-compose.monitoring.yml down
```

### Pull updated images

```bash
docker compose pull
docker compose up -d
```

## Layout

```
miner-stack/
├── docker-compose.yml             # Node + miner
├── docker-compose.monitoring.yml  # Prometheus + Grafana + node-exporter
├── init-node.sh                   # Generates node identity on first start
├── .env                           # Local config (gitignored)
├── .env.example                   # Reference config
├── node-keys/                     # Persistent node identity (BACK THIS UP)
│   └── key_node
├── node-data/                     # Chain data (safe to delete)
│   └── chains/
├── prometheus/
│   └── prometheus.yml             # Scrape config
└── grafana/
    ├── grafana.ini
    ├── provisioning/              # Datasources + dashboards provider
    └── dashboards/                # Bundled dashboards
        └── quantus-node/
            ├── overview.json
            ├── quantus-business.json
            ├── quantus-miner.json
            ├── quantus-node-metrics.json
            └── system-monitoring.json
```

`node-keys/` persists the node's libp2p identity. Back it up — losing it
means a new peer ID and a fresh sync.

## Bundled Dashboards

| Dashboard | Description |
|-----------|-------------|
| **Overview** | High-level snapshot: block height, peers, miner hash rate, host CPU & memory |
| **Quantus Business** | Chain-level QPoW metrics: chain height, block time EMA, last block duration, difficulty |
| **Quantus Miner Metrics** | Miner: total/CPU/GPU hash rate, active jobs, workers, GPU devices, cumulative hashes |
| **Quantus Node Metrics** | Substrate metrics: block height, peers, network traffic, task polling |
| **System Monitoring** | Host: CPU, memory, disk, network, load |

## Adjusting Miner Workload

The miner reads worker counts from environment variables (forwarded from
`.env`):

- `CPU_WORKERS` → `MINER_CPU_WORKERS` (number of CPU threads, default auto-detect)
- `GPU_DEVICES` → `MINER_GPU_DEVICES` (number of GPU devices, default `0`)
- `MINER_LOG` → `RUST_LOG` for the miner

Examples:

```bash
# 8 CPU workers, no GPU
echo "CPU_WORKERS=8" >> .env

# CPU + GPU hybrid (requires GPU-enabled image / drivers)
echo "CPU_WORKERS=4" >> .env
echo "GPU_DEVICES=1" >> .env

# Verbose miner logs
echo "MINER_LOG=debug" >> .env

docker compose up -d --force-recreate quantus-miner
```

> The default `ghcr.io/quantus-network/quantus-miner:latest` image is built
> without CUDA. For GPU mining on Linux you typically need a GPU-enabled
> build of the miner and the NVIDIA Container Toolkit. See the
> [`quantus-miner` repository](https://github.com/Quantus-Network/quantus-miner)
> for GPU build instructions.

## Troubleshooting

### Node sync status

```bash
docker compose logs quantus-node | grep -E "Syncing|Idle|Imported"
```

### Peer count

```bash
curl -s -H "Content-Type: application/json" \
  -d '{"id":1,"jsonrpc":"2.0","method":"system_peers"}' \
  http://localhost:9944 | jq '.result | length'
```

### Miner connectivity

The miner periodically retries the QUIC connection to the node. Check that:

```bash
docker compose logs quantus-miner | grep -E "connect|connected|node-addr|error"
```

If the miner cannot reach the node, ensure:
- Both containers are on the same `quantus` Docker network (default).
- The node started successfully and printed something like
  `External miner QUIC server listening on 0.0.0.0:9833`.

### Verify miner metrics

```bash
curl -s http://localhost:9900/metrics | grep -E "miner_(hash_rate|active_jobs|workers|cpu_workers|gpu_devices|hashes_total)"
```

You should see values for:

- `miner_hash_rate` (total H/s)
- `miner_cpu_hash_rate` / `miner_gpu_hash_rate`
- `miner_active_jobs` (0 or 1)
- `miner_cpu_workers` / `miner_gpu_devices` / `miner_workers` / `miner_effective_cpus`
- `miner_hashes_total` (counter)

### Port conflicts

Override ports in `.env` (see `MINER_LISTEN_PORT`, `MINER_METRICS_PORT`,
`P2P_PORT`, `RPC_PORT`, `PROMETHEUS_PORT`).

### Reset chain data

```bash
docker compose down
rm -rf node-data/
docker compose up -d
```

The node identity in `node-keys/` is preserved.

### Rotate / regenerate node identity

```bash
docker compose down
rm -rf node-keys/
docker compose up -d   # init-node.sh will regenerate key_node on startup
```

## Multiple Miners

You can connect additional miners (on the same host or another host) to the
node. The node broadcasts every job to all connected miners; whoever finds a
valid solution first wins. To run an extra miner from another host, expose
`MINER_LISTEN_PORT` (UDP) on the node's host and start a miner anywhere with:

```bash
docker run --rm --platform linux/amd64 \
  -p 9900:9900 \
  ghcr.io/quantus-network/quantus-miner:latest \
  serve \
  --node-addr <NODE_HOST>:9833 \
  --cpu-workers 4 \
  --metrics-port 9900
```

---

**Happy mining.**
