# Wormhole ZK Prover

This is a prover for the Wormhole zero-knowledge proof system. It generates proofs that can be verified by the Wormhole pallet on the blockchain.

## Prerequisites

- Rust 1.70+ with Cargo
- The circuit artifacts (common.hex, prover.hex, verifier.hex) in the `src/data` directory

## Building

```bash
cargo build --release
```

## Usage

### Generating Example Input Files

```bash
cargo run --bin generate_examples
```

This will create example JSON files in the `examples` directory:
- `private_inputs.json`: Contains private inputs (secrets, Merkle paths, etc.)
- `public_inputs.json`: Contains public inputs (nullifier, exit account, etc.)

### Generating a Proof

```bash
cargo run --bin prover_cli generate --private-inputs examples/private_inputs.json --public-inputs examples/public_inputs.json --output proof.hex
```

This will generate a proof and save it to the specified output file.

## Notes on Circuit Implementation

The prover requires three circuit data files:
1. `common.hex`: Common circuit data shared between prover and verifier
2. `prover.hex`: Prover-specific circuit data
3. `verifier.hex`: Verifier-specific circuit data

To use this prover with real circuit data:

1. Generate the circuit artifacts using the plonky2 toolchain
2. Place the circuit data files in the `src/data` directory
3. Implement the actual circuit logic in `src/circuit.rs`

## Implementing Your Circuit

The `process_witness` function in `src/circuit.rs` is where you'll implement the actual circuit logic. This function:

1. Takes private and public inputs
2. Builds the circuit and constraints
3. Returns witness values for the prover

The current implementation is a placeholder that needs to be replaced with your actual circuit implementation.

## Integration with the Substrate Chain

Once the proof is generated, it can be submitted to the Wormhole pallet on your Substrate chain using the `verify_wormhole_proof` extrinsic. 