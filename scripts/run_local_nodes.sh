#!/bin/zsh

# Kill any previous node processes
pkill -f "quantus-node"

# Clean up old chain data
rm -rf /tmp/validator1 /tmp/validator2 /tmp/listener

# Calculate expected Peer ID from the node key
# -----------------------------
# 1) Start Node1 (Alice)
#    WebSocket on 127.0.0.1:9944
# -----------------------------
./target/release/quantus-node \
  --base-path /tmp/validator1 \
  --dev \
  --port 30333 \
  --prometheus-port 9616 \
  --name Node1 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
  --node-key cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15 \
  --validator \
  &

# Wait for Node1 to come online
sleep 5

# Retrieve its peer ID via HTTP on the same endpoint
NODE1_PEER_ID=$(
  curl -s http://127.0.0.1:9944 \
    -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"system_localPeerId","id":1}' \
  | jq -r '.result'
)

if [ -z "$NODE1_PEER_ID" ] || [ "$NODE1_PEER_ID" = "null" ]; then
  echo "Failed to retrieve Node1 Peer ID"
  exit 1
fi

echo "Node1 Peer ID: $NODE1_PEER_ID"

# -----------------------------
# 2) Start Node2 (Bob)
#    WebSocket on 127.0.0.1:9945
# -----------------------------
./target/release/quantus-node \
  --base-path /tmp/validator2 \
  --dev \
  --port 30334 \
  --prometheus-port 9617 \
  --name Node2 \
  --node-key bbb5338fe3dbe14aacde7465aac6606ce22a9630ad63978030224764d6fb2c51 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9945,methods=unsafe,cors=all" \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/$NODE1_PEER_ID \
  --validator \
  &

# -----------------------------
# 3) Start Listener (non-mining node)
#    WebSocket on 127.0.0.1:9946
# -----------------------------
./target/release/quantus-node \
  --base-path /tmp/listener \
  --dev \
  --port 30335 \
  --prometheus-port 9618 \
  --name Listener \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9946,methods=unsafe,cors=all" \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/$NODE1_PEER_ID \
  &