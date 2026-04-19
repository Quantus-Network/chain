# Quantus Node & Miner - Docker Setup

Minimal Docker Compose setup for running a Quantus node with external miner.

## 🚀 Quick Start

### 1. Configure

```bash
cp .env.example .env
nano .env
```

Edit these values:
- `REWARDS_ADDRESS` - Your SS58 address for mining rewards
- `NODE_NAME` - Your node name (visible in network)

### 2. Start

```bash
# Start node and miner
docker compose up -d

# Optional: Start with monitoring (Prometheus + Grafana)
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d
```

### 3. Monitor

```bash
# Check logs
docker compose logs -f quantus-node
docker compose logs -f quantus-miner
```

**If monitoring is enabled**, access dashboards:

- **Grafana** (metrics visualization): http://localhost:3000
  - Default login: `quantus` / `quantus`
  - Change credentials in `.env` via `GRAFANA_USER` and `GRAFANA_PASSWORD`
  
- **Prometheus** (metrics storage): http://localhost:9090

## 📋 Configuration

### Essential Settings

Edit `.env` file:

```bash
# Required
REWARDS_ADDRESS=your_ss58_address_here
CHAIN=planck
NODE_NAME=my-quantus-node
```

### Optional Settings

```bash
NODE_VERSION=v0.4.2
MINER_VERSION=v1.0.0

# Miner workers (default: auto-detect)
WORKERS=4

# Network settings
IN_PEERS=256
OUT_PEERS=256

# Ports
P2P_PORT=30333
RPC_PORT=9944
PROMETHEUS_PORT=9615

# Grafana credentials (only if using monitoring)
GRAFANA_USER=quantus
GRAFANA_PASSWORD=quantus
```

## 🛠️ Commands

### Basic (Node + Miner)

```bash
docker compose up -d        # Start
docker compose down         # Stop
docker compose logs -f      # View logs
docker compose restart      # Restart
docker compose ps           # Check status
```

### With Monitoring

```bash
# Start all (node + miner + monitoring)
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d

# Stop all
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml down

# Stop only monitoring (keep node + miner running)
docker compose -f docker-compose.monitoring.yml down
```

## 📁 Data Structure

```
miner-stack/
├── docker-compose.yml            # Main: node + miner
├── docker-compose.monitoring.yml # Optional: Prometheus + Grafana
├── init-node.sh
├── .env
├── node-keys/                    # Node identity (persistent)
│   └── key_node
├── node-data/                    # Chain data (can be deleted)
│   └── chains/
├── prometheus/
│   └── prometheus.yml
└── grafana/
    └── provisioning/
```

**Important:** `node-keys/` persists your node identity. Backup this directory!

## 🔧 Troubleshooting

### Check node status
```bash
docker compose logs quantus-node | grep "Syncing"
```

### Check peer count
```bash
curl -H "Content-Type: application/json" \
  -d '{"id":1, "jsonrpc":"2.0", "method": "system_peers"}' \
  http://localhost:9944
```

### Port conflicts
Change ports in `.env` file.

### Reset chain data
```bash
docker compose down
rm -rf node-data/
docker compose up -d
```

Node key will be preserved in `node-keys/`.

---

**Happy Mining! ⛏️**
