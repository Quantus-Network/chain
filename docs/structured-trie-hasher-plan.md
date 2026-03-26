# Structured Trie Hasher

## Problem

The ZK-trie node codec produces **felt-aligned** (8-byte aligned) encoded nodes, but the
`hash_db::Hasher` trait has a single method — `fn hash(x: &[u8]) -> Self::Out` — that receives
opaque bytes with no context about what they represent. `PoseidonHasher` must therefore apply
the expensive **injective encoder** to all inputs, including trie nodes that are already
felt-aligned by construction.

This means the ZK circuit proving trie membership must include constraints for the injective
byte-to-felt conversion on every node hash — even though the node bytes are already structured
as a sequence of 8-byte felt-sized chunks.

## Goal

Extend the `Hasher` trait so the hasher can distinguish between **encoded trie nodes**
and **raw storage values**, enabling `PoseidonHasher` to:

- **Skip the injective encoder for nodes** — directly interpret 8-byte chunks as Goldilocks felts
- **Apply the injective encoder only for values** — arbitrary-length user data that isn't felt-aligned

This eliminates encoding constraints from every trie node hash in the ZK circuit.

## Trait Design

### Extended `Hasher` trait in `hash-db`

```rust
pub trait Hasher: Sync + Send {
    type Out: AsRef<[u8]> + AsMut<[u8]> + Default + ...;
    type StdHasher: Sync + Send + Default + hash::Hasher;
    const LENGTH: usize;

    fn hash(x: &[u8]) -> Self::Out;

    fn hash_node(encoded_node: &[u8]) -> Self::Out {
        Self::hash(encoded_node)
    }

    fn hash_value(value: &[u8]) -> Self::Out {
        Self::hash(value)
    }
}
```

**Why this shape:**

- **Methods on `Hasher` itself, not a separate trait** — `hash_node` and `hash_value` are added
  directly to the existing `Hasher` trait with default implementations that delegate to `hash`.
  This means every existing `Hasher` impl (e.g. `Blake2Hasher`) works without changes. Only
  `PoseidonHasher` overrides them.
- **No separate `TrieHasher` trait** — an earlier iteration introduced a `TrieHasher: Hasher`
  supertrait, but this added complexity (orphan rule issues, extra imports, redundant bounds)
  with no benefit. Since the defaults delegate to `hash`, putting the methods directly on
  `Hasher` is strictly simpler.
- **Two methods, not per-node-type methods** — the earlier idea of `hash_leaf(partial, value)` /
  `hash_branch(partial, children, value)` would require threading individual fields through every
  encoding path. Instead, `hash_node` receives the already-encoded bytes. Since we control both
  the codec and the hasher, the hasher can re-interpret the felt-aligned structure directly.
- No `hash_key` method — `FatDB`/`SecTrieDB` (which hash user keys) are unused in this codebase.

### Extended `HashDB` trait

```rust
pub trait HashDB<H: Hasher, T>: Send + Sync + AsHashDB<H, T> {
    fn get(&self, key: &H::Out, prefix: Prefix) -> Option<T>;
    fn contains(&self, key: &H::Out, prefix: Prefix) -> bool;
    fn insert(&mut self, prefix: Prefix, value: &[u8]) -> H::Out;
    fn emplace(&mut self, key: H::Out, prefix: Prefix, value: T);
    fn remove(&mut self, key: &H::Out, prefix: Prefix);

    fn insert_node(&mut self, prefix: Prefix, encoded_node: &[u8]) -> H::Out {
        self.insert(prefix, encoded_node)
    }
}
```

The default `insert_node` delegates to `insert`, so all existing `HashDB` impls compile.
Only `memory-db` overrides it to route through `hash_node`.

### `MAX_INLINE_NODE` on `TrieLayout`

```rust
pub trait TrieLayout {
    const MAX_INLINE_VALUE: Option<u32>;
    const MAX_INLINE_NODE: Option<u32> = None;
    // ...
}
```

Controls the child inlining threshold. `None` preserves the upstream default (`Hash::LENGTH`).
`Some(0)` forces all children to be hashed, which is required by our codec (it panics on
inline children). Both `LayoutV0` and `LayoutV1` set this to `Some(0)`.

## Call Site Inventory

Every `H::hash(&[u8])` call in the trie stack falls into exactly 3 categories:

| Category | Description | Action |
|----------|-------------|--------|
| **Encoded trie node** | Full encoded node bytes (leaf, branch, empty, root) | → `hash_node` / `insert_node` |
| **Storage value** | Raw value bytes exceeding inline threshold | → `hash_value` / `insert` |
| **Sentinel** | Null key / empty marker (fixed small input) | → `Hasher::hash` (unchanged) |

## Changes Per Crate

### 1. `hash-db` (local fork)

**Files:** `src/lib.rs`

| Change | Detail |
|--------|--------|
| Add `hash_node` / `hash_value` to `Hasher` | Default impls delegate to `hash` |
| Add `insert_node` default method on `HashDB` | Delegates to `insert` |

### 2. `memory-db` (local fork)

**Files:** `src/lib.rs`

| Change | Detail |
|--------|--------|
| `HashDB::insert` impl | `H::hash(value)` → `H::hash_value(value)` |
| `HashDB::insert_node` override | Calls `H::hash_node(encoded_node)` then `emplace` |
| `from_null_node` | Unchanged — sentinel, uses base `Hasher::hash` |

### 3. `trie-root` (local fork)

**Files:** `src/lib.rs`

| Current | Change to | Reason |
|---------|-----------|--------|
| `H::hash(value)` | `H::hash_value(value)` | Value that exceeded threshold |
| `H::hash(&stream.out())` | `H::hash_node(&stream.out())` | Root node encoding |

### 4. `trie-db` (local, at `primitives/trie-db`)

**`iter_build.rs`** — 4 `ProcessEncodedNode` impls updated:
- `self.db.insert` → `self.db.insert_node` for encoded nodes
- `Hasher::hash(node)` → `Hasher::hash_node(node)` for in-memory root calculation
- `Hasher::hash(value)` → `Hasher::hash_value(value)` for inner hashed values
- Inline threshold checks use `T::MAX_INLINE_NODE` instead of hardcoded `Hash::LENGTH`

**`triedbmut.rs`** — 2 insert sites:
- Root node and child node insertions → `insert_node`
- Inline threshold uses `L::MAX_INLINE_NODE`

**`trie_codec.rs`** — 1 site: `db.insert` → `db.insert_node` for encoded nodes

**`proof/verify.rs`** — 2 sites: `hash(value)` → `hash_value`, `hash(node)` → `hash_node`

**`node.rs`** — 1 site: inline value → `hash_value`

**`lib.rs`** — Added `MAX_INLINE_NODE` constant to `TrieLayout`

### 5. `sp-trie` (local, at `primitives/trie`)

| File | Change |
|------|--------|
| `lib.rs` | `LayoutV0`/`LayoutV1` set `MAX_INLINE_NODE: Some(0)` |
| `node_codec.rs` | `hash` → `hash_node`; handle `Inline(_, 0)` sentinel for compact proofs |
| `recorder.rs` | `hash` → `hash_node` for proof nodes |
| `trie_stream.rs` | `hash` → `hash_node` |

### 6. `sp-state-machine` (local, at `primitives/state-machine`)

| File | Change |
|------|--------|
| `ext.rs` | `storage_hash` / `child_storage_hash` → `hash_value` |
| All files | Mechanical — no bound changes needed since `hash_node`/`hash_value` are on `Hasher` |

### 7. `PoseidonHasher` (companion PR in `qp-poseidon`)

```rust
impl Hasher for PoseidonHasher {
    // ... existing type/const defs ...

    fn hash(x: &[u8]) -> H256 {
        H256::from_slice(&Self::hash_for_circuit(x))
    }

    fn hash_node(encoded_node: &[u8]) -> H256 {
        let felts: Vec<Goldilocks> = encoded_node
            .chunks(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf[..chunk.len()].copy_from_slice(chunk);
                Goldilocks::from_u64(u64::from_le_bytes(buf))
            })
            .collect();
        H256::from_slice(&hash_to_bytes(&felts))
    }

    fn hash_value(value: &[u8]) -> H256 {
        H256::from_slice(&Self::hash_for_circuit(value))
    }
}
```

This is where the constraint savings come from. `hash_node` does zero encoding work —
it treats each 8-byte chunk as a native field element. The ZK circuit for node hashing
becomes: load felts directly from witness → feed into Poseidon2 sponge. No range checks,
no length separators, no byte packing logic.

`hash_value` delegates to `hash_for_circuit` (injective encoding) since storage values
are arbitrary bytes.

### 8. `Blake2Hasher`

No changes needed. The default `hash_node`/`hash_value` implementations on `Hasher`
delegate to `Blake2Hasher::hash`, so all tests using `Blake2Hasher` work without any
additional code.

## What Does NOT Change

| Component | Why unchanged |
|-----------|---------------|
| `frame_system::Config::Hashing` | Pallets use `T::Hashing::hash()` (base `Hasher::hash`) |
| `qp-header` | Already has its own bespoke felt-aligned hashing via `Header::hash(&self)` |
| Wormhole pallet | Uses `PoseidonCore::hash_storage`, independent of trie hasher |
| PoW / mining | Uses `hash_squeeze_twice`, unrelated |
| Block import / consensus | Uses header hashing via `qp-header` |
| Host functions (`sp_io`) | Upstream, calls into state machine which carries the generic `H: Hasher` |

## Risk Assessment

### Consensus-breaking change

This changes trie node hashes and therefore every state root. **Requires genesis reset.**
Pre-mainnet, genesis reset is acceptable.

### Correctness of `hash_node`

The direct 8-byte-to-felt mapping only works because the ZK-trie codec guarantees 8-byte
alignment on all node encodings. Existing tests provide coverage:

- `random_test_8_byte_alignment` — random trie alignment over 20 seeds
- `storage_proof_8_byte_alignment_test` — random data + edge cases + non-inclusion proofs
- `child_reference_8_byte_boundary_test` — branch node child positioning

### Proof verification

ZK proofs of trie membership need the verifier to agree on the hashing scheme. The verifier
circuit uses the same direct felt-loading (which is exactly what makes this faster), so
both sides benefit.

## Circuit Constraint Savings

**Current path** (`PoseidonHasher::hash` with injective encoder):
- 64-byte node → injective encoding → ~10–12 felts + length overhead
- Circuit: range checks per felt + length separator constraints + Poseidon sponge

**New path** (`PoseidonHasher::hash_node` with direct loading):
- 64-byte node → 8 felts (direct 8-byte chunks)
- Circuit: Poseidon sponge only

For every trie node hash verified in a block proof (typically 10–20+ nodes per storage
access, multiplied by all storage accesses in the block), the injective encoding constraints
are eliminated entirely. The savings compound across the entire block proof.

---

## Addendum: Deviations from the Original Plan

The original plan (preserved below as reference) proposed a specific architecture that was
refined during implementation. Here are the changes and why they were made.

### 1. No separate `TrieHasher` trait

**Plan said:** Add a `pub trait TrieHasher: Hasher` with `hash_node` and `hash_value` methods.
Change all `H: Hasher` bounds across the stack to `H: TrieHasher`.

**What was implemented:** `hash_node` and `hash_value` were added directly to the `Hasher`
trait as default methods. No `TrieHasher` trait exists.

**Why:** The separate trait caused cascading problems:
- **Orphan rules:** `Blake2Hasher` is defined in `sp-core`, so implementing `TrieHasher`
  (defined in `hash-db`) for it required either forking `sp-core` or placing the impl in
  `sp-trie`, which triggered Rust's orphan rule (E0117).
- **Cyclic dependencies:** Attempting to add `sp-core` as an optional dep of `hash-db`
  (to impl `TrieHasher` for `Blake2Hasher` there) created a dependency cycle since `sp-core`
  depends on `hash-db`.
- **Unnecessary complexity:** Since the default implementations just delegate to `hash`,
  putting the methods on `Hasher` directly means every existing `Hasher` impl automatically
  gets correct behavior. Only `PoseidonHasher` overrides them. No bound changes needed anywhere.

This eliminated ~100 lines of bound changes across `sp-state-machine` and simplified the
entire diff.

### 2. No `Blake2Hasher` implementation needed

**Plan said:** Implement `TrieHasher for Blake2Hasher` with trivial delegation to `hash`.

**What was implemented:** Nothing. The defaults on `Hasher` handle this automatically.

**Why:** With methods directly on `Hasher`, the default `hash_node`/`hash_value` delegate
to `Self::hash`. `Blake2Hasher` gets this for free.

### 3. `PoseidonHasher` overrides are on `impl Hasher`, not a separate impl block

**Plan said:** `impl TrieHasher for PoseidonHasher { ... }` as a separate impl block.

**What was implemented:** `hash_node` and `hash_value` are overridden directly inside
`impl Hasher for PoseidonHasher { ... }`.

**Why:** There is no `TrieHasher` trait. The methods live on `Hasher`, so the overrides
go in the `Hasher` impl. This is also cleaner — one impl block per type.

### 4. Added `MAX_INLINE_NODE` to `TrieLayout`

**Plan did not mention this.**

**What was implemented:** A new `const MAX_INLINE_NODE: Option<u32> = None` on `TrieLayout`,
set to `Some(0)` in both `LayoutV0` and `LayoutV1`.

**Why:** The chain's codec panics on inline children (`ChildReference::Inline` with non-zero
length). The upstream `trie-db` inlines children smaller than `Hash::LENGTH` (32 bytes).
The existing `MAX_INLINE_VALUE` already forced all values to be hashed, but there was no
equivalent for children. Adding `MAX_INLINE_NODE` and checking it at all 5 inline-decision
sites (`triedbmut.rs` + 4 in `iter_build.rs`) fixed pre-existing test failures.

### 5. Codec handles `Inline(_, 0)` sentinel

**Plan did not mention this.**

**What was implemented:** `node_codec.rs` `branch_node_nibbled` now treats
`ChildReference::Inline(_, 0)` as absent (bitmap bit = false) instead of panicking.

**Why:** The proof generator uses `Inline(zero, 0)` as a sentinel meaning "this child
hash is omitted from the compact proof." With the codec's strict no-inline-children policy,
this sentinel was triggering the panic. The zero-length case is semantically "absent" and
needs to pass through without encoding any child data.

### 6. `hash_value` delegates to `hash_for_circuit`, not `injective_bytes_to_felts` directly

**Plan said:** `hash_value` would call `injective_bytes_to_felts` then hash the felts.

**What was implemented:** `hash_value` delegates to `Self::hash_for_circuit(value)`,
which is the existing padded injective encoding path.

**Why:** `hash_for_circuit` already wraps the injective encoding with the correct padding
and length handling used everywhere else. Calling it directly avoids duplicating that logic.

### 7. `Goldilocks::from_u64` instead of `from_canonical_u64`

**Plan said:** `Goldilocks::from_canonical_u64(u64::from_le_bytes(buf))`.

**What was implemented:** `Goldilocks::from_u64(u64::from_le_bytes(buf))`.

**Why:** The `p3-field` crate (v0.3.0) generates `from_u64` via a macro on the
`PrimeCharacteristicRing` trait. `from_canonical_u64` does not exist on `Goldilocks`
in this version. Both perform modular reduction, which is correct for felt-aligned data.
