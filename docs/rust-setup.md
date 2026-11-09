# Rust and host setup

Builds use the toolchain file at the repository root. Host packages below are the same ones CI installs in [`.github/actions`](../.github/actions).

## Toolchain

[`rust-toolchain`](../rust-toolchain) pins:

| Field | Value |
| --- | --- |
| Channel | `1.93.0` (stable) |
| Components | `clippy` |
| Targets | `wasm32v1-none` |
| Profile | `minimal` |

With [rustup](https://rustup.rs) installed, `cargo` and `rustc` read that file when the working directory is inside this repo and install the pin if it is missing. Build and `cargo doc` pick up the pin on their own. `scripts/fmt.sh` is the entry point for formatting.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cd /path/to/chain
rustup show
```

`rustup show` should report `1.93.0` as the active toolchain, with `wasm32v1-none` installed.

Runtime builds use `wasm32v1-none`, the target named in `rust-toolchain`. CI also installs `wasm32-unknown-unknown` and the `rust-src` component before clippy and `cargo doc`.

## rustfmt

[`.rustfmt.toml`](../.rustfmt.toml) uses options that only nightly `rustfmt` accepts. That nightly is pinned separately in [`rustfmt-toolchain`](../rustfmt-toolchain) so format output stays stable. [`scripts/fmt.sh`](../scripts/fmt.sh) installs it (`--profile minimal --component rustfmt`) and forwards arguments to `cargo fmt`.

```bash
scripts/fmt.sh --all -- --check
```

To bump the formatter, change the date in `rustfmt-toolchain`, run `scripts/fmt.sh --all`, and commit the reformat together with the pin.

TOML is formatted with taplo:

```bash
taplo format --config taplo.toml
```

## Host packages

`bindgen` (RocksDB and related crates) needs libclang. `litep2p` compiles protobuf schemas at build time.

### Ubuntu/Debian

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends clang libclang-dev protobuf-compiler
```

### macOS

```bash
brew install protobuf llvm cmake openssl
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
```

Put `LIBCLANG_PATH` in the shell profile you use for builds. Apple Silicon and Intel Homebrew prefixes differ; `brew --prefix llvm` covers both.

### Windows

Native Windows builds are what the Windows CI job does. From a shell that can see `choco` and `rustup`:

```bash
choco install protoc --yes
choco install llvm --yes
```

Set `LIBCLANG_PATH` to `C:\Program Files\LLVM\bin`.

### Nix

[`env-setup/flake.nix`](../env-setup/flake.nix) is a dev shell with `rustup`, `clang`, `protobuf`, and `LIBCLANG_PATH` already set. From `env-setup/`:

```bash
nix develop
```

Then build from the repository root so rustup still honors `rust-toolchain`.

## Build

```bash
cargo build --release
```

The node binary is `target/release/quantus-node`. See the [README](../README.md) for running it.
