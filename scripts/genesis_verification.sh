#!/bin/bash

# This script verifies that the genesis runtime Wasm matches the
# published release artifact from a specific GitHub release tag.

set -e

# --- Configuration ---
GITHUB_REPO="${GITHUB_REPO:-Quantus-Network/chain}"

# --- Helper Functions ---
print_usage() {
  echo "Usage: $0 --release-tag <TAG> --node-url <URL> [--artifact-name <NAME>]"
  echo ""
  echo "Arguments:"
  echo "  --release-tag      Required. The GitHub release tag (e.g., v0.9.44)."
  echo "  --node-url         Required. The RPC URL of a running node (e.g., http://localhost:9944)."
  echo "  --artifact-name    Optional. The specific name of the Wasm artifact file."
  echo "                     If not provided, the script will try to find a unique artifact"
  echo "                     ending in '.compact.compressed.wasm' in the release assets."
  echo "  --help             Display this help message."
  echo ""
  echo "This script requires 'gh', 'curl', 'jq', and 'xxd' to be installed."
  echo "Set GITHUB_REPO to verify against a release in another repository (default: $GITHUB_REPO)."
  echo "Note: This script compares the GENESIS runtime (block 0x0) with the release artifact."
  echo "The node must be an archive node or have access to genesis state for this to work."
}

# --- Argument Parsing ---
RELEASE_TAG=""
NODE_URL=""
ARTIFACT_NAME=""

while [[ "$#" -gt 0 ]]; do
  case $1 in
    --release-tag) RELEASE_TAG="$2"; shift ;;
    --node-url) NODE_URL="$2"; shift ;;
    --artifact-name) ARTIFACT_NAME="$2"; shift ;;
    --help) print_usage; exit 0 ;;
    *) echo "Unknown parameter passed: $1"; print_usage; exit 1 ;;
  esac
  shift
done

if [ -z "$RELEASE_TAG" ] || [ -z "$NODE_URL" ]; then
  echo "❌ Error: Missing required arguments."
  print_usage
  exit 1
fi

# --- Main Logic ---

if [ -n "$ARTIFACT_NAME" ]; then
  echo "ℹ️ Using user-provided artifact name: $ARTIFACT_NAME"
else
  # gh (not the REST API) so private repositories work through the caller's gh auth
  echo "ℹ️ Artifact name not provided. Querying release '$RELEASE_TAG' of $GITHUB_REPO for Wasm artifact..."
  CANDIDATES=$(gh release view "$RELEASE_TAG" -R "$GITHUB_REPO" --json assets --jq '.assets[].name | select(endswith(".compact.compressed.wasm"))')

  if [ -z "$CANDIDATES" ]; then
    echo "❌ Error: Could not find any Wasm artifact (*.compact.compressed.wasm) in release '$RELEASE_TAG' of $GITHUB_REPO."
    echo "   Please check the release page or provide the correct name using --artifact-name."
    exit 1
  fi

  # Note: wc -l on a single line string returns 1, which is what we want.
  NUM_CANDIDATES=$(echo "$CANDIDATES" | wc -l)

  if [ "$NUM_CANDIDATES" -gt 1 ]; then
    echo "❌ Error: Found multiple possible Wasm artifacts. Please specify one using --artifact-name:"
    echo "$CANDIDATES"
    exit 1
  fi

  ARTIFACT_NAME="$CANDIDATES"
  echo "✅ Found unique artifact: $ARTIFACT_NAME"
fi


# 1. Download the release artifact
TEMP_WASM_FILE=$(mktemp)

echo "⬇️  Downloading '$ARTIFACT_NAME' from release '$RELEASE_TAG' of $GITHUB_REPO..."

if ! gh release download "$RELEASE_TAG" -R "$GITHUB_REPO" -p "$ARTIFACT_NAME" -O "$TEMP_WASM_FILE" --clobber; then
  echo "❌ Error: Failed to download artifact."
  echo "   Please check if the release tag and artifact name are correct."
  rm "$TEMP_WASM_FILE"
  exit 1
fi

echo "✅ Artifact downloaded successfully."
echo ""

# 2. Convert the local Wasm artifact to a hex string
echo "🔧 Converting local Wasm artifact to hex..."
LOCAL_WASM="0x$(xxd -p "$TEMP_WASM_FILE" | tr -d '\n')"
rm "$TEMP_WASM_FILE" # Clean up the downloaded file immediately
echo "✅ Conversion complete."
echo ""

# 3. Query the genesis runtime from the node (block 0x0)
echo "🔎 Querying genesis runtime from node at '$NODE_URL' (block 0x0)..."
GENESIS_HASH=$(curl -s -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}' "$NODE_URL" | jq -r .result)

if [ -z "$GENESIS_HASH" ] || [ "$GENESIS_HASH" == "null" ]; then
    echo "❌ Error: Failed to query the genesis block hash from '$NODE_URL'."
    exit 1
fi
echo "    Genesis hash: $GENESIS_HASH"

GENESIS_WASM=$(curl -s -H "Content-Type: application/json" -d "{\"jsonrpc\":\"2.0\",\"method\":\"state_getStorage\",\"params\":[\"0x3a636f6465\",\"$GENESIS_HASH\"],\"id\":1}" "$NODE_URL" | jq -r .result)

if [ -z "$GENESIS_WASM" ] || [ "$GENESIS_WASM" == "null" ]; then
    echo "❌ Error: Failed to query genesis runtime. Received empty or null response."
    echo "   This could mean:"
    echo "   - The node URL is incorrect or the node is not running"
    echo "   - The node is not an archive node and doesn't have genesis state"
    echo "   - There was an RPC error"
    exit 1
fi

echo "✅ Genesis runtime queried successfully."
echo ""

# 4. Compare the Wasm strings
echo "🔄 Comparing local artifact with genesis runtime..."

# For debugging, display the beginning and end of each hex string.
echo "---"
echo "Genesis Wasm  (start...end): ${GENESIS_WASM:0:60}...${GENESIS_WASM: -60}"
echo "Local Wasm    (start...end): ${LOCAL_WASM:0:60}...${LOCAL_WASM: -60}"
echo "---"

if [ "$GENESIS_WASM" == "$LOCAL_WASM" ]; then
  echo "✅✅✅ SUCCESS: The genesis runtime matches the release artifact."
else
  echo "❌❌❌ FAILURE: Mismatch detected!"
  echo "   The genesis runtime does NOT match the release artifact."
  # Optionally, print hashes for easier comparison
  echo "   Genesis hash: $(echo -n "$GENESIS_WASM" | sha256sum | awk '{print $1}')"
  echo "   Local hash:   $(echo -n "$LOCAL_WASM" | sha256sum | awk '{print $1}')"
  exit 1
fi

exit 0