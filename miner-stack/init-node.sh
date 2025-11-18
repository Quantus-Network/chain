#!/bin/bash
set -e

NODE_KEY_PATH="/node-keys"
NODE_KEY_FILE="$NODE_KEY_PATH/key_node"

# Generate node key if it doesn't exist
if [ ! -f "$NODE_KEY_FILE" ]; then
    echo "Generating node key..."
    mkdir -p "$NODE_KEY_PATH"
    /usr/local/bin/quantus-node key generate-node-key --file "$NODE_KEY_FILE"
    echo "Node key generated at: $NODE_KEY_FILE"
fi

# Start node with original arguments
exec /usr/local/bin/quantus-node "$@"
