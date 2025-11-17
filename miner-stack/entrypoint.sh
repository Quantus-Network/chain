#!/bin/bash
set -e

# Validate required configuration
if [ -z "$REWARDS_ADDRESS" ]; then
    echo "ERROR: REWARDS_ADDRESS not set!"
    echo "Please set it in .env file"
    exit 1
fi

# Configuration with defaults
CHAIN="${CHAIN:-dirac}"
BASE_PATH="${BASE_PATH:-/data}"
NODE_KEY_PATH="${NODE_KEY_PATH:-/node-keys}"
NODE_NAME="${NODE_NAME:-quantus-node}"
IN_PEERS="${IN_PEERS:-256}"
OUT_PEERS="${OUT_PEERS:-256}"

# Node key file (stored separately from chain data)
NODE_KEY_FILE="$NODE_KEY_PATH/${CHAIN}_node_key"

# Generate node key if it doesn't exist
if [ ! -f "$NODE_KEY_FILE" ]; then
    echo "Generating node key..."
    mkdir -p "$NODE_KEY_PATH"
    /usr/local/bin/quantus-node key generate-node-key --file "$NODE_KEY_FILE"
    echo "Node key generated"
fi

# Create symlink in the expected location for the node
CHAIN_KEY_DIR="$BASE_PATH/chains/$CHAIN/network"
mkdir -p "$CHAIN_KEY_DIR"
ln -sf "$NODE_KEY_FILE" "$CHAIN_KEY_DIR/secret_dilithium"

echo "Starting quantus node..."
echo "Chain: $CHAIN"
echo "Rewards: $REWARDS_ADDRESS"
echo ""

# Build command arguments
CMD_ARGS=(
    --chain "$CHAIN"
    --base-path "$BASE_PATH"
    --name "$NODE_NAME"
    --validator
    --in-peers "$IN_PEERS"
    --out-peers "$OUT_PEERS"
    --port 30333
    --rpc-port 9944
    --rpc-cors all
    --prometheus-external
    --prometheus-port 9615
    --rewards-address "$REWARDS_ADDRESS"
)

# Optional: External miner
if [ -n "$EXTERNAL_MINER_URL" ]; then
    CMD_ARGS+=(--external-miner-url "$EXTERNAL_MINER_URL")
fi

# Optional: Peer sharing
if [ "$ENABLE_PEER_SHARING" = "true" ]; then
    CMD_ARGS+=(--enable-peer-sharing)
fi

# Start the node
exec /usr/local/bin/quantus-node "${CMD_ARGS[@]}"
