#!/bin/bash

# Test script for multi-core external miner with dev chain
# This script starts the chain in dev mode and external miner with 8 cores

set -e

echo "🚀 Starting Resonance Network Dev Test with Multi-Core External Miner"
echo "=================================================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MINER_CORES=8
MINER_PORT=9833
NODE_RPC_PORT=9944

echo -e "${BLUE}Configuration:${NC}"
echo -e "  Miner cores: ${GREEN}${MINER_CORES}${NC}"
echo -e "  Miner port: ${GREEN}${MINER_PORT}${NC}"
echo -e "  Node RPC port: ${GREEN}${NODE_RPC_PORT}${NC}"
echo ""

# Function to cleanup background processes
cleanup() {
    echo -e "\n${YELLOW}Cleaning up processes...${NC}"
    if [ ! -z "$NODE_PID" ]; then
        echo "Stopping node (PID: $NODE_PID)"
        kill $NODE_PID 2>/dev/null || true
    fi
    if [ ! -z "$MINER_PID" ]; then
        echo "Stopping miner (PID: $MINER_PID)"
        kill $MINER_PID 2>/dev/null || true
    fi
    exit 0
}

# Set trap to cleanup on script exit
trap cleanup SIGINT SIGTERM EXIT

# Build the project first
echo -e "${BLUE}Building project...${NC}"
cargo build --release
if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Build failed!${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build completed${NC}"
echo ""

# Start the external miner
echo -e "${BLUE}Starting external miner with ${MINER_CORES} cores...${NC}"
cd external-miner
cargo run --release -- --cores ${MINER_CORES} --port ${MINER_PORT} &
MINER_PID=$!
cd ..

# Wait a moment for miner to start
sleep 3

# Check if miner is running
if ! kill -0 $MINER_PID 2>/dev/null; then
    echo -e "${RED}❌ External miner failed to start!${NC}"
    exit 1
fi

echo -e "${GREEN}✅ External miner started (PID: ${MINER_PID})${NC}"
echo ""

# Start the node in dev mode
echo -e "${BLUE}Starting node in dev mode...${NC}"
./target/release/node-template \
    --dev \
    --tmp \
    --rpc-cors all \
    --rpc-methods unsafe \
    --rpc-port ${NODE_RPC_PORT} \
    --external-miner-url "http://127.0.0.1:${MINER_PORT}" &
NODE_PID=$!

# Wait a moment for node to start
sleep 5

# Check if node is running
if ! kill -0 $NODE_PID 2>/dev/null; then
    echo -e "${RED}❌ Node failed to start!${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Node started in dev mode (PID: ${NODE_PID})${NC}"
echo ""

# Check miner endpoint
echo -e "${BLUE}Testing miner endpoint...${NC}"
MINER_RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:${MINER_PORT}/result/test 2>/dev/null || echo "000")

if [ "$MINER_RESPONSE" = "404" ]; then
    echo -e "${GREEN}✅ Miner HTTP endpoint responding${NC}"
else
    echo -e "${YELLOW}⚠️  Miner endpoint response: ${MINER_RESPONSE}${NC}"
fi
echo ""

# Display status
echo -e "${GREEN}🎉 SYSTEM READY!${NC}"
echo "=================================================================="
echo -e "${BLUE}Status:${NC}"
echo -e "  📦 Node: Running (PID: ${NODE_PID})"
echo -e "  ⛏️  Miner: Running with ${MINER_CORES} cores (PID: ${MINER_PID})"
echo -e "  🌐 RPC: http://127.0.0.1:${NODE_RPC_PORT}"
echo -e "  🔗 Miner API: http://127.0.0.1:${MINER_PORT}"
echo ""
echo -e "${BLUE}What to watch for:${NC}"
echo "  • Node should start mining blocks automatically"
echo "  • Check node logs for mining requests to external miner"
echo "  • Check miner logs for incoming mining jobs and results"
echo "  • Blocks should appear in the node output"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop both processes${NC}"
echo "=================================================================="

# Monitor logs
echo -e "\n${BLUE}Monitoring logs (last 50 lines):${NC}"
echo "=================================================================="

# Function to show recent logs
show_logs() {
    echo -e "\n${BLUE}=== NODE LOGS ===${NC}"
    if [ -f /tmp/node.log ]; then
        tail -n 20 /tmp/node.log
    else
        echo "Node logs not available yet..."
    fi
    
    echo -e "\n${BLUE}=== MINER LOGS ===${NC}"
    if [ -f /tmp/miner.log ]; then
        tail -n 20 /tmp/miner.log
    else
        echo "Miner logs not available yet..."
    fi
}

# Wait and monitor
while true; do
    sleep 30
    show_logs
    echo -e "\n${GREEN}--- System still running, waiting 30s for next update ---${NC}"
    
    # Check if processes are still alive
    if ! kill -0 $NODE_PID 2>/dev/null; then
        echo -e "${RED}❌ Node process died!${NC}"
        break
    fi
    if ! kill -0 $MINER_PID 2>/dev/null; then
        echo -e "${RED}❌ Miner process died!${NC}"
        break
    fi
done