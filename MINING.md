# Quantus Network Mining Guide

Get started mining on the Quantus Network testnet in minutes.

## System Requirements

### Minimum Requirements
- **CPU**: 2+ cores
- **RAM**: 4GB
- **Storage**: 100GB available space
- **Network**: Stable internet connection
- **OS**: Linux (Ubuntu 20.04+), macOS (10.15+), or Windows WSL2

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

3. **Generate Rewards Address**
   ```bash
   ./quantus-node key quantus
   ```

  The address is in the output like this:
```sh
...
Address: qzpjg55HuN2vLdQerpZwhsGfRn6b4pc8uh4bdEgsYbJNeu8rn
...
```

4. **Run the node (Dirac testnet)**

Minimal command - see --help for many more options
```sh
./quantus-node \
    --validator \
    --chain dirac \
    --node-key-file ~/.quantus/node_key.p2p \
    --rewards-address <COPIED_REWARDS_ADDRESS> \
    --max-blocks-per-request 64 \
    --sync full
```
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

**Step 3: Generate and Save Your Rewards Address**

Run the following command to generate your unique rewards address:
```bash
# If on Apple Silicon, you may need to add --platform linux/amd64
docker run --rm ghcr.io/quantus-network/quantus-node:latest key quantus
```
Replace `quantus-node:v0.0.4` with your desired image.
This command will display your secret phrase, public key, address, and seed.
**Important: Securely back up your secret phrase!**
Next, **copy the displayed `Address`.

**Step 4: Run the Validator Node**

Now, run the Docker container with all the necessary parameters:
```bash
# If on Apple Silicon, you may need to add --platform linux/amd64
docker run -d \
  --name quantus-node \
  --restart unless-stopped \
  -v "$(pwd)/quantus_node_data":/var/lib/quantus \
  -p 30333:30333 \
  -p 9944:9944 \
  ghcr.io/quantus-network/quantus-node:latest \
  --validator \
  --base-path /var/lib/quantus \
  --chain dirac \
  --node-key-file /var/lib/quantus/node_key.p2p \
  --rewards-address <COPIED_REWARDS_ADDRESS>
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
  --chain dirac \
  --rewards-address YOUR_ADDRESS_HERE
```

**Docker system requirements**
- Docker 20.10+ or compatible runtime
- All other system requirements same as binary installation

## Configuration Options

### Node Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `--node-key-file` | Path to P2P identity file | Required |
| `--rewards-address` | Path to rewards address file | Required |
| `--chain` | Chain specification | `dirac` |
| `--port` | P2P networking port | `30333` |
| `--prometheus-port` | Metrics endpoint port | `9616` |
| `--name` | Node display name | Auto-generated |
| `--base-path` | Data directory | `~/.local/share/quantus-node` |



## Monitoring Your Node

### Check Node Status

**View Logs**
```bash
# Real-time logs
tail -f ~/.local/share/quantus-node/chains/dirac/network/quantus-node.log

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

**View Balance**
```bash
# Replace YOUR_ADDRESS with your rewards address
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"faucet_getAccountInfo","params":["YOUR_ADDRESS"]}' \
  http://localhost:9944
```

## Testnet Information

- **Chain**: Dirac Testnet
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
quantus-node purge-chain --chain dirac
```

**Mining Not Working**
1. Check that `--validator` flag is present
2. Verify rewards address file exists and contains valid address
3. Ensure node is synchronized (check logs for "Imported #XXXX")

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

### Rewards Structure

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

- **Backup Your Keys**: Store copies of your node identity and rewards keys safely
- **Secure Storage**: Keep private keys in encrypted storage
- **Regular Rotation**: Consider rotating keys periodically for enhanced security

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
2. **Monitor Performance**: Track your mining efficiency and rewards
3. **Experiment**: Try different configurations and optimizations
4. **Contribute**: Help improve the network by reporting issues and feedback

Happy mining! 🚀

---

*For technical support and updates, visit the [Quantus Network GitHub repository](https://github.com/Quantus-Network/chain).*
