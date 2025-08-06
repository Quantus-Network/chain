# ZK-Trie: Zero-Knowledge Friendly Substrate Trie

A fork of Substrate's `sp-trie` modified to be zero-knowledge proof friendly by ensuring all data structures are aligned to 8-byte (felt) boundaries.

## Overview

This is a customized implementation of Substrate's Patricia Merkle Trie (`sp-trie`) designed for zero-knowledge proof systems. The key modification is **felt-alignment**: all trie node structures are padded to 8-byte multiples, making them compatible with cryptographic field elements used in ZK proof systems.

## Motivation

### Why Fork sp-trie?

Standard Substrate tries use compact encoding to minimize storage overhead, but this creates problems for zero-knowledge proof systems. The standard Substrate MPT uses compact encoding with variable-length headers and data which results in node structures being unaligned to cryptographic field boundaries. Verifying an inclusion proof requires finding the hash of a child node in the parent node and that hash may not be aligned to a 8-byte boundary. This repository fixes that.

## Key Changes

### 1. Fixed 8-Byte Headers

**Before (Standard sp-trie):**
```rust
// Variable-length compact encoding
let header = compact_encode(node_type, nibble_count);
```

**After (ZK-Trie):**
```rust
// Fixed 8-byte header
#[derive(Encode, Decode)]
enum NodeHeader {
    Null,                           // Type 0
    Branch(bool, usize),           // Type 1/2
    Leaf(usize),                   // Type 3
    HashedValueBranch(usize),      // Type 4
    HashedValueLeaf(usize),        // Type 5
}

// Always encodes to exactly 8 bytes
let header_bytes = header.encode(); // [8 bytes]
```

### 2. Felt-Aligned Data Padding

**All data structures are padded to 8-byte boundaries:**

```rust
// Nibble data padding
let nibble_bytes = (nibble_count + 1) / 2;
let felt_aligned_bytes = ((nibble_bytes + 7) / 8) * 8;

// Value data padding
let value_len = value.len();
let padded_len = ((value_len + 7) / 8) * 8;
```

### 3. Modified Node Structure

**Standard Node:**
```
[compact_header][bitmap][nibbles][value][children...]
child = [child_length][inlined_or_hashed_child]...
```

**ZK-Trie Node:**
```
[8_byte_header][felt_aligned_bitmap][felt_aligned_nibbles][felt_aligned_value][felt_aligned_children...]
child = [8_byte_child_length][hashed_child]...
```

Note that there are no inlined children in ZK-Trie nodes. This is because the minimum size for a child node is now 32 bytes, the same size as the hash. The 8_byte_child_length could be removed in principle because the child hashes are all the same lengths, but to maintain consistency with the standard node structure, we left it in.

## Usage

### Integration with Substrate Runtime

Replace the standard `sp-trie` dependency:

```toml
[dependencies]
# sp-trie = "38.0.0"  # Standard version

[patch.crates-io]
sp-trie = { path = "path/to/zk-trie" }
```

This patching is necessary because sp-trie is a dependency of many Substrate packages.

### Runtime Configuration

If you want to use Poseidon for the storage root, do the following:

```rust
// Runtime configuration
impl frame_system::Config for Runtime {
    type Hashing = PoseidonHasher;  // ZK-friendly hasher
    // ...
}

// Trie layout
pub type LayoutV1 = sp_trie::LayoutV1<PoseidonHasher>;
```

### Logging

The implementation includes targeted logging for debugging:

```bash
# View only zk-trie logs
RUST_LOG=zk-trie=debug ./your-node --dev

# Mix with other logs
RUST_LOG=warn,zk-trie=debug ./your-node --dev
```

## Technical Details

### Node Header Encoding

The 8-byte header uses a structured format:

```rust
// Header structure (8 bytes)
// Bits 63-60: Node type (4 bits)
// Bits 31-0:  Nibble count (32 bits)
// Bits 59-32: Reserved (28 bits)

let header_value = (node_type << 60) | nibble_count;
header_value.to_le_bytes()  // 8 bytes
```

### Memory Overhead

The felt-alignment increases storage / memory requirements. Storage proofs are ~35% larger due to the padding.

We consider this overhead acceptable for ZK applications where proof generation efficiency is more important than storage optimization.

## Migration from Standard sp-trie

### Clean Migration (Recommended)

For development chains, start with a fresh database:

```bash
# Clean old database
rm -rf /path/to/substrate/db

# Start with zk-trie
./your-node --dev --tmp
```

## Contributing

When contributing to zk-trie:

1. **Maintain felt-alignment**: All new data structures must be 8-byte aligned
2. **Preserve compatibility**: Don't break the `sp-trie` API
3. **Add tests**: Please don't change existing tests if possible
4. **Update docs**: Document any changes to the encoding format

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Acknowledgments

- Based on Substrate's `sp-trie` implementation by Parity Technologies
