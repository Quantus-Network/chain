#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

NODE="./target/release/quantus-node"
RUNTIME="./target/release/wbuild/quantus-runtime/quantus_runtime.wasm"
TEMPLATE="./.maintain/frame-weight-template.hbs"

# pallet_name:output_path:steps:repeat
PALLETS=(
  "pallet-wormhole:pallets/wormhole/src/weights.rs:50:20"
  "pallet_multisig:pallets/multisig/src/weights.rs:20:50"
  "pallet_reversible_transfers:pallets/reversible-transfers/src/weights.rs:50:20"
  "pallet_scheduler:pallets/scheduler/src/weights.rs:50:20"
  "pallet_mining_rewards:pallets/mining-rewards/src/weights.rs:50:20"
  "pallet_treasury:pallets/treasury/src/weights.rs:50:20"
)

COMMON_ARGS=(
  --runtime="$RUNTIME"
  --genesis-builder=runtime
  --extrinsic='*'
  --wasm-execution=compiled
  --heap-pages=4096
  --template="$TEMPLATE"
)

for entry in "${PALLETS[@]}"; do
  IFS=':' read -r pallet output steps repeat <<< "$entry"
  echo "=== Benchmarking $pallet -> $output ==="
  "$NODE" benchmark pallet \
    --pallet="$pallet" \
    --steps="$steps" \
    --repeat="$repeat" \
    "${COMMON_ARGS[@]}" \
    --output="./$output"
done

cargo +nightly fmt
echo "Done."
