# Structured Trie Hasher: Implementation Plan

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

Introduce a `TrieHasher` trait that lets the hasher distinguish between **encoded trie nodes**
and **raw storage values**, enabling `PoseidonHasher` to:

- **Skip the injective encoder for nodes** — directly interpret 8-byte chunks as Goldilocks felts
- **Apply the injective encoder only for values** — arbitrary-length user data that isn't felt-aligned

This eliminates encoding constraints from every trie node hash in the ZK circuit.

## Trait Design

### New trait in `hash-db`

```rust
pub trait TrieHasher: Hasher {
    /// Hash a fully encoded trie node (leaf, branch, or empty).
    /// The implementation may assume the input is felt-aligned.
    fn hash_node(encoded_node: &[u8]) -> Self::Out;

    /// Hash a raw storage value that exceeded the inline threshold.
    fn hash_value(value: &[u8]) -> Self::Out;
}
```

**Why this shape:**

- `TrieHasher: Hasher` — superset, not replacement. All non-trie code that only needs
  `Hasher` compiles unchanged.
- **Two methods, not per-node-type methods** — the earlier idea of `hash_leaf(partial, value)` /
  `hash_branch(partial, children, value)` would require threading individual fields through every
  encoding path. Instead, `hash_node` receives the already-encoded bytes. Since we control both
  the codec and the hasher, the hasher can re-interpret the felt-aligned structure directly.
- No `hash_key` method — `FatDB`/`SecTrieDB` (which hash user keys) are unused in this codebase.

### Extended `HashDB` trait

```rust
pub trait HashDB<H: TrieHasher, T>: Send + Sync + AsHashDB<H, T> {
    fn get(&self, key: &H::Out, prefix: Prefix) -> Option<T>;
    fn contains(&self, key: &H::Out, prefix: Prefix) -> bool;
    fn insert(&mut self, prefix: Prefix, value: &[u8]) -> H::Out;
    fn emplace(&mut self, key: H::Out, prefix: Prefix, value: T);
    fn remove(&mut self, key: &H::Out, prefix: Prefix);

    /// Insert an encoded trie node, hashing with `TrieHasher::hash_node`.
    fn insert_node(&mut self, prefix: Prefix, encoded_node: &[u8]) -> H::Out {
        // Default: same as insert (backward compatible)
        self.insert(prefix, encoded_node)
    }
}
```

The default `insert_node` delegates to `insert`, so all existing `HashDB` impls compile.
Only `memory-db` overrides it to route through `hash_node`.

## Call Site Inventory

Every `H::hash(&[u8])` call in the trie stack falls into exactly 3 categories:

| Category | Description | Action |
|----------|-------------|--------|
| **Encoded trie node** | Full encoded node bytes (leaf, branch, empty, root) | → `hash_node` / `insert_node` |
| **Storage value** | Raw value bytes exceeding inline threshold | → `hash_value` / `insert` |
| **Sentinel** | Null key / empty marker (fixed small input) | → `Hasher::hash` (base trait) |

## Changes Per Crate

### 1. `hash-db` (local fork)

**Files:** `src/lib.rs`

| Change | Detail |
|--------|--------|
| Add `TrieHasher` trait | As shown above, extends `Hasher` |
| Update `HashDB` trait bound | `H: Hasher` → `H: TrieHasher` |
| Add `insert_node` default method | On `HashDB` with fallback to `insert` |
| Update `HashDBRef`, `AsHashDB` | Same bound change: `H: Hasher` → `H: TrieHasher` |

**Estimated diff:** ~25 lines

### 2. `memory-db` (local fork)

**Files:** `src/lib.rs`

| Change | Detail |
|--------|--------|
| Bound change | `H: KeyHasher` → `H: KeyHasher + TrieHasher` where needed |
| `HashDB::insert` impl (line 541) | Change `H::hash(value)` → `H::hash_value(value)` |
| Add `HashDB::insert_node` override | Calls `H::hash_node(value)` then `emplace` |
| `from_null_node` (line 317) | Keep as `H::hash(null_key)` — sentinel, uses base trait |

**Estimated diff:** ~30 lines

### 3. `trie-root` (needs local patch or fork)

**Files:** `src/lib.rs`

| Line | Current | Change to | Reason |
|------|---------|-----------|--------|
| 59 | `H::hash(value)` | `H::hash_value(value)` | Hashing a value that exceeded threshold |
| 166 | `H::hash(&stream.out())` | `H::hash_node(&stream.out())` | Hashing the root node encoding |
| Trait bounds | `H: Hasher` | `H: TrieHasher` | On `trie_root_inner`, `trie_root_no_extension`, `unhashed_trie_no_extension`, `sec_trie_root` |

**Estimated diff:** ~15 lines

### 4. `trie-db` (local, at `primitives/trie-db`)

#### `iter_build.rs` — `ProcessEncodedNode` impls

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 389 | `self.db.insert(prefix, &encoded_node)` | `self.db.insert_node(prefix, &encoded_node)` | `TrieBuilder::process` — encoded node |
| 397 | `self.db.insert(prefix, value)` | (unchanged) | `TrieBuilder::process_inner_hashed_value` — value |
| 427 | `<T::Hash as Hasher>::hash(encoded_node.as_slice())` | `<T::Hash as TrieHasher>::hash_node(encoded_node.as_slice())` | `TrieRoot::process` — encoded node |
| 435 | `<T::Hash as Hasher>::hash(value)` | `<T::Hash as TrieHasher>::hash_value(value)` | `TrieRoot::process_inner_hashed_value` — value |
| 486 | `<T::Hash as Hasher>::hash(encoded_node.as_slice())` | `<T::Hash as TrieHasher>::hash_node(...)` | `TrieRootPrint::process` — encoded node |
| 496 | `<T::Hash as Hasher>::hash(value)` | `<T::Hash as TrieHasher>::hash_value(value)` | `TrieRootPrint::process_inner_hashed_value` — value |
| 514 | `<T::Hash as Hasher>::hash(encoded_node.as_slice())` | `<T::Hash as TrieHasher>::hash_node(...)` | `TrieRootUnhashed::process` — encoded node |
| 523 | `<T::Hash as Hasher>::hash(value)` | `<T::Hash as TrieHasher>::hash_value(value)` | `TrieRootUnhashed::process_inner_hashed_value` — value |

#### `triedbmut.rs` — `commit` / `commit_child`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 1837 | `self.db.insert(k.as_prefix(), value)` | (unchanged) | Hashing a storage value |
| 1852 | `self.db.insert(EMPTY_PREFIX, &encoded_root)` | `self.db.insert_node(EMPTY_PREFIX, &encoded_root)` | Hashing the encoded root node |
| 1977 | `self.db.insert(prefix.as_prefix(), value)` | (unchanged) | Hashing a storage value |
| 1994 | `self.db.insert(prefix.as_prefix(), &encoded)` | `self.db.insert_node(prefix.as_prefix(), &encoded)` | Hashing an encoded child node |

#### `trie_codec.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 519 | `db.insert(...)` for attached value | (unchanged) | Value |
| 521 | `db.insert(...)` for encoded node | `db.insert_node(...)` | Node |

#### `proof/verify.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 258 | `H::hash(value)` | `H::hash_value(value)` | Value exceeding inline threshold |
| 457 | `H::hash(node_data)` | `H::hash_node(node_data)` | Encoded node during proof unwind |

#### `node.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 134 | `L::Hash::hash(data)` | `L::Hash::hash_value(data)` | Inline value → `ValueOwned` |

#### `lookup.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 411 | `L::Hash::hash(v)` | `L::Hash::hash_value(v)` | Inline value Merkle hash |

#### Trait bounds

All `H: Hasher` bounds on `TrieLayout`, `TrieConfiguration`, and related types change
to `H: TrieHasher`.

#### `fatdb*.rs` / `sectriedb*.rs`

Not used in this codebase. For completeness: these hash user keys and should use
`H::hash()` (base trait). No change needed — they already use base `Hasher::hash`.

**Estimated diff in trie-db:** ~60 lines

### 5. `sp-trie` (local, at `primitives/trie`)

#### `node_codec.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 86 | `H: Hasher` | `H: TrieHasher` | Bound on `NodeCodec<H>` |
| 94 | `H::hash(empty_node)` | `H::hash_node(empty_node)` | Hashing the empty node encoding |

#### `lib.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 93, 146 | `H: Hasher` on `LayoutV0`, `LayoutV1` | `H: TrieHasher` | Layout bounds |
| 105, 156 | `H: Hasher` on `TrieConfiguration` impls | `H: TrieHasher` | Same |

#### `recorder.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 58 | `Hasher::hash(&n)` | `Hasher::hash_node(&n)` | Proof nodes are encoded trie nodes |
| 70 | `Hasher::hash(&data)` | `Hasher::hash_node(&data)` | DB entries are encoded trie nodes |

**Estimated diff:** ~20 lines

### 6. `sp-state-machine` (local, at `primitives/state-machine`)

#### `ext.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 95, 111, 139, 163, 673, 800, 827 | `H: Hasher` bounds | `H: TrieHasher` | Bound propagation |
| 202 | `H::hash(x)` | `H::hash_value(x)` | `storage_hash` — hashing a storage value |
| 242 | `H::hash(x)` | `H::hash_value(x)` | `child_storage_hash` — hashing a storage value |

#### `trie_backend_essence.rs`

| Lines | Current | Change to | Reason |
|-------|---------|-----------|--------|
| 231, 249 | `H::hash(&[0u8])` | (unchanged) | Sentinel — uses base `Hasher::hash` |
| All `H: Hasher` bounds | | `H: TrieHasher` | Bound propagation |

#### `basic.rs`

Currently hardcodes `Blake2Hasher`. Implement `TrieHasher` for `Blake2Hasher` (trivially —
all methods delegate to `Blake2Hasher::hash`). No logic changes needed.

#### Other files

`in_memory_backend.rs`, `overlayed_changes/mod.rs`, `backend.rs`, `testing.rs`,
`read_only.rs`, `fuzzing.rs` — bound changes only (`H: Hasher` → `H: TrieHasher`).

**Estimated diff:** ~40 lines

### 7. `PoseidonHasher` (the optimization)

```rust
impl TrieHasher for PoseidonHasher {
    fn hash_node(encoded_node: &[u8]) -> H256 {
        // Node codec guarantees 8-byte alignment.
        // Each 8-byte chunk is one Goldilocks felt — no encoding overhead.
        let felts: Vec<Goldilocks> = encoded_node
            .chunks(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf[..chunk.len()].copy_from_slice(chunk);
                Goldilocks::from_canonical_u64(u64::from_le_bytes(buf))
            })
            .collect();
        hash_variable_length(felts).into()
    }

    fn hash_value(value: &[u8]) -> H256 {
        // Arbitrary bytes — must use injective encoding for safety.
        let felts = injective_bytes_to_felts::<Goldilocks>(value);
        hash_variable_length(felts).into()
    }
}
```

This is where the constraint savings come from. `hash_node` does zero encoding work —
it treats each 8-byte chunk as a native field element. The ZK circuit for node hashing
becomes: load felts directly from witness → feed into Poseidon2 sponge. No range checks,
no length separators, no byte packing logic.

**Estimated diff:** ~20 lines

### 8. `Blake2Hasher` (backward compat for tests)

```rust
impl TrieHasher for Blake2Hasher {
    fn hash_node(encoded_node: &[u8]) -> H256 {
        Blake2Hasher::hash(encoded_node)
    }

    fn hash_value(value: &[u8]) -> H256 {
        Blake2Hasher::hash(value)
    }
}
```

Trivial delegation. Ensures all tests using `Blake2Hasher` continue to work.

**Estimated diff:** ~10 lines

## Implementation Order

```
Step 1: hash-db          — add TrieHasher trait, update HashDB bounds
Step 2: memory-db        — implement insert_node, route through hash_node/hash_value
Step 3: trie-root        — update 2 call sites + bounds
Step 4: trie-db          — update ~15 call sites + bounds
Step 5: sp-trie          — update NodeCodec, layouts, recorder
Step 6: sp-state-machine — update bounds + ext.rs call sites
Step 7: Blake2Hasher     — trivial TrieHasher impl (unblocks tests)
Step 8: PoseidonHasher   — the real optimization
```

Steps 1–7 are mechanical. Step 8 is the payoff.

Each step should compile and pass tests before proceeding to the next.

## What Does NOT Change

| Component | Why unchanged |
|-----------|---------------|
| `frame_system::Config::Hashing` | Pallets use `T::Hashing::hash()` (base `Hasher` trait) |
| `qp-header` | Already has its own bespoke felt-aligned hashing via `Header::hash(&self)` |
| Wormhole pallet | Uses `PoseidonCore::hash_storage`, independent of trie hasher |
| PoW / mining | Uses `hash_squeeze_twice`, unrelated |
| Block import / consensus | Uses header hashing via `qp-header` |
| Host functions (`sp_io`) | Upstream, calls into state machine which carries the generic `H: TrieHasher` |

## Risk Assessment

### Consensus-breaking change

This changes trie node hashes and therefore every state root. **Requires genesis reset or
coordinated migration.** Pre-mainnet, genesis reset is presumably acceptable.

### Correctness of `hash_node`

The direct 8-byte-to-felt mapping only works because the ZK-trie codec guarantees 8-byte
alignment on all node encodings. Existing tests provide coverage:

- `random_test_8_byte_alignment` — random trie alignment over 20 seeds
- `storage_proof_8_byte_alignment_test` — random data + edge cases + non-inclusion proofs
- `child_reference_8_byte_boundary_test` — branch node child positioning

A new test should be added: round-trip verification that `hash_node(encode(node))` matches
the expected Poseidon output for known test vectors.

### Proof verification

ZK proofs of trie membership need the verifier to agree on the hashing scheme. The verifier
circuit uses the same direct felt-loading (which is exactly what makes this faster), so
both sides benefit.

## Circuit Constraint Savings

**Current path** (`PoseidonHasher::hash` with injective encoder):
- 64-byte node → injective encoding → ~10–12 felts + length overhead
- Circuit: range checks per felt + length separator constraints + Poseidon sponge

**Proposed path** (`PoseidonHasher::hash_node` with direct loading):
- 64-byte node → 8 felts (direct 8-byte chunks)
- Circuit: Poseidon sponge only

For every trie node hash verified in a block proof (typically 10–20+ nodes per storage
access, multiplied by all storage accesses in the block), the injective encoding constraints
are eliminated entirely. The savings compound across the entire block proof.

## Total Estimated Diff

| Crate | Lines changed |
|-------|---------------|
| hash-db | ~25 |
| memory-db | ~30 |
| trie-root | ~15 |
| trie-db | ~60 |
| sp-trie | ~20 |
| sp-state-machine | ~40 |
| PoseidonHasher | ~20 |
| Blake2Hasher | ~10 |
| **Total** | **~220 lines** |
