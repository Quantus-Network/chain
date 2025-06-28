cargo fix --all

taplo format --check --config taplo.toml

cargo fmt --all -- --check

SKIP_WASM_BUILD=1 cargo clippy --locked --workspace

cargo build --locked --workspace --features runtime-benchmarks

SKIP_WASM_BUILD=1 cargo test --locked --workspace


SKIP_WASM_BUILD=1 cargo doc --locked --workspace --no-deps