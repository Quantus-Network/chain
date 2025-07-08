#!/bin/bash

# Peer Query Script for Quantus Network
# This script queries the peer sharing RPC endpoint to get network information

set -e

# Default values
DEFAULT_PORT=9944
DEFAULT_HOST=localhost

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to show usage
show_usage() {
    cat << EOF
Usage: $0 [OPTIONS] [HOSTS...]

Query peer information from Quantus nodes with peer sharing enabled.

OPTIONS:
    -p, --port PORT     RPC port (default: $DEFAULT_PORT)
    -h, --help          Show this help message
    -v, --verbose       Verbose output
    -j, --json          Output raw JSON

EXAMPLES:
    # Query localhost
    $0

    # Query specific host
    $0 192.168.1.100

    # Query multiple hosts
    $0 node1.example.com node2.example.com node3.example.com

    # Query with custom port
    $0 -p 9934 localhost

    # Get raw JSON output
    $0 -j localhost

    # Verbose output
    $0 -v node1.example.com node2.example.com

NOTES:
    - Nodes must be started with --enable-peer-sharing flag
    - RPC must be accessible (use --rpc-external for remote access)
    - Default RPC endpoint: http://localhost:9933
EOF
}

# Parse command line arguments
VERBOSE=false
JSON_OUTPUT=false
PORT=$DEFAULT_PORT
HOSTS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_usage
            exit 0
            ;;
        -p|--port)
            PORT="$2"
            shift 2
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -j|--json)
            JSON_OUTPUT=true
            shift
            ;;
        -*)
            print_error "Unknown option $1"
            echo
            show_usage
            exit 1
            ;;
        *)
            HOSTS+=("$1")
            shift
            ;;
    esac
done

# Default to localhost if no hosts specified
if [ ${#HOSTS[@]} -eq 0 ]; then
    HOSTS=("$DEFAULT_HOST")
fi

# Function to query a single node
query_node() {
    local host=$1
    local url="http://${host}:${PORT}"

    if [ "$VERBOSE" = true ]; then
        print_info "Querying $url"
    fi

    # Create JSON-RPC request
    local request='{
        "jsonrpc": "2.0",
        "method": "peer_getBasicInfo",
        "params": [],
        "id": 1
    }'

    # Make the request
    local response
    if ! response=$(curl -s -H "Content-Type: application/json" -d "$request" "$url" 2>/dev/null); then
        print_error "Failed to connect to $host:$PORT"
        return 1
    fi

    # Check if response contains error
    if echo "$response" | grep -q '"error"'; then
        if [ "$VERBOSE" = true ]; then
            print_error "RPC error from $host: $response"
        else
            print_error "RPC error from $host (peer sharing may not be enabled)"
        fi
        return 1
    fi

    # Extract result
    local result
    if ! result=$(echo "$response" | jq -r '.result' 2>/dev/null); then
        print_error "Invalid JSON response from $host"
        if [ "$VERBOSE" = true ]; then
            echo "Response: $response"
        fi
        return 1
    fi

    if [ "$result" = "null" ]; then
        print_error "No result from $host"
        return 1
    fi

    # Output based on format preference
    if [ "$JSON_OUTPUT" = true ]; then
        echo "$result"
    else
        local peer_id=$(echo "$result" | jq -r '.peer_id')
        local peer_count=$(echo "$result" | jq -r '.peer_count')
        local external_addresses=$(echo "$result" | jq -r '.external_addresses[]' 2>/dev/null)
        local listen_addresses=$(echo "$result" | jq -r '.listen_addresses[]' 2>/dev/null)

        print_success "$host - Peer ID: ${peer_id:0:12}... - Connected Peers: $peer_count"

        # Show external addresses if any
        if [ -n "$external_addresses" ]; then
            echo "  External Addresses:"
            echo "$external_addresses" | while read -r addr; do
                if [ -n "$addr" ]; then
                    echo "    - $addr"
                fi
            done
        fi

        # Show listen addresses if any
        if [ -n "$listen_addresses" ]; then
            echo "  Listen Addresses:"
            echo "$listen_addresses" | while read -r addr; do
                if [ -n "$addr" ]; then
                    echo "    - $addr"
                fi
            done
        fi

        # Show connected peers if any
        local total_connected=$(echo "$result" | jq -r '.connected_peers | length' 2>/dev/null)
        if [ "$total_connected" -gt 0 ]; then
            if [ "$VERBOSE" = true ]; then
                echo "  Connected Peers (all $total_connected):"
                echo "$result" | jq -r '.connected_peers[]' 2>/dev/null | while read -r peer; do
                    if [ -n "$peer" ]; then
                        echo "    - ${peer:0:12}..."
                    fi
                done
            else
                echo "  Connected Peers (showing first 5 of $total_connected):"
                echo "$result" | jq -r '.connected_peers[]' 2>/dev/null | head -5 | while read -r peer; do
                    if [ -n "$peer" ]; then
                        echo "    - ${peer:0:12}..."
                    fi
                done

                if [ "$total_connected" -gt 5 ]; then
                    echo "    ... and $((total_connected - 5)) more (use -v to see all)"
                fi
            fi
        fi

        echo  # Add blank line for readability
    fi

    return 0
}

# Function to check dependencies
check_dependencies() {
    local missing_deps=()

    if ! command -v curl &> /dev/null; then
        missing_deps+=("curl")
    fi

    if ! command -v jq &> /dev/null; then
        missing_deps+=("jq")
    fi

    if [ ${#missing_deps[@]} -ne 0 ]; then
        print_error "Missing required dependencies: ${missing_deps[*]}"
        echo
        echo "Please install the missing dependencies:"
        echo "  Ubuntu/Debian: sudo apt-get install ${missing_deps[*]}"
        echo "  macOS: brew install ${missing_deps[*]}"
        echo "  CentOS/RHEL: sudo yum install ${missing_deps[*]}"
        exit 1
    fi
}

# Main execution
main() {
    # Check for required tools
    check_dependencies

    if [ "$JSON_OUTPUT" = false ]; then
        print_info "Querying peer information from ${#HOSTS[@]} node(s)"
        echo
    fi

    local success_count=0
    local total_peers=0

    # Query each host
    for host in "${HOSTS[@]}"; do
        if query_node "$host"; then
            ((success_count++))
            if [ "$JSON_OUTPUT" = false ]; then
                # Extract peer count for summary
                local temp_response
                temp_response=$(curl -s -H "Content-Type: application/json" -d '{
                    "jsonrpc": "2.0",
                    "method": "peer_getBasicInfo",
                    "params": [],
                    "id": 1
                }' "http://${host}:${PORT}" 2>/dev/null)

                if [ $? -eq 0 ]; then
                    local peer_count
                    peer_count=$(echo "$temp_response" | jq -r '.result.peer_count' 2>/dev/null)
                    if [[ "$peer_count" =~ ^[0-9]+$ ]]; then
                        ((total_peers += peer_count))
                    fi
                fi
            fi
        fi
    done

    # Summary
    if [ "$JSON_OUTPUT" = false ] && [ ${#HOSTS[@]} -gt 1 ]; then
        echo
        if [ $success_count -eq ${#HOSTS[@]} ]; then
            print_success "Successfully queried all $success_count nodes"
        else
            print_warning "Successfully queried $success_count out of ${#HOSTS[@]} nodes"
        fi

        if [ $success_count -gt 0 ] && [ $total_peers -gt 0 ]; then
            local avg_peers=$((total_peers / success_count))
            print_info "Average peers per node: $avg_peers"
        fi
    fi

    # Exit with appropriate code
    if [ $success_count -eq 0 ]; then
        exit 1
    fi
}

# Run main function
main "$@"
