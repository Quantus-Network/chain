#!/bin/zsh

# Usage: ./run_local_nodes.sh [num_nodes]
# Default: 2 nodes

NUM_NODES=${1:-2}

if [ "$NUM_NODES" -lt 1 ]; then
  echo "Error: Must run at least 1 node"
  exit 1
fi

echo "Starting $NUM_NODES local nodes..."

pkill -f "quantus-node"
sleep 1

# Clean up old data
for i in $(seq 1 $NUM_NODES); do
  rm -rf /tmp/validator$i
done

BINARY=./target/release/quantus-node

if [ ! -f "$BINARY" ]; then
  echo "Binary not found at $BINARY — building release..."
  cargo build --release -p quantus-node || exit 1
fi

# Base ports
BASE_P2P_PORT=30333
BASE_RPC_PORT=9944

# Start Node1 (bootstrap node)
echo "Starting Node1..."
$BINARY \
  --base-path /tmp/validator1 \
  --dev \
  --port $BASE_P2P_PORT \
  --name Node1 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:$BASE_RPC_PORT,methods=unsafe,cors=all" \
  --validator \
  -lqpow=debug \
  2>&1 | sed 's/^/[Node1] /' &

sleep 3

# Get Node1's peer ID for other nodes to connect to
NODE1_PEER_ID=$(
  curl -s http://127.0.0.1:$BASE_RPC_PORT \
    -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"system_localPeerId","id":1}' \
  | jq -r '.result'
)

if [ -z "$NODE1_PEER_ID" ] || [ "$NODE1_PEER_ID" = "null" ]; then
  echo "Failed to retrieve Node1 Peer ID"
  exit 1
fi

echo "Node1 Peer ID: $NODE1_PEER_ID"

# Start remaining nodes
for i in $(seq 2 $NUM_NODES); do
  P2P_PORT=$((BASE_P2P_PORT + i - 1))
  RPC_PORT=$((BASE_RPC_PORT + i - 1))
  
  echo "Starting Node$i (P2P: $P2P_PORT, RPC: $RPC_PORT)..."
  
  $BINARY \
    --base-path /tmp/validator$i \
    --dev \
    --port $P2P_PORT \
    --name Node$i \
    --experimental-rpc-endpoint "listen-addr=127.0.0.1:$RPC_PORT,methods=unsafe,cors=all" \
    --bootnodes /ip4/127.0.0.1/tcp/$BASE_P2P_PORT/p2p/$NODE1_PEER_ID \
    --validator \
    -lqpow=debug \
    2>&1 | sed "s/^/[Node$i] /" &
  
  sleep 1
done

echo ""
echo "Started $NUM_NODES nodes. Ctrl+C to stop."
echo "RPC endpoints: $(for i in $(seq 1 $NUM_NODES); do echo -n "127.0.0.1:$((BASE_RPC_PORT + i - 1)) "; done)"
echo ""
wait
