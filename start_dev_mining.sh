#!/bin/bash

# Simple script to start chain in dev mode and external miner with 8 cores

set -e

echo "Starting Resonance Network Dev Test"
echo "==================================="

# Build first
echo "Building..."
cargo build --release

# Start external miner with 8 cores
echo "Starting external miner with 8 cores..."
cd external-miner
cargo run --release -- --cores 8 --port 9833 &
MINER_PID=$!
cd ..

# Wait for miner to start
sleep 3

# Start node in dev mode
echo "Starting node in dev mode..."
./target/release/quantus-node \
    --dev \
    --tmp \
    --rpc-cors all \
    --rpc-methods unsafe \
    --rpc-port 9944 \
    --external-miner-url "http://127.0.0.1:9833" &
NODE_PID=$!

echo ""
echo "✅ Both processes started:"
echo "   Node PID: $NODE_PID"
echo "   Miner PID: $MINER_PID"
echo ""
echo "Press Ctrl+C to stop..."

# Cleanup function
cleanup() {
    echo "Stopping processes..."
    kill $NODE_PID $MINER_PID 2>/dev/null || true
    exit 0
}

trap cleanup SIGINT SIGTERM

# Wait for processes
wait
