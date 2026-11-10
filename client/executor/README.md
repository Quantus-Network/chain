# sc-executor (vendored)

Vendored from crates.io `sc-executor` **0.47.0**, then patched to drop the
experimental PolkaVM executor (`sc-executor-polkavm`). Quantus runs WebAssembly
runtimes only, so the PolkaVM dispatch in `create_wasm_runtime_with_code` is
removed along with the dependency; see `../executor-common` for the matching
`RuntimeBlob` change.

Upstream unit tests that depend on the unpublished `sc-runtime-test` /
`substrate-test-runtime` crates were dropped or rewritten against small WAT
modules. The Criterion bench (and its kusama runtime blob) is not vendored.

---

A crate that provides means of executing/dispatching calls into the runtime.

There are a few responsibilities of this crate at the moment:

- It provides an implementation of a common entrypoint for calling into the runtime, both
wasm and compiled.
- It defines the environment for the wasm execution, namely the host functions that are to be
provided into the wasm runtime module.
- It also provides the required infrastructure for executing the current wasm runtime (specified
by the current value of `:code` in the provided externalities), i.e. interfacing with
wasm engine used, instance cache.

License: GPL-3.0-or-later WITH Classpath-exception-2.0
