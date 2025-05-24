#!/bin/bash
#
# start_quantus_node.sh: Starts the quantus-node, configured to join a specified chain.
#
# This script is designed to be configurable. Edit the variables in the
# --- Configuration --- section below to suit your needs.
#

set -e # Exit immediately if a command exits with a non-zero status.
set -u # Treat unset variables as an error when substituting.

# --- Configuration ---
# Directory for storing rewards address file and other local config
CONFIG_DIR="$HOME/.quantus-node"
REWARDS_ADDRESS_FILE="$CONFIG_DIR/rewards_address.txt"
NODE_KEY_FILE="$CONFIG_DIR/node_key.p2p" # File to store/read the node's libp2p private key
CREATE_ADDRESS_SCRIPT_PATH="$(dirname "$0")/create_quantus_address.sh"
CREATE_NODE_KEY_SCRIPT_PATH="$(dirname "$0")/create_node_identity.sh"

# Node Binary: Assumes 'quantus-node' is in PATH.
# You can set this to an absolute path, e.g., "$(dirname "$0")/../target/release/quantus-node"
# if you want to run a specific build.
NODE_BINARY="quantus-node"

# Node Identity and Network
NODE_NAME="MyLocalQuantusTestnetNode-$(LC_ALL=C head /dev/urandom | LC_ALL=C tr -dc A-Za-z0-9 | head -c 4)" # Unique default name
CHAIN_SPEC_ID="live_resonance" # Chain spec to use (e.g., live_resonance, local, dev)

# Data Storage: Base path for node data.
# Data for different chains will be stored in subdirectories.
BASE_PATH_DIR_ROOT="$HOME/.quantus-node-data"
BASE_PATH_DIR="$BASE_PATH_DIR_ROOT/$CHAIN_SPEC_ID" # Chain-specific data path

# Network Ports
P2P_PORT="30334" # P2P listening port
RPC_PORT="9945"    # RPC listening port
# PROMETHEUS_PORT="9616" # Prometheus metrics port (Removed by user request)

# Logging: RUST_LOG can be set in the environment before running this script.
# Example: export RUST_LOG="info,sync=debug,network=debug"

# Miner Configuration
REWARDS_ADDRESS="" # Will be loaded from file or user input. Essential for mining/validation rewards.
EXTERNAL_MINER_URL="" # Set to "http://127.0.0.1:9833" if using an external QPoW miner

# Additional Node Arguments: Add other flags here
# Example: "--some-other-flag"
ADDITIONAL_NODE_ARGS=(
    "--validator"
    # "--unsafe-force-node-key-generation" # Removed: Node key should be pre-generated
    # "--node-key-file" "$NODE_KEY_FILE" # This will be added conditionally later
)

# --- Sanity Checks ---
if ! command -v "$NODE_BINARY" &>/dev/null; then
    echo -e "\033[1;31mERROR\033[0m: '$NODE_BINARY' command not found. Please ensure it is installed and in your PATH, or set NODE_BINARY variable correctly."
    exit 1
fi

# Ensure config directory exists
mkdir -p "$CONFIG_DIR"

# Ensure base path directory exists
mkdir -p "$BASE_PATH_DIR"
echo -e "\033[1;32mINFO\033[0m: Node data will be stored in: $BASE_PATH_DIR"

# --- Check for Node Key File ---
if [ ! -f "$NODE_KEY_FILE" ]; then
    echo -e "\033[1;31mERROR\033[0m: Node key file not found at $NODE_KEY_FILE."
    echo "Please run the node identity creation script to generate it:"
    echo "  ./${CREATE_NODE_KEY_SCRIPT_PATH##*/}"
    echo "Alternatively, ensure your node key is correctly placed at $NODE_KEY_FILE."
    if [ -z "${CI-}" ]; then # Only exit if not in CI, allow CI to potentially handle it differently or fail later
      exit 1
    else
      echo -e "\033[1;33mWARN\033[0m: CI environment detected, proceeding without node key. Node might fail to start or generate one."
    fi
else
    echo -e "\033[1;32mINFO\033[0m: Using node key from $NODE_KEY_FILE"
    # Add node-key-file to ADDITIONAL_NODE_ARGS only if the file exists
    ADDITIONAL_NODE_ARGS+=("--node-key-file" "$NODE_KEY_FILE")
fi

# --- Load or Prompt for Rewards Address ---
if [ -f "$REWARDS_ADDRESS_FILE" ]; then
    REWARDS_ADDRESS=$(cat "$REWARDS_ADDRESS_FILE")
    # Simple validation: check if it looks like a placeholder or is empty
    if [ -z "$REWARDS_ADDRESS" ] || [ "$REWARDS_ADDRESS" == "<YOUR_REWARDS_ADDRESS>" ]; then
        echo -e "\033[1;33mWARN\033[0m: Rewards address file ($REWARDS_ADDRESS_FILE) is empty or contains a placeholder."
        REWARDS_ADDRESS="" # Reset to trigger prompt if interactive
    else
        echo -e "\033[1;32mINFO\033[0m: Loaded rewards address from $REWARDS_ADDRESS_FILE: $REWARDS_ADDRESS"
    fi
fi

if [ -z "${REWARDS_ADDRESS}" ]; then # If still empty after trying to load
    if [ -n "${CI-}" ]; then # Non-interactive (e.g., CI environment)
        echo -e "\033[1;31mERROR\033[0m: Rewards address is not set and running in non-interactive mode."
        echo "Please create $REWARDS_ADDRESS_FILE with a valid address or set REWARDS_ADDRESS environment variable."
        exit 1
    fi

    echo -e "\033[1;33mWARN\033[0m: Rewards address is not set."
    echo "A Quantus address is needed to receive mining/validation rewards."
    echo "Choose an option:"
    echo "  1. Enter your existing Quantus rewards address manually."
    echo "  2. Generate a new rewards address (runs '${CREATE_ADDRESS_SCRIPT_PATH##*/}')"
    echo "  3. Exit to set it manually later (edit $REWARDS_ADDRESS_FILE or this script)."

    read -r -p "Enter your choice (1, 2, or 3): " choice

    case "$choice" in
        1)
            read -r -p "Enter your Quantus SS58 rewards address: " manual_address
            if [ -n "$manual_address" ]; then
                REWARDS_ADDRESS="$manual_address"
                echo "$REWARDS_ADDRESS" > "$REWARDS_ADDRESS_FILE"
                echo -e "\033[1;32mINFO\033[0m: Rewards address set to: $REWARDS_ADDRESS (and saved to $REWARDS_ADDRESS_FILE)"
            else
                echo -e "\033[1;31mERROR\033[0m: No address entered. Exiting."
                exit 1
            fi
            ;;
        2)
            if [ -f "$CREATE_ADDRESS_SCRIPT_PATH" ]; then
                echo "Running address generation script..."
                bash "$CREATE_ADDRESS_SCRIPT_PATH"
                if [ -f "$REWARDS_ADDRESS_FILE" ]; then
                    REWARDS_ADDRESS=$(cat "$REWARDS_ADDRESS_FILE")
                     if [ -z "$REWARDS_ADDRESS" ] || [ "$REWARDS_ADDRESS" == "<YOUR_REWARDS_ADDRESS>" ]; then
                        echo -e "\033[1;31mERROR\033[0m: Address generation script ran, but $REWARDS_ADDRESS_FILE is still empty or placeholder. Exiting."
                        exit 1
                    fi
                    echo -e "\033[1;32mINFO\033[0m: Loaded new rewards address from $REWARDS_ADDRESS_FILE: $REWARDS_ADDRESS"
                else
                     echo -e "\033[1;31mERROR\033[0m: Address file not found after running generation script. Exiting."
                     exit 1
                fi
            else
                echo -e "\033[1;31mERROR\033[0m: Address generation script not found at $CREATE_ADDRESS_SCRIPT_PATH. Exiting."
                exit 1
            fi
            ;;
        3)
            echo "Exiting. Please set your REWARDS_ADDRESS by creating $REWARDS_ADDRESS_FILE or editing this script."
            exit 0
            ;;
        *)
            echo -e "\033[1;31mERROR\033[0m: Invalid choice. Exiting."
            exit 1
            ;;
    esac
fi

if [[ " ${ADDITIONAL_NODE_ARGS[*]} " == *" --validator "* ]] && [[ -z "$REWARDS_ADDRESS" ]]; then
    echo -e "\033[1;31mERROR\033[0m: Running as a validator requires a rewards address. Rewards address is currently empty."
    echo "Please ensure $REWARDS_ADDRESS_FILE contains a valid address or provide it interactively."
    exit 1
fi

# --- Construct and Run Command ---
# Start with the node binary
CMD_ARRAY=("$NODE_BINARY")

# Add chain spec
CMD_ARRAY+=("--chain" "$CHAIN_SPEC_ID")

# Add base path
CMD_ARRAY+=("--base-path" "$BASE_PATH_DIR")

# Add node name
CMD_ARRAY+=("--name" "$NODE_NAME")

# Add P2P port
CMD_ARRAY+=("--port" "$P2P_PORT")

# Add RPC port
CMD_ARRAY+=("--rpc-port" "$RPC_PORT")

# Add Prometheus port (Removed by user request)
# if [ -n "${PROMETHEUS_PORT-}" ]; then
#     CMD_ARRAY+=("--prometheus-port" "$PROMETHEUS_PORT")
# fi

# Add rewards address if set
if [ -n "$REWARDS_ADDRESS" ]; then
    CMD_ARRAY+=("--rewards-address" "$REWARDS_ADDRESS")
fi

# Add external miner URL if set
if [ -n "$EXTERNAL_MINER_URL" ]; then
    CMD_ARRAY+=("--external-miner-url" "$EXTERNAL_MINER_URL")
fi

# Add any additional arguments from the array
CMD_ARRAY+=("${ADDITIONAL_NODE_ARGS[@]}")

# DO NOT add explicit --bootnodes here if relying on chain_spec.json
# If you need to override bootnodes, add them to ADDITIONAL_NODE_ARGS:
# ADDITIONAL_NODE_ARGS+=("--bootnodes" "/dns/example.com/..." "/dns/another.com/...")

echo "Starting $NODE_BINARY with command:"
# Properly quote arguments for display and execution
# Using printf for safer command echoing
printf "RUST_LOG=\"%s\" " "${RUST_LOG:-NOT SET}" # Print RUST_LOG if set, otherwise indicate it's not set
printf "%q " "${CMD_ARRAY[@]}"
echo # Newline
echo "-----------------------------------------------------"

# Execute the command
# 'exec' replaces the script process with the node process.
# Set a default RUST_LOG if not already set in the environment
export RUST_LOG="${RUST_LOG:-info,sync=debug,network=debug,libp2p_gossipsub=debug}"
exec "${CMD_ARRAY[@]}" 