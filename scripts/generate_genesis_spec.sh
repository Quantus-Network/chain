#!/bin/bash

# This script generates the 'live-resonance.json' chain specification from a specific git release tag.
# This ensures that the genesis state is transparently and reproducibly built from a known version of the runtime code.

set -e

# Check if a release tag is provided
if [ -z "$1" ]; then
  echo "❌ Error: No release tag provided."
  echo "Usage: $0 <release_tag>"
  echo "Example: $0 v0.0.7-test-genesis"
  exit 1
fi

RELEASE_TAG=$1
OUTPUT_FILE="node/src/chain-specs/live-resonance.json" # Directly overwrite the existing spec file
QUANTUS_NODE_BIN="./target/release/quantus-node"
# This is the chain spec identifier that builds the genesis state from code, rather than loading from a file.
CHAIN_ID="live_resonance_local"

echo "🔄 Checking current git status..."
if ! git diff-index --quiet HEAD --; then
    echo "❌ Error: Your working directory is not clean. Please commit or stash your changes before running this script."
    exit 1
fi

echo "⬇️ Fetching latest tags from origin..."
git fetch --all --tags

BRANCH_NAME="genesis/$RELEASE_TAG"
echo "✨ Creating and switching to new branch '$BRANCH_NAME'..."
git checkout -b "$BRANCH_NAME" "tags/$RELEASE_TAG"

echo "🚀 Building quantus-node at release '$RELEASE_TAG'..."
cargo build --release

if [ ! -f "$QUANTUS_NODE_BIN" ]; then
    echo "❌ Build failed. Quantus node binary not found."
    exit 1
fi

echo "✅ Node built successfully."

echo "🔧 Generating raw chain spec from '$CHAIN_ID'..."
$QUANTUS_NODE_BIN build-spec --chain "$CHAIN_ID" --raw > "$OUTPUT_FILE"

if [ ! -s "$OUTPUT_FILE" ]; then
  echo "❌ Failed to generate chain spec. The output file is empty."
  exit 1
fi

echo "✅ Chain spec generated successfully."
echo "📄 The chain spec at '$OUTPUT_FILE' has been updated."
echo "🎉 Genesis generation complete."
echo ""
echo "ℹ️ You are now on a new branch named '$BRANCH_NAME'."
echo "   Please review and commit the changes to '$OUTPUT_FILE'."
echo "   Example: git add $OUTPUT_FILE && git commit -m \"feat: generate genesis spec from $RELEASE_TAG\"" 