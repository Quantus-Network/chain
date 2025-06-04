# Quantus Network Mining Guide

Get started mining on the Quantus Network testnet in minutes.

## Quick Start

### 1. Install Node Binary

Download and run our installation script:

```bash
curl -sSL https://raw.githubusercontent.com/Quantus-Network/chain/main/scripts/install-quantus-node.sh | bash
```

This script will:
- Download the latest Quantus node binary for your system
- Create your node identity (P2P key)
- Generate or import your rewards address
- Set up the Quantus home directory (`~/.quantus`)

### 2. Start Mining

After installation, run the command provided by the installer:

```bash
quantus-node \
  --node-key-file ~/.quantus/node_key.p2p \
  --rewards-address ~/.quantus/rewards-address.txt \
  --validator \
  --chain live_resonance \
  --port 30333 \
  --prometheus-port 9616 \
  --name "ResonanceLiveTestnetNode" \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all"
```

That's it! You're now mining on the Quantus Network.

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

## Advanced Setup

### Manual Installation

If you prefer manual installation or the script doesn't work for your system:

1. **Download Binary**

   Visit [GitHub Releases](https://github.com/Quantus-Network/chain/releases) and download the appropriate binary for your system.

2. **Generate Node Identity**
   ```bash
   ./quantus-node key generate-node-key --file ~/.quantus/node_key.p2p
   ```

3. **Generate Rewards Address**
   ```bash
   ./quantus-node key quantus
   ```

   Save the displayed address to `~/.quantus/rewards-address.txt`

### External Mining Setup

For improved performance, you can offload mining to a dedicated service:

1. **Build External Miner**
   ```bash
   git clone https://github.com/Quantus-Network/chain.git
   cd chain
   cargo build --release -p external-miner
   ```

2. **Start External Miner** (separate terminal)
   ```bash
   RUST_LOG=info ./target/release/external-miner
   ```

3. **Start Node with External Miner**
   ```bash
   quantus-node \
     --node-key-file ~/.quantus/node_key.p2p \
     --rewards-address ~/.quantus/rewards-address.txt \
     --validator \
     --chain live_resonance \
     --external-miner-url http://127.0.0.1:9833 \
     --port 30333 \
     --name "ResonanceLiveTestnetNode"
   ```

## Configuration Options

### Node Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `--node-key-file` | Path to P2P identity file | Required |
| `--rewards-address` | Path to rewards address file | Required |
| `--chain` | Chain specification | `live_resonance` |
| `--port` | P2P networking port | `30333` |
| `--prometheus-port` | Metrics endpoint port | `9616` |
| `--name` | Node display name | Auto-generated |
| `--base-path` | Data directory | `~/.local/share/quantus-node` |



## Monitoring Your Node

### Check Node Status

**View Logs**
```bash
# Real-time logs
tail -f ~/.local/share/quantus-node/chains/live_resonance/network/quantus-node.log

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

- **Chain**: Resonance Live Testnet
- **Consensus**: Quantum Proof of Work (QPoW)
- **Block Time**: ~6 seconds target
- **Network Explorer**: Coming soon
- **Faucet**: Built into node RPC

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
quantus-node purge-chain --chain live_resonance
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
