#!/bin/bash

# Start a Quantus node in testnet mode
#
# USAGE:
#   ./start_testnet.sh
#
# This script will start a Quantus node in testnet mode (Dirac Testnet)
#

rm -rf /tmp/validator1

./target/release/quantus-node \
  --base-path /tmp/validator1 \
  --chain dirac \
  --port 30333 \
  --prometheus-port 9616 \
  --name DiracTestnetNode \
  --experimental-rpc-endpoint "listen-addr=127.0.0.1:9944,methods=unsafe,cors=all" \
  --node-key cffac33ca656d18f3ae94393d01fe03d6f9e8bf04106870f489acc028b214b15 \
  --validator
