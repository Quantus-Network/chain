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
docker compose up -d
```

### 3. Monitor

```bash
docker compose logs -f quantus-node
docker compose logs -f quantus-miner
```

## 📋 Configuration

### Essential Settings

Edit `.env` file:

```bash
# Required
REWARDS_ADDRESS=your_ss58_address_here
CHAIN=dirac
NODE_NAME=my-quantus-node
NODE_VERSION=v0.4.2
MINER_VERSION=v1.0.0
```

### Optional Settings

```bash
# Miner workers (default: auto-detect)
WORKERS=4

# Network settings
IN_PEERS=256
OUT_PEERS=256

# Ports
P2P_PORT=30333
RPC_PORT=9944
PROMETHEUS_PORT=9615
```

## 🛠️ Commands

```bash
docker compose up -d        # Start
docker compose down         # Stop
docker compose logs -f      # View logs
docker compose restart      # Restart
docker compose ps           # Check status
```

## 📁 Data Structure

```
miner-stack/
├── docker-compose.yml
├── init-node.sh
├── .env
├── node-keys/          # Node identity (persistent)
│   └── key_node
└── node-data/          # Chain data (can be deleted)
    └── chains/
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
