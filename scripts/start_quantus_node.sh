#!/bin/bash
#
# start-quantus-node.sh: Starts the quantus-node with typical mining configurations.
#
# IMPORTANT:
# 1. Ensure 'quantus-node' binary is in your PATH (installed via install-quantus-node.sh).
# 2. Customize the parameters below to match your desired mining setup.
#

set -e # Exit immediately if a command exits with a non-zero status.
set -u # Treat unset variables as an error when substituting.

# --- Configuration ---
CONFIG_DIR="$HOME/.quantus-node"
REWARDS_ADDRESS_FILE="$CONFIG_DIR/rewards_address.txt"
CREATE_ADDRESS_SCRIPT_PATH="$(dirname "$0")/create-quantus-address.sh"

# Node Name (Optional, for identification in telemetry or UIs if supported)
# NODE_NAME="MyQuantusMiner"

# Chain Specification (e.g., --chain mainnet, --chain testnet, or --dev for local development)
# Using --dev as a placeholder. For actual mining, you'll likely specify a chain.
CHAIN_FLAG="--dev"

# Rewards Address (Crucial for mining)
# Replace <YOUR_REWARDS_ADDRESS> with your actual address.
REWARDS_ADDRESS="" # Will be loaded from file or user input

# External Miner URL (If you're using an external miner service)
# If not using an external miner, you might remove or comment out this flag.
EXTERNAL_MINER_URL="http://127.0.0.1:9833" # Default from your README

# Base Path for node data (Optional, defaults to a platform-specific directory)
# BASE_PATH_FLAG="--base-path /path/to/my/quantus-data"

# Logging Configuration (RUST_LOG environment variable)
# Example: "info,sc_consensus_pow=debug" from your README
export RUST_LOG="${RUST_LOG:-info,sc_consensus_pow=debug}" # Use existing RUST_LOG or default

# Additional flags (Add any other flags required for your setup)
# ADDITIONAL_FLAGS="--validator --rpc-port 9945"

# --- Sanity Checks ---
if ! command -v quantus-node &>/dev/null; then
    echo -e "\033[1;31mERROR\033[0m: 'quantus-node' command not found. Please ensure it is installed and in your PATH."
    exit 1
fi

# --- Load or Prompt for Rewards Address ---
if [ -f "$REWARDS_ADDRESS_FILE" ]; then
    REWARDS_ADDRESS=$(cat "$REWARDS_ADDRESS_FILE")
    echo -e "\033[1;32mINFO\033[0m: Loaded rewards address from $REWARDS_ADDRESS_FILE: $REWARDS_ADDRESS"
fi

if [ -z "$REWARDS_ADDRESS" ] || [ "$REWARDS_ADDRESS" == "<YOUR_REWARDS_ADDRESS>" ]; then
    echo -e "\033[1;33mWARN\033[0m: Rewards address is not set or is a placeholder."
    echo "You need a Quantus address to receive mining rewards."
    echo "Choose an option:"
    echo "  1. Enter your existing Quantus rewards address manually."
    echo "  2. Generate a new rewards address (runs '${CREATE_ADDRESS_SCRIPT_PATH##*/}')."
    echo "  3. Exit to set it manually later (edit this script or $REWARDS_ADDRESS_FILE)."
    
    read -p "Enter your choice (1, 2, or 3): " choice

    case "$choice" in
        1)
            read -p "Enter your Quantus SS58 rewards address: " manual_address
            if [ -n "$manual_address" ]; then
                REWARDS_ADDRESS="$manual_address"
                # Optionally save it back to the file for next time
                mkdir -p "$CONFIG_DIR"
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
                # Try to load the address again from the file
                if [ -f "$REWARDS_ADDRESS_FILE" ]; then
                    REWARDS_ADDRESS=$(cat "$REWARDS_ADDRESS_FILE")
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
            echo "Exiting. Please set your REWARDS_ADDRESS in this script or create $REWARDS_ADDRESS_FILE."
            exit 0
            ;;
        *)
            echo -e "\033[1;31mERROR\033[0m: Invalid choice. Exiting."
            exit 1
            ;;
    esac
fi

if [ -z "$REWARDS_ADDRESS" ] || [ "$REWARDS_ADDRESS" == "<YOUR_REWARDS_ADDRESS>" ]; then
    echo -e "\033[1;31mERROR\033[0m: REWARDS_ADDRESS is still not properly set. Exiting."
    exit 1
fi

# --- Construct and Run Command ---
CMD="quantus-node"
CMD+=" $CHAIN_FLAG"
CMD+=" --rewards-address $REWARDS_ADDRESS"

if [ -n "${EXTERNAL_MINER_URL-}" ]; then # Only add if EXTERNAL_MINER_URL is set and not empty
    CMD+=" --external-miner-url $EXTERNAL_MINER_URL"
fi

# if [ -n "${NODE_NAME-}" ]; then
#     CMD+=" --name \"$NODE_NAME\"" # Ensure proper quoting if name has spaces
# fi

# if [ -n "${BASE_PATH_FLAG-}" ]; then
#     CMD+=" $BASE_PATH_FLAG"
# fi

# if [ -n "${ADDITIONAL_FLAGS-}" ]; then
#     CMD+=" $ADDITIONAL_FLAGS"
# fi

echo "Starting quantus-node with command:"
echo "RUST_LOG="$RUST_LOG" $CMD"
echo "-----------------------------------------------------"

# Execute the command
eval "exec $CMD" 