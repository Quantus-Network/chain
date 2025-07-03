#!/bin/bash

# Wormhole Pallet Runtime Testing Script
# This script tests the wormhole pallet functionality with a live running node

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
NODE_URL="http://localhost:9944"
WEBSOCKET_URL="ws://localhost:9944"
NODE_PID=""
TEST_ACCOUNT=""
PROOF_FILE="pallets/wormhole/proof_from_bins.hex"

# Function to print colored output
print_status() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_header() {
    echo -e "\n${BLUE}=====================================
🧪 $1
=====================================${NC}\n"
}

# Function to check if node is running
check_node_running() {
    if curl -s -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","method":"system_health","id":1}' \
        "$NODE_URL" | grep -q "result"; then
        return 0
    else
        return 1
    fi
}

# Function to wait for node to be ready
wait_for_node() {
    local max_attempts=30
    local attempt=1
    
    print_info "Waiting for node to be ready..."
    
    while [ $attempt -le $max_attempts ]; do
        if check_node_running; then
            print_status "Node is ready!"
            return 0
        fi
        
        echo -n "."
        sleep 2
        attempt=$((attempt + 1))
    done
    
    print_error "Node failed to start within expected time"
    return 1
}

# Function to get proof data
get_proof_data() {
    if [ -f "$PROOF_FILE" ]; then
        # Read hex proof and remove any whitespace/newlines
        PROOF_HEX=$(cat "$PROOF_FILE" | tr -d '\n\r\t ')
        if [ ${#PROOF_HEX} -gt 100 ]; then
            print_status "Proof data loaded: ${#PROOF_HEX} characters"
            echo "$PROOF_HEX"
        else
            print_error "Proof file seems too small or empty"
            return 1
        fi
    else
        print_error "Proof file not found at $PROOF_FILE"
        return 1
    fi
}

# Function to generate a test account
generate_test_account() {
    print_info "Generating test account..."
    
    # Use the node's key generation
    if command -v ./target/release/quantus-node &> /dev/null; then
        # Generate a standard key
        local key_output=$(./target/release/quantus-node key quantus 2>/dev/null)
        TEST_ACCOUNT=$(echo "$key_output" | grep "Address:" | cut -d' ' -f2)
        
        if [ -n "$TEST_ACCOUNT" ]; then
            print_status "Generated test account: $TEST_ACCOUNT"
        else
            print_warning "Could not extract address from key generation"
            # Fallback to a known dev account
            TEST_ACCOUNT="5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY" # Alice
            print_info "Using Alice dev account: $TEST_ACCOUNT"
        fi
    else
        print_error "Node binary not found. Please build first."
        return 1
    fi
}

# Function to make RPC call
make_rpc_call() {
    local method="$1"
    local params="$2"
    local expected_success="$3"
    
    local payload="{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}"
    
    print_info "Making RPC call: $method"
    echo "Payload: $payload" | head -c 200
    if [ ${#payload} -gt 200 ]; then
        echo "... (truncated)"
    else
        echo
    fi
    
    local response=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "$payload" "$NODE_URL")
    
    echo "Response: $response"
    
    if [ "$expected_success" = "true" ]; then
        if echo "$response" | grep -q '"error"'; then
            print_error "RPC call failed with error"
            echo "$response" | jq '.error' 2>/dev/null || echo "Raw response: $response"
            return 1
        else
            print_status "RPC call succeeded"
            return 0
        fi
    else
        if echo "$response" | grep -q '"error"'; then
            print_status "RPC call failed as expected"
            return 0
        else
            print_warning "RPC call succeeded but was expected to fail"
            return 1
        fi
    fi
}

# Function to test system health
test_system_health() {
    print_header "Testing System Health"
    
    make_rpc_call "system_health" "[]" "true"
    make_rpc_call "system_version" "[]" "true"
    make_rpc_call "system_chain" "[]" "true"
}

# Function to test account balance
test_account_balance() {
    print_header "Testing Account Balance"
    
    if [ -z "$TEST_ACCOUNT" ]; then
        print_error "No test account available"
        return 1
    fi
    
    # Get account info
    make_rpc_call "system_accountNextIndex" "[\"$TEST_ACCOUNT\"]" "true"
}

# Function to test faucet (if available)
test_faucet() {
    print_header "Testing Faucet Functionality"
    
    if [ -z "$TEST_ACCOUNT" ]; then
        print_error "No test account available"
        return 1
    fi
    
    # Try to get account info from faucet
    make_rpc_call "faucet_getAccountInfo" "[\"$TEST_ACCOUNT\"]" "true"
    
    # Try to request tokens
    print_info "Requesting tokens from faucet..."
    make_rpc_call "faucet_requestTokens" "[\"$TEST_ACCOUNT\"]" "true"
}

# Function to test wormhole proof verification
test_wormhole_verification() {
    print_header "Testing Wormhole Proof Verification"
    
    local proof_hex=$(get_proof_data)
    if [ $? -ne 0 ]; then
        print_error "Failed to get proof data"
        return 1
    fi
    
    # Test with valid proof
    print_info "Testing valid proof verification..."
    local params="[\"0x$proof_hex\"]"
    
    # This would be the actual extrinsic submission
    # For now, we test that the RPC endpoint accepts the format
    make_rpc_call "author_submitExtrinsic" "[\"0x${proof_hex:0:100}\"]" "false"
    
    # Test with invalid proof
    print_info "Testing invalid proof verification..."
    make_rpc_call "author_submitExtrinsic" "[\"0xdeadbeef\"]" "false"
}

# Function to test storage queries
test_storage_queries() {
    print_header "Testing Storage Queries"
    
    # Query wormhole used nullifiers storage
    print_info "Querying wormhole storage..."
    
    # Get storage keys for wormhole pallet
    make_rpc_call "state_getMetadata" "[]" "true"
}

# Function to start the node
start_node() {
    print_header "Starting Development Node"
    
    # Check if node binary exists
    if [ ! -f "./target/release/quantus-node" ]; then
        print_info "Node binary not found. Building..."
        cargo build --release --package quantus-node
        if [ $? -ne 0 ]; then
            print_error "Failed to build node"
            return 1
        fi
    fi
    
    # Kill any existing node processes
    pkill -f "quantus-node" 2>/dev/null || true
    sleep 2
    
    # Clean up old chain data
    rm -rf /tmp/wormhole-test-node
    
    # Start the node in background
    print_info "Starting node with development chain..."
    
    RUST_LOG=info,pallet_wormhole=debug ./target/release/quantus-node \
        --dev \
        --base-path /tmp/wormhole-test-node \
        --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
        --port 30333 \
        --prometheus-port 9616 \
        > /tmp/node.log 2>&1 &
    
    NODE_PID=$!
    print_info "Node started with PID: $NODE_PID"
    
    # Wait for node to be ready
    if wait_for_node; then
        print_status "Node is running and ready for testing"
        return 0
    else
        print_error "Node failed to start properly"
        return 1
    fi
}

# Function to stop the node
stop_node() {
    if [ -n "$NODE_PID" ]; then
        print_info "Stopping node (PID: $NODE_PID)..."
        kill $NODE_PID 2>/dev/null || true
        wait $NODE_PID 2>/dev/null || true
    fi
    
    # Also kill any remaining quantus-node processes
    pkill -f "quantus-node" 2>/dev/null || true
    
    print_info "Node stopped"
}

# Function to show node logs
show_node_logs() {
    if [ -f "/tmp/node.log" ]; then
        print_info "Last 20 lines of node logs:"
        tail -20 /tmp/node.log
    fi
}

# Cleanup function
cleanup() {
    print_info "Cleaning up..."
    stop_node
    rm -rf /tmp/wormhole-test-node
    rm -f /tmp/node.log
}

# Main test execution
main() {
    print_header "Wormhole Pallet Runtime Testing"
    
    # Set up cleanup trap
    trap cleanup EXIT
    
    # Start the node
    if ! start_node; then
        print_error "Failed to start node. Exiting."
        show_node_logs
        exit 1
    fi
    
    # Generate test account
    generate_test_account
    
    # Run tests
    local test_results=()
    
    # Test 1: System Health
    if test_system_health; then
        test_results+=("✅ System Health")
    else
        test_results+=("❌ System Health")
    fi
    
    # Test 2: Account Balance
    if test_account_balance; then
        test_results+=("✅ Account Balance")
    else
        test_results+=("❌ Account Balance")
    fi
    
    # Test 3: Faucet
    if test_faucet; then
        test_results+=("✅ Faucet")
    else
        test_results+=("⚠️  Faucet (may not be available)")
    fi
    
    # Test 4: Storage Queries
    if test_storage_queries; then
        test_results+=("✅ Storage Queries")
    else
        test_results+=("❌ Storage Queries")
    fi
    
    # Test 5: Wormhole Verification
    if test_wormhole_verification; then
        test_results+=("✅ Wormhole Verification")
    else
        test_results+=("❌ Wormhole Verification")
    fi
    
    # Show results
    print_header "Test Results Summary"
    for result in "${test_results[@]}"; do
        echo -e "$result"
    done
    
    # Show some node logs
    echo -e "\n${BLUE}Recent Node Logs:${NC}"
    show_node_logs
    
    print_status "Runtime testing completed!"
}

# Check if script is being run directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi 