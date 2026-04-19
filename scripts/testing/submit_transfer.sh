#!/usr/bin/env bash
set -euo pipefail

# Submit a transfer from Alice to Bob on a running dev node.
# Requires: quantus-cli with test wallets created (quantus developer create-test-wallets)

QUANTUS_CLI="${QUANTUS_CLI:-quantus}"
NODE_URL="${NODE_URL:-ws://127.0.0.1:9944}"
AMOUNT="${1:-5}"

echo "Sending ${AMOUNT} UNIT from crystal_alice -> crystal_bob on ${NODE_URL}"
"$QUANTUS_CLI" send --from crystal_alice --to crystal_bob --amount "$AMOUNT" --node-url "$NODE_URL"
