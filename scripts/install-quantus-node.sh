#!/bin/bash

# Install the Quantus node binary
#
# USAGE:
#   ./install-quantus-node.sh
#
# This script will install the Quantus node binary, create a node identity file, and create a rewards address file.
# It will also create the Quantus home directory.

set -e

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"

# Configuration
REPO_OWNER="Quantus-Network"
REPO_NAME="chain"
BINARY_NAME="quantus-node"
QUANTUS_HOME="$HOME/.quantus"
NODE_IDENTITY_PATH="$QUANTUS_HOME/node_key.p2p"
REWARDS_ADDRESS_PATH="$QUANTUS_HOME/rewards-address.txt"

# Detect OS and Architecture
OS=""
ARCH=""
TARGET_ARCH_NAME=""

case "$(uname -s)" in
    Linux*)
        OS="linux"
        ;;
    Darwin*)
        OS="macos"
        ;;
    *)
        echo "Unsupported operating system: $(uname -s)"
        exit 1
        ;;
esac

case "$(uname -m)" in
    x86_64|amd64)
        ARCH="x86_64"
        if [ "$OS" = "linux" ]; then
            TARGET_ARCH_NAME="x86_64-unknown-linux-gnu"
        elif [ "$OS" = "macos" ]; then
            TARGET_ARCH_NAME="x86_64-apple-darwin"
        fi
        ;;
    arm64|aarch64)
        ARCH="arm64"
        if [ "$OS" = "linux" ]; then
            TARGET_ARCH_NAME="aarch64-unknown-linux-gnu"
        elif [ "$OS" = "macos" ]; then
            TARGET_ARCH_NAME="aarch64-apple-darwin"
        fi
        ;;
    *)
        echo "Unsupported architecture: $(uname -m)"
        exit 1
        ;;
esac

echo "Detected OS: $OS"
echo "Detected Architecture: $ARCH (Target: $TARGET_ARCH_NAME)"

# Determine installation path based on root status
if [ "$EUID" -eq 0 ]; then
    NODE_BINARY_PATH="/usr/local/bin/quantus-node"
else
    NODE_BINARY_PATH="$HOME/.local/bin/quantus-node"
    # Create .local/bin if it doesn't exist
    mkdir -p "$(dirname "$NODE_BINARY_PATH")"
    echo "Installing node binary to user directory: $NODE_BINARY_PATH"
    echo "Note: Make sure $HOME/.local/bin is in your PATH"
    echo "You can add it by running:"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc  # for bash"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc   # for zsh"
    echo "Then source your shell config file or open a new terminal"
fi

# Function to download and install the node binary
install_node_binary() {
    echo "Downloading latest Quantus node binary..."
    # Create a temporary directory for the download
    TEMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TEMP_DIR"' EXIT

    # Get the latest release info
    echo "Fetching latest release information..."
    LATEST_RELEASE=$(curl -s https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest)
    if [ -z "$LATEST_RELEASE" ]; then
        echo "Error: Failed to fetch latest release information"
        exit 1
    fi

    LATEST_TAG=$(echo "$LATEST_RELEASE" | grep -o '"tag_name": "[^"]*"' | head -n 1 | cut -d'"' -f4)
    if [ -z "$LATEST_TAG" ]; then
        echo "Error: Could not find latest release tag"
        exit 1
    fi

    echo "Latest release tag: $LATEST_TAG"
    
    # Construct the asset filename and URL
    ASSET_FILENAME="${BINARY_NAME}-${LATEST_TAG}-${TARGET_ARCH_NAME}.tar.gz"
    ASSET_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${LATEST_TAG}/${ASSET_FILENAME}"
    
    echo "Attempting to download asset: $ASSET_URL"
    
    # Download the asset
    if ! curl -L "$ASSET_URL" -o "$TEMP_DIR/$ASSET_FILENAME"; then
        echo "Error: Failed to download node binary"
        exit 1
    fi
    
    echo "Asset downloaded to $TEMP_DIR/$ASSET_FILENAME"
    
    # Extract the binary
    echo "Extracting $BINARY_NAME from $TEMP_DIR/$ASSET_FILENAME..."
    tar -xzf "$TEMP_DIR/$ASSET_FILENAME" -C "$TEMP_DIR"
    
    # Verify extracted binary
    if [ ! -f "$TEMP_DIR/$BINARY_NAME" ]; then
        echo "Error: Failed to extract $BINARY_NAME from the archive"
        exit 1
    fi
    
    chmod +x "$TEMP_DIR/$BINARY_NAME"
    
    # Move to final location
    mv "$TEMP_DIR/$BINARY_NAME" "$NODE_BINARY_PATH"
    
    echo "Node binary installed successfully at $NODE_BINARY_PATH"
}

# Function to handle node identity setup
setup_node_identity() {
    echo "Checking node identity setup..."
    if [ ! -f "$NODE_IDENTITY_PATH" ]; then
        echo "No node identity file found at $NODE_IDENTITY_PATH"
        echo "Would you like to:"
        echo "[1] Provide a path to an existing node identity file"
        echo "[2] Generate a new node identity"
        read -p "Enter your choice (1/2): " choice

        case $choice in
            1)
                read -p "Enter the path to your node identity file: " identity_path
                if [ -f "$identity_path" ]; then
                    cp "$identity_path" "$NODE_IDENTITY_PATH"
                    echo "Node identity file copied to $NODE_IDENTITY_PATH"
                else
                    echo "Error: File not found at $identity_path"
                    exit 1
                fi
                ;;
            2)
                echo "Generating new node identity..."
                if ! command -v "$NODE_BINARY_PATH" &> /dev/null; then
                    echo "Error: Node binary not found at $NODE_BINARY_PATH"
                    exit 1
                fi
                $NODE_BINARY_PATH key generate-node-key --file "$NODE_IDENTITY_PATH"
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

secret_phrase=""

# Function to handle rewards address setup
setup_rewards_address() {
    echo "Checking rewards address setup..."
    if [ ! -f "$REWARDS_ADDRESS_PATH" ]; then
        echo "No rewards address found at $REWARDS_ADDRESS_PATH"
        echo "Would you like to:"
        echo "[1] Provide an existing rewards address"
        echo "[2] Generate a new rewards address"
        read -p "Enter your choice (1/2): " choice

        case $choice in
            1)
                read -p "Enter your rewards address: " address
                echo "$address" > "$REWARDS_ADDRESS_PATH"
                echo "Rewards address saved to $REWARDS_ADDRESS_PATH"
                ;;
            2)
                echo "Generating new rewards address..."
                if ! command -v "$NODE_BINARY_PATH" &> /dev/null; then
                    echo "Error: Node binary not found at $NODE_BINARY_PATH"
                    exit 1
                fi
                # Generate new address and capture all output
                output=$($NODE_BINARY_PATH key quantus)
                
                # Extract the address (assuming it's the last line)
                address=$(echo "$output" | grep "Address:" | awk '{print $2}')
                
                # Secret phrase: shadow valve wild recall jeans blush mandate diagram recall slide alley water wealth transfer soup fit above army crisp involve level trust rabbit panda
                line=$(printf '%s\n' "$output" | grep "Secret phrase:")
                # strip everything up through "Secret phrase: "
                secret_phrase="${line#*Secret phrase: }"
                
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
if [ "$secret_phrase" != "" ]; then 
  echo ""
  echo "PLEASE SAVE YOUR SECRET PHRASE IN A SAFE PLACE"
  echo "Secret phrase: $secret_phrase"
fi
echo ""
echo "To start mining Quantus node, run the following command:"
echo ""
cat <<EOF
$NODE_BINARY_PATH \\
  --node-key-file "$NODE_IDENTITY_PATH" \\
  --rewards-address "$REWARDS_ADDRESS_PATH" \\
  --validator \\
  --chain live_resonance \\
  --port 30333 \\
  --prometheus-port 9616 \\
  --name "ResonanceLiveTestnetNode" \\
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all"
EOF