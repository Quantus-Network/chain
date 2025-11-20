#!/bin/bash

# Genesis Draft Generator
#
# This script generates a chain specification for a new environment without creating git branches.
# It's designed for adding completely new chains to the project.
#
# What it does:
# 1. Generates chain spec from a runtime preset (e.g., dirac_live_spec)
# 2. Downloads runtime WASM from a GitHub release
# 3. Replaces the runtime code in the generated chain spec
#
# Unlike the main genesis_generate_spec.sh, this script:
# - Does NOT create or switch git branches
# - Does NOT require a clean working directory
# - Works with the current codebase state
#
# Use this when:
# - Adding a new chain configuration
# - Testing new chain specs
# - Working with development releases
#
# Usage: ./genesis_generate_draft.sh <release_tag> <profile> [local_wasm_path]
# Example: ./genesis_generate_draft.sh v0.2.3-ns-fog dirac
# Example with local WASM: ./genesis_generate_draft.sh v0.2.3-ns-fog dirac /path/to/runtime.wasm

set -e

# Check if both parameters are provided
if [ -z "$1" ] || [ -z "$2" ]; then
  echo "❌ Error: Missing parameters."
  echo "Usage: $0 <release_tag> <profile> [local_wasm_path]"
  echo "Example: $0 v0.2.3-ns-fog dirac"
  echo "Example with local WASM: $0 v0.2.3-ns-fog dirac ~/Downloads/runtime.wasm"
  echo ""
  echo "This script will:"
  echo "  1. Generate chain spec from profile preset"
  echo "  2. Download WASM from the draft release (or use local file if provided)"
  echo "  3. Replace runtime code in the generated spec"
  exit 1
fi

RELEASE_TAG=$1
PROFILE=$2
LOCAL_WASM_PATH=$3

# Dynamic generation based on naming convention
PROFILE_SPEC="${PROFILE}_live_spec"                                    # dirac -> dirac_live_spec
OUTPUT_FILE="node/src/chain-specs/${PROFILE//_/-}.json"               # dirac -> dirac.json

echo "🔧 Generating chain spec for '$PROFILE' with WASM from draft release..."
echo "📁 Output file: $OUTPUT_FILE"
echo "🏷️  Release tag: $RELEASE_TAG"
echo "⚙️  Execution profile: $PROFILE_SPEC"
echo ""

QUANTUS_NODE_BIN="./target/release/quantus-node"
GITHUB_REPO="Quantus-Network/chain"

echo "🚀 Building node to generate chain spec..."
cargo build --release --package quantus-node

if [ ! -f "$QUANTUS_NODE_BIN" ]; then
    echo "❌ Build failed. Quantus node binary not found."
    exit 1
fi

echo "🔧 Generating chain spec from '$PROFILE_SPEC'..."
$QUANTUS_NODE_BIN build-spec --chain "$PROFILE_SPEC" --raw > "$OUTPUT_FILE"

if [ ! -s "$OUTPUT_FILE" ]; then
  echo "❌ Failed to generate chain spec. The output file is empty."
  exit 1
fi

echo "🌐 Fetching runtime spec_version from GitHub release..."

# Try to find in all releases (including drafts)
ALL_RELEASES_URL="https://api.github.com/repos/$GITHUB_REPO/releases"

# TEMPORARY WORKAROUND for draft release v3.0.0-kingfisher
if [ "$RELEASE_TAG" = "v3.0.0-kingfisher" ]; then
    echo "⚠️  Using hardcoded values for draft release $RELEASE_TAG"
    RELEASE_TAG="untagged-4a85066e28e1760c0840"
    SPEC_VERSION="114"
    ASSETS_JSON="quantus-runtime-v114.compact.compressed.wasm"
else
    ASSETS_JSON=$(curl -fsSL "$ALL_RELEASES_URL" | jq -r --arg tag "$RELEASE_TAG" '.[] | select(.tag_name == $tag or .name == $tag) | .assets[] | select(.name | contains("quantus-runtime-v")) | .name' | head -1)

    if [ -z "$ASSETS_JSON" ]; then
        echo "❌ Error: Could not find runtime assets in release $RELEASE_TAG."
        echo "💡 Make sure the release exists and has runtime WASM assets."
        exit 1
    fi

    SPEC_VERSION=$(echo "$ASSETS_JSON" | grep -o 'v[0-9]\+' | sed 's/v//')

    if [ -z "$SPEC_VERSION" ] || [ "$SPEC_VERSION" = "null" ]; then
        echo "❌ Error: Could not determine spec_version from release."
        exit 1
    fi
fi

echo "📋 Using spec_version: $SPEC_VERSION"

TEMP_WASM=$(mktemp)

# Check if local WASM file is provided
if [ -n "$LOCAL_WASM_PATH" ]; then
    echo "📁 Using local WASM file: $LOCAL_WASM_PATH"
    
    if [ ! -f "$LOCAL_WASM_PATH" ]; then
        echo "❌ Error: Local WASM file not found: $LOCAL_WASM_PATH"
        exit 1
    fi
    
    # Copy local file to temp location
    cp "$LOCAL_WASM_PATH" "$TEMP_WASM"
    echo "✅ Local WASM file loaded"
else
    echo "⬇️ Downloading runtime WASM from GitHub release..."
    
    COMPACT_WASM_URL="https://github.com/$GITHUB_REPO/releases/download/$RELEASE_TAG/quantus-runtime-v${SPEC_VERSION}.compact.compressed.wasm"
    echo "📥 Downloading: $COMPACT_WASM_URL"
    
    # Try with GitHub token if available, otherwise without
    if [ -n "$GITHUB_TOKEN" ]; then
        echo "🔐 Using GitHub token for authentication"
        if ! curl -L -H "Authorization: token $GITHUB_TOKEN" -H "Accept: application/octet-stream" "$COMPACT_WASM_URL" -o "$TEMP_WASM"; then
            echo "❌ Error: Failed to download compressed WASM runtime."
            exit 1
        fi
    else
        if ! curl -L -H "Accept: application/octet-stream" "$COMPACT_WASM_URL" -o "$TEMP_WASM"; then
            echo "❌ Error: Failed to download compressed WASM runtime."
            echo "💡 If this is a draft release, set GITHUB_TOKEN environment variable or provide local WASM file path."
            exit 1
        fi
    fi
fi

# Verify file was downloaded/loaded correctly (basic size check)
WASM_SIZE=$(stat -f%z "$TEMP_WASM" 2>/dev/null || stat -c%s "$TEMP_WASM" 2>/dev/null)
if [ "$WASM_SIZE" -lt 1000 ]; then
    echo "❌ Error: WASM file is too small ($WASM_SIZE bytes). Download likely failed."
    echo "File contents:"
    head -c 100 "$TEMP_WASM"
    exit 1
fi

echo "✅ WASM file loaded successfully ($WASM_SIZE bytes)"

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
echo "ℹ️ You can now use this chain spec with:"
echo "   ./target/release/quantus-node --chain $OUTPUT_FILE"
