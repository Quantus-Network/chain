#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
NODE="$TARGET_DIR/release/quantus-node"
RUNTIME="$TARGET_DIR/release/wbuild/quantus-runtime/quantus_runtime.wasm"
TEMPLATE="./.maintain/frame-weight-template.hbs"

# pallet_name:output_path:steps:repeat
#
# WARNING: three outputs carry hand-written layers that a raw regeneration wipes
# out — re-apply them by hand (see each file's header note):
#   - pallets/wormhole/src/weights.rs: refresh the four timing constants only
#     (pre-validate private/public, ZK verify private/public); keep the
#     depth-scaling storage tails and the test module.
#   - pallets/mining-rewards/src/weights.rs: refresh the measured base inside
#     `on_finalize_rewarded_miner`; keep the depth pricing and tests.
#   - pallets/reversible-transfers/src/weights.rs: refresh the base in
#     `execute_transfer_weight` and take fresh values for the other extrinsics;
#     keep the helper, both depth-aware `execute_transfer` impls and tests.
#   - pallets/scheduler/src/weights.rs: keep the V12 audit #162534 write charge
#     in `service_task_base` (both impls).
PALLETS=(
  "pallet-wormhole:pallets/wormhole/src/weights.rs:50:20"
  "pallet_multisig:pallets/multisig/src/weights.rs:20:50"
  "pallet_reversible_transfers:pallets/reversible-transfers/src/weights.rs:50:20"
  "pallet_scheduler:pallets/scheduler/src/weights.rs:50:20"
  "pallet_mining_rewards:pallets/mining-rewards/src/weights.rs:50:20"
  "pallet_treasury:pallets/treasury/src/weights.rs:50:20"
  "pallet_vesting:pallets/vesting/src/weights_generated.rs:50:20"
  "pallet_balances:pallets/balances/src/weights.rs:50:20"
  "frame_system:pallets/frame-system/src/weights.rs:50:20"
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
