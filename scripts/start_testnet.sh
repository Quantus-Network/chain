rm -rf /tmp/validator1

./target/release/quantus-node \
  --base-path /tmp/validator1 \
  --chain live_resonance \
  --port 30333 \
  --prometheus-port 9616 \
  --name ResonanceLiveTestnetNode \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
  --node-key cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15 \
  --validator
