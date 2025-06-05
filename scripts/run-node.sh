#!/usr/bin/env bash
set -euo pipefail

# Default values
DATA_DIR="${HOME}/.quantus"
NODE_NAME="quantus-node-$(hostname)"
DOCKER_IMAGE="quantus-node:latest" # local - ghcr.io/quantus-network/quantus-node:latest # global
MODE="full"  # full, validator
CHAIN="live_resonance"

# Help message
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo
    echo "Options:"
    echo "  -m, --mode MODE        Node mode: full (default) or validator"
    echo "  -n, --name NAME        Node name (default: quantus-node-hostname)"
    echo "  -d, --data-dir DIR     Data directory (default: ~/.quantus)"
    echo "  -r, --rewards ADDR     Rewards address (required for validator mode)"
    echo "  -c, --chain CHAIN      Chain to sync (default: live_resonance)"
    echo "  -i, --image IMAGE      Docker image (default: quantus-node:latest)"
    echo "  -h, --help            Show this help message"
    echo
    echo "Example:"
    echo "  $0 --mode validator --name my-validator --rewards 5G..."
    exit 1
}

# Parse arguments
REWARDS_ADDRESS=""
while [[ $# -gt 0 ]]; do
    case $1 in
        -m|--mode)
            MODE="$2"
            shift 2
            ;;
        -n|--name)
            NODE_NAME="$2"
            shift 2
            ;;
        -d|--data-dir)
            DATA_DIR="$2"
            shift 2
            ;;
        -r|--rewards)
            REWARDS_ADDRESS="$2"
            shift 2
            ;;
        -c|--chain)
            CHAIN="$2"
            shift 2
            ;;
        -i|--image)
            DOCKER_IMAGE="$2"
            shift 2
            ;;
        -h|--help)
            show_help
            ;;
        *)
            echo "Unknown option: $1"
            show_help
            ;;
    esac
done

# Validate arguments
if [[ "$MODE" == "validator" && -z "$REWARDS_ADDRESS" ]]; then
    echo "Error: Validator mode requires a rewards address (-r or --rewards)"
    exit 1
fi

if [[ ! "$MODE" =~ ^(full|validator)$ ]]; then
    echo "Error: Invalid mode '$MODE'. Must be 'full' or 'validator'"
    exit 1
fi

# Ensure data directory exists
mkdir -p "$DATA_DIR"

# Build the command
CMD="docker run -d \
    --name quantus-node \
    --restart unless-stopped \
    -p 30333:30333 \
    -p 9944:9944 \
    -v ${DATA_DIR}:/var/lib/quantus \
    ${DOCKER_IMAGE}"

# Add mode-specific arguments
case "$MODE" in
    validator)
        CMD="$CMD --validator \
            --base-path /var/lib/quantus \
            --chain ${CHAIN} \
            --name ${NODE_NAME} \
            --rewards-address ${REWARDS_ADDRESS}"
        ;;
    full)
        CMD="$CMD \
            --base-path /var/lib/quantus \
            --chain ${CHAIN} \
            --name ${NODE_NAME}"
        ;;
esac

# Print the command (for debugging)
echo "Starting $MODE node with command:"
echo "$CMD"
echo

# Execute the command
eval "$CMD"

# Print helpful information
echo
echo "Node started in $MODE mode!"
echo "To view logs: docker logs -f quantus-node"
echo "To stop node: docker stop quantus-node"
echo "To remove container: docker rm quantus-node"
echo
echo "Data directory: $DATA_DIR"
echo "P2P port: 30333"
echo "RPC port: 9944" 