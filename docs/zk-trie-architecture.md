# ZK Trie: Poseidon Merkle Tree for Zero-Knowledge Transfer Proofs

A 4-ary Poseidon Merkle tree that provides ZK-friendly commitment to all transfers on the Quantus chain.

## Why a Separate Tree?

Substrate's storage trie uses Blake2b hashing. Blake2b is fast for native execution but extremely expensive inside ZK circuits (~100x more constraints than Poseidon). A user wanting to prove "I received 500 QTU" would need to verify Blake2b Merkle paths inside a SNARK -- this would make proofs impractically slow and large.

The ZK trie solves this by maintaining a **parallel commitment structure** using Poseidon hashing, which is native to arithmetic circuits. The key insight: **the ZK trie's storage (leaves, nodes, root) lives inside the normal Substrate state trie**. It is pallet storage, not a separate database. This means it inherits all of Substrate's state management guarantees (atomicity, rollback, finality) for free.

```
┌─────────────────────────────────────────────────────────────┐
│                      Block Header                           │
│                                                             │
│  parent_hash ─── state_root ─── extrinsics_root ─── digest │
│                      │                                │     │
│                      ▼                           ┌────┘     │
│              ┌───────────────┐                   │          │
│              │ Substrate     │            DigestItem::Other  │
│              │ State Trie    │            ┌──────────────┐   │
│              │ (Blake2b)     │            │  ZK Trie     │   │
│              │               │            │  Root Hash   │   │
│              │ ┌───────────┐ │            │  (32 bytes)  │   │
│              │ │ Balances  │ │            └──────────────┘   │
│              │ │ Wormhole  │ │                               │
│              │ │ QPoW      │ │                               │
│              │ │ ...       │ │                               │
│              │ │           │ │                               │
│              │ │ ┌───────┐ │ │                               │
│              │ │ │ZkTrie │◄├─┼──── Pallet storage holding    │
│              │ │ │Leaves │ │ │     the Poseidon tree data    │
│              │ │ │Nodes  │ │ │                               │
│              │ │ │Root   │ │ │                               │
│              │ │ │Depth  │ │ │                               │
│              │ │ └───────┘ │ │                               │
│              │ └───────────┘ │                               │
│              └───────────────┘                               │
└─────────────────────────────────────────────────────────────┘
```

The ZK trie root is additionally published in the **block digest** on every `on_finalize`, giving light clients and external verifiers a direct commitment without needing to parse storage proofs.

## Tree Structure

A 4-ary (quaternary) Merkle tree where each internal node has 4 children.

```
Depth 3 (capacity: 64 leaves)

                              [Root]                           Level 3
                           /    |    \     \
                     [N0]     [N1]    [N2]    [N3]             Level 2
                    / | \ \   / | \ \
              [N00]  ...      [N10] ...                        Level 1
              / | \ \
          [L0][L1][L2][L3]                                     Level 0 (leaves)
```

| Depth | Capacity (4^d) | Notes                    |
|-------|---------------|--------------------------|
| 0     | 0             | Empty tree               |
| 1     | 4             | First 4 transfers        |
| 2     | 16            | Grows automatically      |
| 3     | 64            |                          |
| ...   | ...           |                          |
| 32    | ~1.8 × 10^19  | Maximum supported depth  |

The tree grows dynamically -- when the 5th leaf arrives, depth increases from 1 to 2. The old root becomes child[0] of a new root node.

### Hashing Strategy

| Layer              | Encoding                  | Felts | Injective? |
|--------------------|---------------------------|-------|------------|
| **Leaves**         | Structured (see below)    | 8     | Yes (all values ≤ 32 bits except account) |
| **Internal nodes** | 8 bytes/felt compact      | 16    | No (hash outputs may exceed field modulus) |

**Leaf encoding** (8 field elements):
```
┌─────────────────────┬──────────────────┬───────────┬─────────────────┐
│   to_account        │ transfer_count   │ asset_id  │ amount          │
│   (4 felts)         │ (2 felts)        │ (1 felt)  │ (1 felt)        │
│   32 bytes →        │ u64 as high/low  │ u32       │ u32 quantized   │
│   8 bytes/felt      │ 32-bit limbs     │           │ (raw / 10^10)   │
└─────────────────────┴──────────────────┴───────────┴─────────────────┘
                              │
                        poseidon(8 felts)
                              │
                              ▼
                      leaf_hash (32 bytes)
```

The leaf encoding is effectively injective because:
- `transfer_count`, `asset_id`, `amount` are all ≤ 32 bits, well under the Goldilocks field modulus (~2^64)
- `to_account` uses 8 bytes/felt which could theoretically wrap, but AccountId32 values in practice don't collide

**SCALE-encoded leaf data** (60 bytes on-chain storage):
- `to`: 32 bytes (AccountId32)
- `transfer_count`: 8 bytes (u64 LE)
- `asset_id`: 4 bytes (u32 LE)
- `amount`: 16 bytes (u128 LE, **raw planck value** - full precision stored)

> **Why no `from` address?** The ZK circuit proves ownership of received funds. Uniqueness is guaranteed by `(to, transfer_count)` -- each recipient has a monotonically increasing counter. The sender's identity is irrelevant for proving "I received this transfer."

**Node hash** (128 bytes input):
```
sort(child[0..3])  →  concatenate  →  bytes_to_felts_compact  →  poseidon  →  node_hash
  4 × 32 B               128 B              16 felts                           32 B
```

Children are sorted before hashing, making the hash order-independent. This eliminates path indices from Merkle proofs -- the verifier just combines current hash with 3 siblings, sorts all 4, and hashes.

## Data Flow: From Transfer to ZK Proof

### 1. Transfer Recording

Every balance transfer on the chain is automatically captured and recorded into the ZK trie via a transaction extension.

```
   User submits tx (e.g., transfer 500 QTU to Bob)
                         │
                         ▼
   ┌──────────────────────────────────────────────┐
   │ Transaction Execution                         │
   │                                               │
   │  pallet_balances::transfer()                  │
   │       │                                       │
   │       ├── moves funds                         │
   │       └── emits Event::Transfer {from, to, amount}
   │                                               │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ WormholeProofRecorderExtension::post_dispatch │
   │ (runs after EVERY transaction automatically)  │
   │                                               │
   │  1. Scans new events since tx start           │
   │  2. Finds Transfer/Minted/Issued events       │
   │  3. Calls Wormhole::record_transfer() each    │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ Wormhole::record_transfer()                   │
   │                                               │
   │  1. Reads TransferCount[bob] → current_count  │
   │  2. Increments TransferCount[bob]             │
   │  3. Calls T::ZkTrie::record_transfer(         │
   │       bob, current_count, asset_id, amount)   │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ ZkTrie::insert_leaf()                         │
   │                                               │
   │  1. Store leaf data at next index             │
   │  2. Compute leaf_hash = poseidon(leaf_data)   │
   │  3. Update path from leaf to root             │
   │  4. Store new root                            │
   │  5. Emit LeafInserted event                   │
   └──────────────────────────────────────────────┘
```

### 2. Generating a ZK Proof

To prove "I (Bob) received transfer #3 of 500 QTU," a user (or wallet software) follows this flow:

```
   ┌──────────────────────────────────────────────┐
   │ Step 1: Query the Merkle proof via RPC        │
   │                                               │
   │  POST zkTrie_getMerkleProof(leaf_index: 42)   │
   │                                               │
   │  Response:                                    │
   │  {                                            │
   │    leaf_index: 42,                            │
   │    leaf_data: <encoded ZkLeaf>,               │
   │    leaf_hash: [u8; 32],                       │
   │    siblings: [[sibling; 3]; depth],           │
   │    root: [u8; 32],                            │
   │    depth: 3                                   │
   │  }                                            │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ Step 2: Build the ZK circuit witness          │
   │                                               │
   │  Private inputs (known only to prover):       │
   │    - leaf_data (to, transfer_count,           │
   │                 asset_id, amount)             │
   │    - siblings at each tree level              │
   │                                               │
   │  Public inputs (visible to verifier):         │
   │    - zk_trie_root (from block digest)         │
   │    - claimed amount (or other public claims)  │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ Step 3: ZK circuit logic                      │
   │                                               │
   │  1. Recompute leaf_hash from leaf_data        │
   │     using Poseidon (efficient in circuit)     │
   │                                               │
   │  2. Walk up the tree using siblings:          │
   │     for each level:                           │
   │       combine current_hash with 3 siblings    │
   │       sort all 4                              │
   │       current_hash = poseidon(sorted)         │
   │                                               │
   │  3. Assert computed_root == public_root        │
   │                                               │
   │  This proves: "a transfer with these exact    │
   │  properties exists in the committed tree"     │
   │  without revealing the full tree contents.    │
   └───────────────────────┬───────────────────────┘
                           │
                           ▼
   ┌──────────────────────────────────────────────┐
   │ Step 4: Submit proof on-chain                 │
   │                                               │
   │  The SNARK proof can be verified by anyone    │
   │  against the zk_trie_root in the block digest │
   │  (or in ZkTrie pallet storage).               │
   └──────────────────────────────────────────────┘
```

**Why this works even though the ZK trie is "outside" the normal trie:**

The ZK trie is not outside the state trie -- its storage *lives inside* the state trie as normal pallet storage items (`Leaves`, `Nodes`, `Root`, `Depth`, `LeafCount`). What's "separate" is the **hashing scheme**: the Poseidon Merkle tree structure is an overlay that uses ZK-friendly hashes, while the underlying storage still uses Substrate's Blake2b trie for persistence.

The RPC node reads ZK trie data from pallet storage (via the Substrate state) and returns the Poseidon Merkle proof. The user never needs to interact with Blake2b storage proofs at all -- the entire proof path uses Poseidon.

```
                    What the RPC returns (all Poseidon)
                    ──────────────────────────────────

     Block Digest ──── zk_trie_root (Poseidon)
                            │
              ┌─────────────┼─────────────┐
              │             │             │
         [Poseidon]    [Poseidon]    [Poseidon]     ← siblings (level 2)
              │
        ┌─────┼─────┬─────┐
        │     │     │     │
    [Pos] [Pos] [Pos] [Pos]                         ← siblings (level 1)
              │
        leaf_hash = injective_poseidon(to, count, asset_id, amount)

     The ZK circuit only ever sees Poseidon hashes.
     Blake2b is invisible to the prover/verifier.
```

## Handling Block Reorgs

### The Problem

If block N is reverted due to a chain reorganization, any ZK trie leaves inserted during block N must also be reverted. Since the ZK trie is a cumulative data structure (append-only, with each insert changing the root), a naive approach could leave the tree in an inconsistent state.

### The Solution: Substrate State Management

Because the ZK trie is **pallet storage**, reorgs are handled automatically by Substrate's state management. There is no extra work needed.

```
   Block 99 (finalized)
       state_root_99 ─── ZkTrie { root: R99, leaf_count: 1000, ... }
            │
            ▼
   Block 100 (canonical)
       state_root_100 ─── ZkTrie { root: R100, leaf_count: 1005, ... }
            │
            ├──── Block 101a (fork A - canonical)
            │         state_root_101a ─── ZkTrie { root: R101a, leaf_count: 1012, ... }
            │
            └──── Block 101b (fork B - uncle)
                      state_root_101b ─── ZkTrie { root: R101b, leaf_count: 1010, ... }
```

**What happens on reorg (fork B wins):**

```
   1. Substrate detects fork B has more cumulative work
   2. Substrate reverts block 101a's state changes
   3. Substrate applies block 101b's state changes
   4. ALL pallet storage (including ZkTrie) atomically reverts to block 100's state
   5. Block 101b's transactions are re-executed against block 100's state
   6. ZkTrie insertions from 101b produce a new root R101b

   Result: ZkTrie is consistent, as if 101a never happened.
```

This works because:
- Each block's state is a complete snapshot of all storage (including ZK trie data)
- Substrate's state DB maintains state for non-finalized blocks on all forks
- Reverting a block means switching to the parent state, not "undoing" operations
- The ZK trie root is deterministic: same inputs in same order = same root

### Finality Considerations

Quantus uses QPoW consensus with a `MaxReorgDepth` of **180 blocks**. Blocks older than 180 blocks behind the tip are finalized and cannot be reverted.

```
                    ◄────── 180 blocks ──────►
   ┌──────────┬────────────────────────────────┬─────────┐
   │ Finalized│      Can be reorged            │  Tip    │
   │          │                                │         │
   │ ZK proofs│  ZK proofs here are valid but  │ Latest  │
   │ here are │  could change if reorg occurs  │ state   │
   │ permanent│                                │         │
   └──────────┴────────────────────────────────┴─────────┘
```

**Recommendation for ZK proof consumers:**

| Use Case | Wait for | Why |
|----------|----------|-----|
| Low-value transfers | Best block | Root is very likely final |
| High-value transfers | Finalized block | Root is guaranteed permanent |
| Cross-chain bridges | Finalized block | Must be irreversible |

The RPC queries (`zkTrie_getState`, `zkTrie_getMerkleProof`) operate against the **best block** by default. For finality-sensitive applications, query at a specific finalized block hash instead.

### What About Leaf Indices?

Since the ZK trie is append-only (leaves are never removed), leaf indices are stable within a given fork. However, a reorg can cause the same leaf index to map to different transfer data on different forks.

After finalization, a leaf index is permanently bound to its data. Before finalization, treat leaf indices as tentative.

## Storage Layout

All ZK trie data is stored as standard Substrate pallet storage:

| Storage Item | Key | Value | Description |
|-------------|-----|-------|-------------|
| `Leaves` | `u64` (leaf index) | `ZkLeaf` | Raw leaf data |
| `Nodes` | `(u8, u64)` (level, index) | `Hash256` | Internal node hashes |
| `LeafCount` | -- | `u64` | Total leaves inserted |
| `Depth` | -- | `u8` | Current tree depth |
| `Root` | -- | `Hash256` | Current Poseidon root |

Storage keys use `Identity` hasher since leaf indices are sequential (no adversarial key selection).

## RPC API

| Method | Parameters | Returns | Description |
|--------|-----------|---------|-------------|
| `zkTrie_getState` | -- | `{root, leaf_count, depth}` | Current tree state |
| `zkTrie_getMerkleProof` | `leaf_index: u64` | `ZkMerkleProofRpc \| null` | Full Merkle proof for a leaf |

### Example: Query a Merkle Proof

```json
// Request
{
  "jsonrpc": "2.0",
  "method": "zkTrie_getMerkleProof",
  "params": [42],
  "id": 1
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "leaf_index": 42,
    "leaf_data": "0x...",
    "leaf_hash": "0x...",
    "siblings": [
      ["0x...", "0x...", "0x..."],
      ["0x...", "0x...", "0x..."]
    ],
    "root": "0x...",
    "depth": 2
  },
  "id": 1
}
```

The `leaf_data` field is SCALE-encoded `ZkLeaf<AccountId32, u32, u128>` (60 bytes total):
- Bytes 0-31: `to` (AccountId32)
- Bytes 32-39: `transfer_count` (u64 LE)
- Bytes 40-43: `asset_id` (u32 LE)
- Bytes 44-59: `amount` (u128 LE, raw planck value)

Note: The amount stored on-chain is the raw planck value. When hashing for the ZK circuit, amounts are **quantized** by dividing by 10^10 to fit in a single field element with 2 decimal places of precision.
