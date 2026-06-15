cargo +nightly fmt
taplo format
SKIP_WASM_BUILD=1 cargo clippy --locked --workspace
