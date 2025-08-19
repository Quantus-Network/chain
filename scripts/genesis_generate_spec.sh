#!/bin/bash

# This script generates chain specifications from a specific git release tag.
# This ensures that the genesis state is transparently and reproducibly built from a known version of the runtime code.
# The script downloads the WASM runtime from GitHub releases and directly replaces the runtime code in the generated chain spec.

set -e

# Check if both parameters are provided
if [ -z "$1" ] || [ -z "$2" ]; then
  echo "❌ Error: Missing parameters."
  echo "Usage: $0 <release_tag> <profile>"
  echo "Example: $0 v0.0.7-test-genesis live_resonance"
  echo "Example: $0 v0.0.7-test-genesis heisenberg"
  echo ""
  echo "Available profiles:"
  echo "  - live_resonance: Live Resonance network"
  echo "  - heisenberg: Heisenberg testnet"
  exit 1
fi

RELEASE_TAG=$1
PROFILE=$2

# Validate profile parameter
if [ "$PROFILE" != "live_resonance" ] && [ "$PROFILE" != "heisenberg" ]; then
  echo "❌ Error: Invalid profile '$PROFILE'."
  echo "Available profiles: live_resonance, heisenberg"
  exit 1
fi

# Set output file and chain ID based on profile
case $PROFILE in
  "live_resonance_live_spec")
    OUTPUT_FILE="node/src/chain-specs/live-resonance.json"
    CHAIN_ID="live_resonance"
    ;;
  "heisenberg_live_spec")
    OUTPUT_FILE="node/src/chain-specs/heisenberg.json"
    CHAIN_ID="heisenberg"
    ;;
esac

QUANTUS_NODE_BIN="./target/release/quantus-node"
GITHUB_REPO="Quantus-Network/chain"

echo "🔄 Checking current git status..."
if ! git diff-index --quiet HEAD --; then
    echo "❌ Error: Your working directory is not clean. Please commit or stash your changes before running this script."
    exit 1
fi

echo "⬇️ Fetching latest tags from origin..."
git fetch --all --tags

BRANCH_NAME="genesis/$PROFILE/$RELEASE_TAG"
echo "✨ Creating and switching to new branch '$BRANCH_NAME'..."
git checkout -b "$BRANCH_NAME" "tags/$RELEASE_TAG"

echo "🌐 Fetching runtime spec_version from GitHub release..."
# First, get the list of release assets to find the spec_version
RELEASE_API_URL="https://api.github.com/repos/$GITHUB_REPO/releases/tags/$RELEASE_TAG"
ASSETS_JSON=$(curl -fsSL "$RELEASE_API_URL" | jq -r '.assets[] | select(.name | contains("quantus-runtime-v")) | .name' | head -1)
if [ -z "$ASSETS_JSON" ]; then
    echo "❌ Error: Could not find runtime assets in release $RELEASE_TAG."
    exit 1
fi

SPEC_VERSION=$(echo "$ASSETS_JSON" | grep -o 'v[0-9]\+' | sed 's/v//')

if [ -z "$SPEC_VERSION" ] || [ "$SPEC_VERSION" = "null" ]; then
    echo "❌ Error: Could not determine spec_version from release."
    exit 1
fi

echo "📋 Using spec_version: $SPEC_VERSION"
echo "🎯 Generating chain spec for profile: $PROFILE"

echo "🚀 Building node to generate initial chain spec..."
cargo build --release --package quantus-node

if [ ! -f "$QUANTUS_NODE_BIN" ]; then
    echo "❌ Build failed. Quantus node binary not found."
    exit 1
fi

echo "🔧 Generating initial chain spec from '$CHAIN_ID'..."
$QUANTUS_NODE_BIN build-spec --chain "$CHAIN_ID" --raw > "$OUTPUT_FILE"

if [ ! -s "$OUTPUT_FILE" ]; then
  echo "❌ Failed to generate chain spec. The output file is empty."
  exit 1
fi

echo "⬇️ Downloading runtime WASM from GitHub release..."

# Download the compressed WASM (this is what should be in the runtime code storage)
TEMP_WASM=$(mktemp)
COMPACT_WASM_URL="https://github.com/$GITHUB_REPO/releases/download/$RELEASE_TAG/quantus-runtime-v${SPEC_VERSION}.compact.compressed.wasm"
echo "Downloading: $COMPACT_WASM_URL"
if ! curl -fsSL "$COMPACT_WASM_URL" -o "$TEMP_WASM"; then
    echo "❌ Error: Failed to download compressed WASM runtime."
    exit 1
fi

echo "📝 Converting WASM to hex and replacing runtime code in chain spec..."

# Convert WASM to hex without 0x prefix
WASM_HEX=$(xxd -p "$TEMP_WASM" | tr -d '\n')

# Replace the runtime code in the JSON (0x3a636f6465 is the :code storage key)
# Create a temporary file for the modified JSON
TEMP_JSON=$(mktemp)
TEMP_HEX_FILE=$(mktemp)

# Write the hex string to a temporary file (with 0x prefix)
echo "0x$WASM_HEX" > "$TEMP_HEX_FILE"

# Use jq to replace the runtime code, reading the hex from file
jq --rawfile wasm_hex "$TEMP_HEX_FILE" '.genesis.raw.top."0x3a636f6465" = ($wasm_hex | rtrimstr("\n"))' "$OUTPUT_FILE" > "$TEMP_JSON"

# Replace the original file
mv "$TEMP_JSON" "$OUTPUT_FILE"

# Clean up temp files
rm -f "$TEMP_WASM" "$TEMP_HEX_FILE"

echo "✅ Runtime code replaced successfully in chain spec."
echo "📄 The chain spec at '$OUTPUT_FILE' has been updated with runtime from $RELEASE_TAG."
echo "🎉 Genesis generation complete for profile: $PROFILE"
echo ""
echo "ℹ️ You are now on a new branch named '$BRANCH_NAME'."
echo "   Please review and commit the changes to '$OUTPUT_FILE'."
echo "   Example: git add $OUTPUT_FILE && git commit -m \"feat: generate $PROFILE genesis spec from $RELEASE_TAG\""