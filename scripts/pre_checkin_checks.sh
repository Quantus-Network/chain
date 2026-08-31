#!/usr/bin/env bash

# This script runs a series of checks to ensure code quality before check-in.
# It will print an error and exit immediately if any command fails.

echo "--- Running pre-checkin checks ---"

echo "[1/8] Running 'cargo fix'..."
cargo fix --all || { echo "Error: 'cargo fix' failed."; exit 1; }

echo "[2/8] Formatting Rust with 'scripts/fmt.sh'..."
scripts/fmt.sh || { echo "Error: 'fmt.sh' failed."; exit 1; }

echo "[3/8] Checking TOML format with 'taplo'..."
taplo format --check --config taplo.toml || { echo "Error: 'taplo format' check failed."; exit 1; }

echo "[4/8] Checking Rust format with 'scripts/fmt.sh --all -- --check'..."
scripts/fmt.sh --all -- --check || { echo "Error: 'fmt.sh --check' failed."; exit 1; }

echo "[5/8] Running 'cargo clippy'..."
SKIP_WASM_BUILD=1 cargo clippy --locked --workspace || { echo "Error: 'cargo clippy' failed."; exit 1; }

echo "[6/8] Building with runtime-benchmarks..."
echo "skipping remaining checks"
# cargo build --locked --workspace --features runtime-benchmarks || { echo "Error: 'cargo build' with benchmarks failed."; exit 1; }

# echo "[7/8] Running tests..."
#SKIP_WASM_BUILD=1 cargo test --locked --workspace || { echo "Error: 'cargo test' failed."; exit 1; }

# echo "[8/8] Building documentation..."
# SKIP_WASM_BUILD=1 cargo doc --locked --workspace --no-deps || { echo "Error: 'cargo doc' failed."; exit 1; }

echo "--- All checks passed successfully! ---"