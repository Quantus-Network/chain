#!/bin/bash

# Ensure the script exits on the first error
set -e

echo "🚀 Building the project..."
cargo build

# Define the node key (should be the same key used to run the bootnode)
NODE_KEY="cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15"

# Get the Peer ID from the node key
BOOTNODE_ID=$(subkey inspect-node-key --file <(echo "$NODE_KEY"))

# Validate the bootnode ID
if [[ -z "$BOOTNODE_ID" ]]; then
  echo "❌ Failed to generate bootnode ID"
  exit 1
fi

echo "✅ Bootnode ID: $BOOTNODE_ID"

# Generate the initial chain spec
echo "🔧 Generating chain spec..."
./target/release/quantus-node build-spec --chain local > custom-spec.json

# Update the chain spec to set the correct bootnode
echo "🔧 Updating bootnode in chain spec..."
jq --arg BOOTNODE_ID "$BOOTNODE_ID" '
  .bootNodes = ["/ip4/127.0.0.1/tcp/30333/p2p/" + $BOOTNODE_ID]
' custom-spec.json > custom-spec-updated.json

# Use the updated chain spec to generate the raw version
echo "🔧 Generating raw chain spec..."
./target/release/quantus-node build-spec --chain custom-spec-updated.json --raw > custom-spec-raw.json

rm custom-spec-updated.json

echo "✅ Chain spec setup completed!"