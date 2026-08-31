"$(dirname "$0")/scripts/fmt.sh"
taplo format
SKIP_WASM_BUILD=1 cargo clippy --locked --workspace
