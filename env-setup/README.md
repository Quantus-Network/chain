# Env setup

[`flake.nix`](./flake.nix) is a Nix dev shell (`rustup`, `clang`, `protobuf`, and `LIBCLANG_PATH`). Enter it from this directory:

```sh
nix develop
```

Then run cargo from the repository root. The compiler pin cargo uses there is [`rust-toolchain`](../rust-toolchain). Nightly `rustfmt` is pinned in [`rustfmt-toolchain`](../rustfmt-toolchain).

[`rust-toolchain.toml`](./rust-toolchain.toml) in this directory applies only when the current directory is `env-setup/` (or a subdirectory). It is a leftover Substrate channel pin (`stable` plus `wasm32-unknown-unknown`).
