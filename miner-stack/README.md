# Quantus Node - Docker Setup

Simple Docker Compose setup for running a Substrate-based blockchain node.

## 🚀 Quick Start

### Prerequisites

- Docker & Docker Compose installed
- Minimum 4GB RAM, 2 CPU cores
- 100GB+ disk space recommended

### Setup in 3 Steps

#### 1. Configure

```bash
# Copy the example configuration
cp .env.example .env

# Edit the .env file
nano .env
```

**Required settings:**
- `REWARDS_ADDRESS` - Your SS58 address for mining rewards
- `CHAIN` - Chain name (e.g., dirac, heisenberg, etc.)
- `DOCKER_IMAGE` - Docker image for your chain

#### 2. Start

```bash
# Using Makefile (recommended)
make start

# OR using docker-compose directly
docker-compose up -d
```

#### 3. Monitor

```bash
# View logs
make logs

# OR
docker-compose logs -f quantus-node
```

## 📋 Configuration

### .env File

All configuration is done via `.env` file:

```bash
# Required
REWARDS_ADDRESS=your_ss58_address_here
CHAIN=dirac
DOCKER_IMAGE=ghcr.io/quantus/dirac-chain:latest

# Optional
NODE_NAME=my-quantus-node
IN_PEERS=256
OUT_PEERS=256
P2P_PORT=30333
RPC_PORT=9944
```

### Multiple Chains

You can run different chains by changing the `CHAIN` parameter:

```bash
# For Dirac
CHAIN=dirac
DOCKER_IMAGE=ghcr.io/quantus/dirac-chain:latest

# For another chain
CHAIN=mychain
DOCKER_IMAGE=myregistry/mychain:latest
```

## 🛠️ Management Commands

### Using Makefile

```bash
make help        # Show all commands
make start       # Start validator
make stop        # Stop validator
make restart     # Restart validator
make logs        # View logs
make status      # Check status
make backup      # Backup node key
make clean       # Remove all data (dangerous!)
```

### Using Docker Compose

```bash
docker-compose up -d              # Start
docker-compose down               # Stop
docker-compose logs -f            # View logs
docker-compose restart            # Restart
docker-compose ps                 # Check status
```

## 📁 Directory Structure

```
miner-stack/
├── docker-compose.yml    # Docker configuration
├── entrypoint.sh         # Node initialization
├── .env                  # Your configuration
├── .env.example          # Configuration template
├── Makefile             # Management commands
└── data/                # Persistent data
    └── chains/
        └── {CHAIN}/
            ├── network/ # Node key (BACKUP THIS!)
            └── db/      # Blockchain database
```

## 🔑 Important Information

### Node Key

Your node key is automatically generated on first start and stored in:
```
data/chains/{CHAIN}/network/secret_dilithium
```

**⚠️ BACKUP THIS FILE!** It's your node's identity.

### PeerId

Your node's PeerId is shown on first start. Save it if you want to add your node as a bootnode.

View it with:
```bash
docker-compose logs quantus-node | grep "PeerId"
```

## 🔧 Troubleshooting

### Port Already in Use

Change port in `.env`:
```bash
P2P_PORT=30334
RPC_PORT=9945
```

### Can't Connect to Peers

Check firewall allows P2P port (default 30333):
```bash
# Linux example
sudo ufw allow 30333/tcp
```

### Node Crashes

Check logs:
```bash
make logs
```

Check resources:
```bash
docker stats quantus-node
```

### Configuration Error

Validate configuration:
```bash
make check-config
```

## 🔐 Security

1. **Backup node key** - `data/chains/{CHAIN}/network/`
2. **Secure rewards address** - Never share private keys
3. **Firewall** - Only expose necessary ports
4. **Keep updated** - `docker-compose pull` regularly

## 📊 Monitoring

### Check Sync Status

```bash
curl -H "Content-Type: application/json" \
  -d '{"id":1, "jsonrpc":"2.0", "method": "system_health"}' \
  http://localhost:9944
```

### Check Peer Count

```bash
curl -H "Content-Type: application/json" \
  -d '{"id":1, "jsonrpc":"2.0", "method": "system_peers"}' \
  http://localhost:9944
```

### Prometheus Metrics

Available at: http://localhost:9615/metrics

## 🆘 Getting Help

Check logs first:
```bash
make logs
```

Common issues:
- Missing `.env` file → Run `make setup`
- Port conflicts → Change ports in `.env`
- No peers → Check firewall
- Out of disk → Check with `du -sh data/`

---

**Happy Validating! 🎉**
