#!/bin/zsh

pkill -f "quantus-node"
sleep 1

rm -rf /tmp/validator1 /tmp/validator2

BINARY=./target/release/quantus-node

if [ ! -f "$BINARY" ]; then
  echo "Binary not found at $BINARY — building release..."
  cargo build --release -p quantus-node || exit 1
fi

# Node1
$BINARY \
  --base-path /tmp/validator1 \
  --dev \
  --port 30333 \
  --name Node1 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
  --validator \
  -lqpow=debug \
  2>&1 | sed 's/^/[Node1] /' &

sleep 3

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

# Node2
$BINARY \
  --base-path /tmp/validator2 \
  --dev \
  --port 30334 \
  --name Node2 \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9945,methods=unsafe,cors=all" \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/$NODE1_PEER_ID \
  --validator \
  -lqpow=debug \
  2>&1 | sed 's/^/[Node2] /' &

echo "Both nodes started. Ctrl+C to stop."
wait
