#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/chain/pallets/miner-aggregation/test-data}"
BINS_DIR="${BINS_DIR:-$ROOT/target/delegated-l1-fixture-bins}"
L0_PROOF="${L0_PROOF:-$ROOT/chain/pallets/wormhole/test-data/aggregated.hex}"
QP_NUM_LEAF_PROOFS="${QP_NUM_LEAF_PROOFS:-16}"
QP_NUM_LAYER0_PROOFS="${QP_NUM_LAYER0_PROOFS:-1}"
AGGREGATOR_ADDRESS="${AGGREGATOR_ADDRESS:-0200000000000000000000000000000000000000000000000000000000000000}"

cd "$ROOT/qp-zk-circuits"

cargo run --release -p qp-wormhole-circuit-builder -- \
	--output "$BINS_DIR" \
	--num-leaf-proofs "$QP_NUM_LEAF_PROOFS" \
	--num-layer0-proofs "$QP_NUM_LAYER0_PROOFS"

cargo run --release -p qp-wormhole-aggregator --example generate_l1_fixture -- \
	--bins-dir "$BINS_DIR" \
	--out "$OUT" \
	--aggregator-address "$AGGREGATOR_ADDRESS" \
	--l0-proof "$L0_PROOF"
