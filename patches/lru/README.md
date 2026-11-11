# LRU Cache

Vendored from crates.io `lru` 0.12.5 (MIT), with the two-line iterator fix from
[upstream PR #224](https://github.com/jeromefroe/lru-rs/pull/224) backported for
[RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002.html).
Both mutable iterator directions borrow keys through shared pointers. The source
is formatted with the workspace formatter, with one Clippy allowance on an upstream
test helper. The public API and version are unchanged.

The root Cargo patch selects this copy for the optional `smoldot-light` dependency.
That dependency is not enabled in the shipped node or runtime, including release
v1.0.5. The unaffected `lru` 0.7.8 dependency used by tracing remains on crates.io.
Version-only scanners may still require a patched-code assessment for this backport.

Run `cargo test --locked -p lru@0.12.5` (also with `--no-default-features`).
The `iter_mut` integration test covers both iterator directions followed by cache
lookups; run it under Miri to detect the original pointer invalidation:
`cargo +nightly miri test -p lru@0.12.5 --test iter_mut`.
On macOS, set `DYLD_LIBRARY_PATH=''` to avoid the workspace's Homebrew LLVM override.

[![Build Badge]][build status]
[![Codecov Badge]][coverage status]
[![crates.io Badge]][crates.io package]
[![docs.rs Badge]][docs.rs documentation]
[![License Badge]][license]

[Documentation]

An implementation of a LRU cache. The cache supports `put`, `get`, `get_mut` and `pop` operations,
all of which are O(1). This crate was heavily influenced by the [LRU Cache implementation in an
earlier version of Rust's std::collections crate].

The MSRV for this crate is 1.64.0.

## Example

Below is a simple example of how to instantiate and use a LRU cache.

```rust,no_run
extern crate lru;

use lru::LruCache;
use std::num::NonZeroUsize;

fn main() {
    let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
    cache.put("apple", 3);
    cache.put("banana", 2);

    assert_eq!(*cache.get(&"apple").unwrap(), 3);
    assert_eq!(*cache.get(&"banana").unwrap(), 2);
    assert!(cache.get(&"pear").is_none());

    assert_eq!(cache.put("banana", 4), Some(2));
    assert_eq!(cache.put("pear", 5), None);

    assert_eq!(*cache.get(&"pear").unwrap(), 5);
    assert_eq!(*cache.get(&"banana").unwrap(), 4);
    assert!(cache.get(&"apple").is_none());

    {
        let v = cache.get_mut(&"banana").unwrap();
        *v = 6;
    }

    assert_eq!(*cache.get(&"banana").unwrap(), 6);
}
```

[build badge]: https://github.com/jeromefroe/lru-rs/actions/workflows/main.yml/badge.svg
[build status]: https://github.com/jeromefroe/lru-rs/actions/workflows/main.yml
[codecov badge]: https://codecov.io/gh/jeromefroe/lru-rs/branch/master/graph/badge.svg
[coverage status]: https://codecov.io/gh/jeromefroe/lru-rs
[crates.io badge]: https://img.shields.io/crates/v/lru.svg
[crates.io package]: https://crates.io/crates/lru/
[documentation]: https://docs.rs/lru/
[docs.rs badge]: https://docs.rs/lru/badge.svg
[docs.rs documentation]: https://docs.rs/lru/
[license badge]: https://img.shields.io/badge/license-MIT-blue.svg
[license]: https://raw.githubusercontent.com/jeromefroe/lru-rs/master/LICENSE
[lru cache implementation in an earlier version of rust's std::collections crate]: https://doc.rust-lang.org/0.12.0/std/collections/lru_cache/struct.LruCache.html
