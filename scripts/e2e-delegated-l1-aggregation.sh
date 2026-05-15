#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHAIN_DIR="$ROOT/chain"
MINER_DIR="$ROOT/quantus-miner"
BINS_DIR="${BINS_DIR:-$ROOT/target/delegated-l1-fixture-bins}"
L0_PROOF="${L0_PROOF:-$CHAIN_DIR/pallets/miner-aggregation/test-data/l0_candidate_0.hex}"

echo "Generating delegated L1 fixtures with release-mode proving"
"$CHAIN_DIR/scripts/generate-delegated-l1-fixture.sh"

echo "Running chain delegated L1 settlement E2E fixture in release mode"
(
	cd "$CHAIN_DIR"
	QP_GENERATE_LAYER1=true QP_NUM_LAYER0_PROOFS=1 \
		cargo test --release -p pallet-miner-aggregation \
		submit_l1_aggregate_accepts_valid_fixture_and_settles_bundle -- --nocapture
)

echo "Running miner mocked claim/prove/submit worker flow"
(
	cd "$MINER_DIR"
	cargo test -p miner-service mocked_worker_processes_claimed_bundle_end_to_end -- --nocapture
)

echo "Running miner real L1 prover fixture in release mode"
(
	cd "$MINER_DIR"
	ZK_AGGREGATION_TEST_BINS_DIR="$BINS_DIR" \
		ZK_AGGREGATION_TEST_L0_PROOF="$L0_PROOF" \
		cargo test --release -p miner-service \
		zk_aggregation_prove_generates_l1_proof_from_fixture_when_configured -- --nocapture
)
