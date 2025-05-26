#!/bin/bash

set -e

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"

# Configuration
NODE_BINARY_URL="https://github.com/quantus-network/quantus-node/releases/latest/download/quantus-node"
NODE_BINARY_PATH="/usr/local/bin/quantus-node"
NODE_IDENTITY_PATH="$HOME/.quantus/node-identity.json"
REWARDS_ADDRESS_PATH="$HOME/.quantus/rewards-address.txt"
QUANTUS_HOME="$HOME/.quantus"

# Function to check if running as root
check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo "This script needs to be run as root to install the node binary in /usr/local/bin"
        echo "Please run: sudo $0"
        exit 1
    fi
}

# Function to download and install the node binary
install_node_binary() {
    echo "Downloading latest Quantus node binary..."
    # Create a temporary directory for the download
    TEMP_DIR=$(mktemp -d)
    TEMP_BINARY="$TEMP_DIR/quantus-node"
    
    # Download to temporary location first
    curl -L "$NODE_BINARY_URL" -o "$TEMP_BINARY"
    chmod +x "$TEMP_BINARY"
    
    # Move to final location
    mv "$TEMP_BINARY" "$NODE_BINARY_PATH"
    rm -rf "$TEMP_DIR"
    
    echo "Node binary installed successfully at $NODE_BINARY_PATH"
}

# Function to handle node identity setup
setup_node_identity() {
    echo "Checking node identity setup..."
    if [ ! -f "$NODE_IDENTITY_PATH" ]; then
        echo "No node identity file found at $NODE_IDENTITY_PATH"
        echo "Would you like to:"
        echo "A) Provide a path to an existing node identity file"
        echo "B) Generate a new node identity"
        read -p "Enter your choice (A/B): " choice

        case $choice in
            A|a)
                read -p "Enter the path to your node identity file: " identity_path
                if [ -f "$identity_path" ]; then
                    cp "$identity_path" "$NODE_IDENTITY_PATH"
                    echo "Node identity file copied to $NODE_IDENTITY_PATH"
                else
                    echo "Error: File not found at $identity_path"
                    exit 1
                fi
                ;;
            B|b)
                echo "Generating new node identity..."
                if ! command -v "$NODE_BINARY_PATH" &> /dev/null; then
                    echo "Error: Node binary not found at $NODE_BINARY_PATH"
                    exit 1
                fi
                $NODE_BINARY_PATH key generate-node-identity --output "$NODE_IDENTITY_PATH"
                echo "New node identity generated and saved to $NODE_IDENTITY_PATH"
                ;;
            *)
                echo "Invalid choice"
                exit 1
                ;;
        esac
    else
        echo "Node identity file already exists at $NODE_IDENTITY_PATH"
    fi
}

# Function to handle rewards address setup
setup_rewards_address() {
    echo "Checking rewards address setup..."
    if [ ! -f "$REWARDS_ADDRESS_PATH" ]; then
        echo "No rewards address found at $REWARDS_ADDRESS_PATH"
        echo "Would you like to:"
        echo "A) Provide an existing rewards address"
        echo "B) Generate a new rewards address"
        read -p "Enter your choice (A/B): " choice

        case $choice in
            A|a)
                read -p "Enter your rewards address: " address
                echo "$address" > "$REWARDS_ADDRESS_PATH"
                echo "Rewards address saved to $REWARDS_ADDRESS_PATH"
                ;;
            B|b)
                echo "Generating new rewards address..."
                if ! command -v "$NODE_BINARY_PATH" &> /dev/null; then
                    echo "Error: Node binary not found at $NODE_BINARY_PATH"
                    exit 1
                fi
                # Generate new address and capture all output
                output=$($NODE_BINARY_PATH key generate --scheme standard)
                
                # Extract the address (assuming it's the last line)
                address=$(echo "$output" | grep "Address:" | awk '{print $2}')
                
                # Save only the address to the file
                echo "$address" > "$REWARDS_ADDRESS_PATH"
                
                # Display all details to the user
                echo "New rewards address generated. Please save these details securely:"
                echo "$output"
                echo "Address has been saved to $REWARDS_ADDRESS_PATH"
                ;;
            *)
                echo "Invalid choice"
                exit 1
                ;;
        esac
    else
        echo "Rewards address already exists at $REWARDS_ADDRESS_PATH"
    fi
}

# Main installation process
echo "Starting Quantus node installation..."

# Check if running as root
check_root

# Create Quantus home directory if it doesn't exist
echo "Creating Quantus home directory at $QUANTUS_HOME"
mkdir -p "$QUANTUS_HOME"

# Install node binary
install_node_binary

# Verify node binary is installed and executable
if ! command -v "$NODE_BINARY_PATH" &> /dev/null; then
    echo "Error: Node binary not found at $NODE_BINARY_PATH after installation"
    exit 1
fi

# Setup node identity
setup_node_identity

# Setup rewards address
setup_rewards_address

echo "Installation completed successfully!"
echo "Node binary: $NODE_BINARY_PATH"
echo "Node identity: $NODE_IDENTITY_PATH"
echo "Rewards address: $REWARDS_ADDRESS_PATH" 