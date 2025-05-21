#!/bin/bash
#
# stop-quantus-node.sh: Stops all running instances of quantus-node.
#

set -e # Exit immediately if a command exits with a non-zero status.

BINARY_NAME="quantus-node"

# Check if the process is running
if ! pgrep -x "$BINARY_NAME" > /dev/null; then
    echo "INFO: '${BINARY_NAME}' is not running."
    exit 0
fi

echo "Attempting to stop '${BINARY_NAME}' by sending SIGTERM..."

# Try to stop gracefully first using pkill (sends SIGTERM by default)
# pkill is generally available on Linux and macOS.
# Use -x for exact match of the name.
if pkill -x "$BINARY_NAME"; then
    echo "SIGTERM signal sent to '${BINARY_NAME}'. Waiting a few seconds for graceful shutdown..."
    # Wait for a few seconds to allow graceful shutdown
    # Check if it's still running
    COUNT=0
    while pgrep -x "$BINARY_NAME" > /dev/null && [ "$COUNT" -lt 10 ]; do
        sleep 1
        COUNT=$((COUNT + 1))
        echo -n "."
    done
    echo ""

    if ! pgrep -x "$BINARY_NAME" > /dev/null; then
        echo "INFO: '${BINARY_NAME}' stopped successfully."
        exit 0
    else
        echo "WARN: '${BINARY_NAME}' did not stop gracefully after 10 seconds with SIGTERM."
        echo "Attempting to force stop with SIGKILL..."
        if pkill -KILL -x "$BINARY_NAME"; then
            echo "INFO: '${BINARY_NAME}' force stopped with SIGKILL."
            exit 0
        else
            echo "ERROR: Failed to force stop '${BINARY_NAME}' with SIGKILL. It might still be running or was already stopped."
            exit 1
        fi
    fi
else
    # This case might be hit if pkill fails for permission reasons, 
    # or if the process terminated between the pgrep check and pkill.
    if ! pgrep -x "$BINARY_NAME" > /dev/null; then
        echo "INFO: '${BINARY_NAME}' was not running or stopped before pkill could act."
        exit 0
    else
        echo "ERROR: Failed to send SIGTERM to '${BINARY_NAME}'. It might still be running."
        exit 1
    fi
fi 