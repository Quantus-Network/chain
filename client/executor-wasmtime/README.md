# sc-executor-wasmtime (vendored)

Vendored from crates.io `sc-executor-wasmtime` **0.43.0**, then patched to use
`wasmtime` **36.0.13** (was 35.0.0) so Apr 2026 RustSec advisories clear without
a full Substrate client bump.

Upstream bump reference: [paritytech/polkadot-sdk#11793](https://github.com/paritytech/polkadot-sdk/pull/11793).

Upstream unit tests that require `sc-runtime-test` are omitted; the node
exercises this crate through `sc-executor`.

License: GPL-3.0-or-later WITH Classpath-exception-2.0
