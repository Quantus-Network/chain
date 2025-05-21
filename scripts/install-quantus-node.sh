#!/bin/bash
#
# install-quantus-node.sh: Installs the latest release of quantus-node.
#
# This script supports Linux and macOS, and architectures x86_64/amd64 and arm64/aarch64.
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/Quantus-Network/chain/main/scripts/install-quantus-node.sh | bash
#   or
#   ./install-quantus-node.sh
#
# It will attempt to install to $HOME/.local/bin by default.
# If $HOME/.local/bin is not in PATH, it will suggest adding it.
# If run with sudo, it will attempt to install to /usr/local/bin.

set -e # Exit immediately if a command exits with a non-zero status.
set -u # Treat unset variables as an error when substituting.
set -o pipefail # Return value of a pipeline is the status of the last command to exit with a non-zero status.

REPO_OWNER="Quantus-Network"
REPO_NAME="chain"
BINARY_NAME="quantus-node"

# --- Helper Functions ---
msg() {
  echo -e "\033[1;32mINFO\033[0m: $1"
}

warn() {
  echo -e "\033[1;33mWARN\033[0m: $1" >&2
}

err() {
  echo -e "\033[1;31mERROR\033[0m: $1" >&2
  exit 1
}

check_dep() {
  if ! command -v "$1" &>/dev/null; then
    err "Required dependency '$1' is not installed. Please install it and try again."
  fi
}

# --- Main Installation Logic ---

# 1. Check dependencies
check_dep "curl"
check_dep "jq"  # For parsing JSON from GitHub API
check_dep "tar" # For extracting the tarball

# 2. Detect OS and Architecture
OS=""
ARCH=""
TARGET_ARCH_NAME="" # For GitHub release asset naming (e.g., x86_64-unknown-linux-gnu)
ARCHIVE_EXTENSION="tar.gz"

case "$(uname -s)" in
  Linux*)
    OS="linux"
    ;;
  Darwin*)
    OS="macos"
    ;;
  *)
    err "Unsupported operating system: $(uname -s). Only Linux and macOS are supported."
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64)
    ARCH="x86_64"
    if [ "$OS" == "linux" ]; then
      TARGET_ARCH_NAME="x86_64-unknown-linux-gnu"
    elif [ "$OS" == "macos" ]; then
      TARGET_ARCH_NAME="x86_64-apple-darwin" # Assuming this is your macOS x86_64 target
    fi
    ;;
  arm64 | aarch64)
    ARCH="arm64" # Using 'arm64' as a common identifier, adjust if your release uses 'aarch64'
    if [ "$OS" == "linux" ]; then
      TARGET_ARCH_NAME="aarch64-unknown-linux-gnu" # Common for arm64 Linux
    elif [ "$OS" == "macos" ]; then
      TARGET_ARCH_NAME="aarch64-apple-darwin"
    fi
    ;;
  *)
    err "Unsupported architecture: $(uname -m)."
    ;;
esac

if [ -z "$TARGET_ARCH_NAME" ]; then
    err "Could not determine TARGET_ARCH_NAME for OS '$OS' and ARCH '$ARCH'."
fi

msg "Detected OS: $OS"
msg "Detected Arch: $ARCH (Target: $TARGET_ARCH_NAME)"

# 3. Fetch Latest Release Information
GITHUB_API_URL="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest"
msg "Fetching latest release information from $GITHUB_API_URL..."

# Use curl with -L to follow redirects, -s for silent, -f to fail fast on HTTP errors
LATEST_RELEASE_INFO=$(curl -sSfL "$GITHUB_API_URL")
if [ -z "$LATEST_RELEASE_INFO" ]; then
  err "Failed to fetch latest release information. Check network or repository URL."
fi

LATEST_TAG=$(echo "$LATEST_RELEASE_INFO" | jq -r '.tag_name')
if [ -z "$LATEST_TAG" ] || [ "$LATEST_TAG" == "null" ]; then
  err "Could not parse latest tag from GitHub API response."
fi
msg "Latest release tag: $LATEST_TAG"

# 4. Construct Asset URL and Download
ASSET_FILENAME="${BINARY_NAME}-${LATEST_TAG}-${TARGET_ARCH_NAME}.${ARCHIVE_EXTENSION}"
ASSET_DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${LATEST_TAG}/${ASSET_FILENAME}"

# Also try to get checksum file
CHECKSUM_FILENAME="sha256sums-${LATEST_TAG}-${TARGET_ARCH_NAME}.txt" # Based on your workflow
CHECKSUM_DOWNLOAD_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${LATEST_TAG}/${CHECKSUM_FILENAME}"


msg "Attempting to download asset: $ASSET_DOWNLOAD_URL"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT # Cleanup temp dir on exit

curl -sSfL "$ASSET_DOWNLOAD_URL" -o "$TEMP_DIR/$ASSET_FILENAME"
msg "Asset downloaded to $TEMP_DIR/$ASSET_FILENAME"

# 5. (Optional but Recommended) Download and Verify Checksum
DOWNLOADED_CHECKSUM=""
EXPECTED_CHECKSUM=""

if curl -sSfL "$CHECKSUM_DOWNLOAD_URL" -o "$TEMP_DIR/$CHECKSUM_FILENAME"; then
  msg "Checksum file downloaded: $TEMP_DIR/$CHECKSUM_FILENAME"
  # Assuming the checksum file format is: <checksum_hash> *<asset_filename_or_binary_name>
  # Or simply: <checksum_hash>  <asset_filename_or_binary_name> (double space)
  # We need to extract the hash for our specific asset.
  # The checksum file might contain checksums for binary AND archive. We need the archive one.
  
  # Try to grep for the asset filename. The star might need escaping if literal.
  # Grep for lines containing the asset filename, then extract the first field (the hash).
  EXPECTED_CHECKSUM=$(grep -E "\b${ASSET_FILENAME}\b" "$TEMP_DIR/$CHECKSUM_FILENAME" | awk '{print $1}' | head -n 1)

  if [ -n "$EXPECTED_CHECKSUM" ]; then
    msg "Expected checksum for $ASSET_FILENAME: $EXPECTED_CHECKSUM"
    
    # Calculate checksum of the downloaded asset
    # sha256sum command differs on Linux vs macOS
    if [ "$OS" == "linux" ]; then
      DOWNLOADED_CHECKSUM=$(sha256sum "$TEMP_DIR/$ASSET_FILENAME" | awk '{print $1}')
    elif [ "$OS" == "macos" ]; then
      DOWNLOADED_CHECKSUM=$(shasum -a 256 "$TEMP_DIR/$ASSET_FILENAME" | awk '{print $1}')
    fi
    msg "Calculated checksum: $DOWNLOADED_CHECKSUM"

    if [ "$DOWNLOADED_CHECKSUM" == "$EXPECTED_CHECKSUM" ]; then
      msg "Checksum verification successful."
    else
      err "Checksum verification FAILED! Expected '$EXPECTED_CHECKSUM' but got '$DOWNLOADED_CHECKSUM'. Aborting."
    fi
  else
    warn "Could not find checksum for '$ASSET_FILENAME' in '$CHECKSUM_FILENAME'. Skipping checksum verification."
  fi
else
  warn "Checksum file '$CHECKSUM_FILENAME' not found or failed to download. Skipping checksum verification."
fi


# 6. Extract Binary
msg "Extracting $BINARY_NAME from $TEMP_DIR/$ASSET_FILENAME..."
if [ "$ARCHIVE_EXTENSION" == "tar.gz" ]; then
  # tar -xzf "$TEMP_DIR/$ASSET_FILENAME" -C "$TEMP_DIR" "$BINARY_NAME" # If binary is at top level
  # If binary is inside a folder named like the asset (minus extension), or target-specific folder.
  # The create-release.yml workflow does: (cd staging && tar -czvf "../${asset_name}" ${NODE_BINARY_NAME})
  # This means the binary should be at the root of the tarball.
  tar -xzf "$TEMP_DIR/$ASSET_FILENAME" -C "$TEMP_DIR"
  # Verify extracted binary
  if [ ! -f "$TEMP_DIR/$BINARY_NAME" ]; then
    err "Failed to extract $BINARY_NAME from the archive. It was not found at the root."
  fi
  chmod +x "$TEMP_DIR/$BINARY_NAME"
else
  err "Unsupported archive extension: $ARCHIVE_EXTENSION" # Placeholder for .zip if needed
fi
msg "$BINARY_NAME extracted to $TEMP_DIR/$BINARY_NAME"

# 7. Determine Installation Path and Install
INSTALL_DIR=""
ASK_FOR_SUDO=false

if [ "$(id -u)" == "0" ]; then # Running as root (e.g. via sudo)
  INSTALL_DIR="/usr/local/bin"
else
  # Default to user-local installation
  INSTALL_DIR="$HOME/.local/bin"
  # Check if running with sudo explicitly (even if not root yet, e.g. sudo bash install.sh)
  # This is tricky; `ps` might show it. A simpler check is if $SUDO_USER is set.
  if [ -n "${SUDO_USER-}" ]; then # SUDO_USER is set by sudo
      INSTALL_DIR="/usr/local/bin" # User invoked with sudo, prefer /usr/local/bin
      ASK_FOR_SUDO=false # Already have sudo or will be prompted by mv
  fi
fi

# Ensure install directory exists
if [ ! -d "$INSTALL_DIR" ]; then
  msg "Installation directory $INSTALL_DIR does not exist. Attempting to create it."
  if [ "$INSTALL_DIR" == "/usr/local/bin" ] && [ "$(id -u)" != "0" ]; then
    # Need sudo to create /usr/local/bin if it does not exist and not root
    # However, 'mv' later will ask for sudo if needed for /usr/local/bin
    # So we let 'mv' handle sudo prompt for simplicity, unless mkdir itself fails.
    # Better: ask for sudo explicitly for mkdir.
    msg "This requires sudo privileges."
    if ! sudo mkdir -p "$INSTALL_DIR"; then
        err "Failed to create $INSTALL_DIR even with sudo. Please create it manually and try again."
    fi
    if ! sudo chown "$(id -u):$(id -g)" "$INSTALL_DIR"; then # Ensure user owns if they sudo created it
        warn "Could not chown $INSTALL_DIR to current user after sudo mkdir."
    fi
  else
     mkdir -p "$INSTALL_DIR" # For $HOME/.local/bin or if already root for /usr/local/bin
  fi
fi

msg "Installing $BINARY_NAME to $INSTALL_DIR..."
INSTALL_CMD="mv "$TEMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME""

if [ "$INSTALL_DIR" == "/usr/local/bin" ] && [ "$(id -u)" != "0" ]; then
  if command -v sudo &>/dev/null; then
    msg "Installation to $INSTALL_DIR requires sudo privileges."
    sudo $INSTALL_CMD
  else
    err "sudo is required to install to $INSTALL_DIR, but sudo command not found. Please install as root or choose a different path."
  fi
else
  eval "$INSTALL_CMD" # Run directly if $HOME/.local/bin or already root
fi

# 8. Verify Installation and PATH
if ! command -v "$BINARY_NAME" &>/dev/null || [ "$(command -v "$BINARY_NAME")" != "$INSTALL_DIR/$BINARY_NAME" ]; then
  warn "$BINARY_NAME installed to $INSTALL_DIR/$BINARY_NAME, but this location might not be in your PATH."
  warn "Please add $INSTALL_DIR to your PATH environment variable. For bash/zsh, you can add this to your ~/.bashrc or ~/.zshrc:"
  warn "  export PATH=\"$INSTALL_DIR:\$PATH\""
  warn "Then source the file or open a new terminal."
else
  msg "$BINARY_NAME installed successfully to $INSTALL_DIR/$BINARY_NAME and is in your PATH."
fi

msg "Installation complete!"
msg "You can now run '$BINARY_NAME --version' (if implemented) or '$BINARY_NAME --help'."
msg "For mining, please refer to the documentation for appropriate command-line flags and configuration."

# Cleanup is handled by trap
exit 0 