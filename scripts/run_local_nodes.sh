#!/bin/zsh

# Set node binary name
NODE_BIN="./target/release/resonance-node"

# Kill any previous node processes
pkill -f "resonance-node"

# Clean up old chain data
rm -rf /tmp/validator1 /tmp/validator2 /tmp/listener

# Calculate expected Peer ID from the node key
NODE_KEY_HEX="cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15"
EXPECTED_PEER_ID=$(subkey inspect-node-key --file <(echo $NODE_KEY_HEX))

# -----------------------------
# 1) Start Node1 (Alice - Bootnode)
#    WebSocket on 127.0.0.1:9944
# -----------------------------
$NODE_BIN \
  --base-path /tmp/validator1 \
  --chain local \
  --port 30333 \
  --name Node1 \
  --public-addr /ip4/127.0.0.1/tcp/30333 \
  --node-key cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15 \
  --prometheus-port 9621 \
  --validator \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
  &

# Wait for Node1 to come online
sleep 5

# Retrieve Peer ID of Node1
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
$NODE_BIN \
  --base-path /tmp/validator2 \
  --chain local \
  --port 30334 \
  --name Node2 \
  --public-addr /ip4/127.0.0.1/tcp/30334 \
  --prometheus-port 9622 \
  --node-key bbb5338fe3dbe14aacde7465aac6606ce22a9630ad63978030224764d6fb2c51 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9945,methods=unsafe,cors=all" \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/$NODE1_PEER_ID \
  --validator \
  &

# -----------------------------
# 3) Start Listener (Non-Mining Node)
#    WebSocket on 127.0.0.1:9946
# -----------------------------
$NODE_BIN \
  --base-path /tmp/listener \
  --chain local \
  --port 30335 \
  --name Listener \
  --public-addr /ip4/127.0.0.1/tcp/30335 \
  --prometheus-port 9623 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9946,methods=unsafe,cors=all" \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/$NODE1_PEER_ID \
  &