# sc-executor-common (vendored)

Vendored from crates.io `sc-executor-common` **0.43.0**, then patched to drop
PolkaVM support: `RuntimeBlob` only holds a WebAssembly module, a `PVM\0` blob
is rejected at `RuntimeBlob::new`, and the `polkavm` dependency is gone. This
removes the `polkavm*` crates from the node's dependency graph entirely
(together with the vendored `../executor`).

---

A set of common definitions that are needed for defining execution engines.

License: GPL-3.0-or-later WITH Classpath-exception-2.0
