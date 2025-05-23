#!/bin/bash
#
# create_node_identity.sh: Generates a new libp2p node key for the Quantus node.
#

set -e # Exit immediately if a command exits with a non-zero status.
set -u # Treat unset variables as an error when substituting.

# --- Configuration ---
CONFIG_DIR="$HOME/.quantus-node"
NODE_KEY_FILE="$CONFIG_DIR/node_key.p2p"

# --- Sanity Checks and Setup ---
# Ensure subkey is installed
if ! command -v subkey &>/dev/null; then
    echo -e "\033[1;31mERROR\033[0m: 'subkey' command not found."
    echo "Please install subkey, which is part of the Substrate development tools."
    echo "You can typically install it by building Substrate and copying the binary, or by downloading a pre-built binary."
    echo "See: https://docs.substrate.io/reference/command-line-tools/subkey/"
    exit 1
fi

# Ensure config directory exists
mkdir -p "$CONFIG_DIR"

# Check if node key file already exists
if [ -f "$NODE_KEY_FILE" ]; then
    echo -e "\033[1;33mWARN\033[0m: Node key file already exists at $NODE_KEY_FILE."
    echo "Node ID: $(subkey inspect-node-key --file "$NODE_KEY_FILE")"
    echo "To generate a new key, please remove the existing file first and re-run this script."
    exit 0 # Exit successfully as the file is already there.
fi

# --- Generate Node Key ---
echo "Generating new node key and saving to $NODE_KEY_FILE..."

# subkey generate-node-key --file <PATH> outputs the Node ID to stdout
# Capture this output to display it clearly.
GENERATED_NODE_ID=$(subkey generate-node-key --file "$NODE_KEY_FILE")

if [ -f "$NODE_KEY_FILE" ]; then
    echo -e "\033[1;32mSUCCESS\033[0m: Node key generated and saved to $NODE_KEY_FILE"
    echo "Your Node ID is: $GENERATED_NODE_ID"
    echo "This ID is derived from your node's private key and is how other nodes will identify you on the network."
else
    echo -e "\033[1;31mERROR\033[0m: Node key generation failed. File not found at $NODE_KEY_FILE after running subkey."
    exit 1
fi

exit 0 