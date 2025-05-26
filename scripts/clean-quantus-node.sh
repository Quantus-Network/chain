#!/bin/bash

set -e

# Configuration
SYSTEM_BINARY_PATH="/usr/local/bin/quantus-node"
USER_BINARY_PATH="$HOME/.local/bin/quantus-node"
QUANTUS_HOME="$HOME/.quantus"
NODE_IDENTITY_PATH="$QUANTUS_HOME/node-identity.json"
REWARDS_ADDRESS_PATH="$QUANTUS_HOME/rewards-address.txt"

echo "Starting Quantus node cleanup..."

# Remove system node binary
if [ -f "$SYSTEM_BINARY_PATH" ]; then
    echo "Deleting system node binary: $SYSTEM_BINARY_PATH"
    sudo rm -f "$SYSTEM_BINARY_PATH"
    echo "✓ System node binary deleted"
else
    echo "No system node binary found at: $SYSTEM_BINARY_PATH"
fi

# Remove user node binary
if [ -f "$USER_BINARY_PATH" ]; then
    echo "Deleting user node binary: $USER_BINARY_PATH"
    rm -f "$USER_BINARY_PATH"
    echo "✓ User node binary deleted"
else
    echo "No user node binary found at: $USER_BINARY_PATH"
fi

# Remove node identity file
if [ -f "$NODE_IDENTITY_PATH" ]; then
    echo "Deleting node identity file: $NODE_IDENTITY_PATH"
    rm -f "$NODE_IDENTITY_PATH"
    echo "✓ Node identity file deleted"
else
    echo "No node identity file found at: $NODE_IDENTITY_PATH"
fi

# Remove rewards address file
if [ -f "$REWARDS_ADDRESS_PATH" ]; then
    echo "Deleting rewards address file: $REWARDS_ADDRESS_PATH"
    rm -f "$REWARDS_ADDRESS_PATH"
    echo "✓ Rewards address file deleted"
else
    echo "No rewards address file found at: $REWARDS_ADDRESS_PATH"
fi

# Remove Quantus home directory
if [ -d "$QUANTUS_HOME" ]; then
    echo "Deleting Quantus home directory: $QUANTUS_HOME"
    rm -rf "$QUANTUS_HOME"
    echo "✓ Quantus home directory deleted"
else
    echo "No Quantus home directory found at: $QUANTUS_HOME"
fi

echo "Clean completed successfully!" 