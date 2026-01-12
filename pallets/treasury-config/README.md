# Treasury Config Pallet

A pallet for managing treasury multisig configuration in a Substrate-based blockchain.

## Overview

This pallet stores the treasury multisig signatories and threshold in runtime storage, allowing:
- **Different treasury addresses per network** (dev/heisenberg/dirac) from the same runtime build
- **Deterministic multisig address generation** compatible with `pallet-multisig`
- **Governance-driven updates** of signatories and threshold through tech referenda
- **Maximum transparency** via events when treasury configuration changes

## Features

### Storage

- **Signatories**: Bounded vector of AccountIds (max 100) who can sign treasury transactions
- **Threshold**: Number of signatures required (u16)

### Extrinsics

#### `set_treasury_signatories(signatories: Vec<AccountId>, threshold: u16)`
- **Origin**: Root (typically via governance/tech referenda)
- **Updates**: All signatories and threshold in a single transaction
- **Emits**: `TreasurySignatoriesUpdated` event with old and new multisig addresses

### Events

#### `TreasurySignatoriesUpdated { old_account, new_account }`
Emitted when treasury configuration changes, showing both the old and new multisig addresses for maximum transparency.

## Usage

### Genesis Configuration

```rust
TreasuryConfigConfig {
    signatories: vec![alice, bob, charlie, dave, eve],
    threshold: 3, // 3-of-5 multisig
}
```

### Updating via Governance

```rust
TreasuryConfig::set_treasury_signatories(
    RuntimeOrigin::root(),
    vec![account1, account2, account3, account4, account5],
    3 // new threshold
)
```

### Getting Treasury Address

```rust
let treasury_address = TreasuryConfig::get_treasury_account();
```

The address is deterministically derived from signatories and threshold using the same algorithm as `pallet-multisig`.

## Configuration

### Network-Specific Setup

- **Development**: 5 test signatories (Alice, Bob, Charlie, Dave, Eve), threshold 3
- **Heisenberg (testnet)**: 5 signatories, threshold 3  
- **Dirac (mainnet)**: 5 production signatories, threshold 3

Each network can have different signatories while using the same runtime build.

## Integration

Add to your runtime:

```rust
impl pallet_treasury_config::Config for Runtime {
    type MaxSignatories = ConstU32<100>;
    type WeightInfo = pallet_treasury_config::weights::SubstrateWeight<Runtime>;
}
```

## Testing

Run unit tests:
```bash
cargo test -p pallet-treasury-config
```

Run benchmarks:
```bash
cargo run --release --features runtime-benchmarks -- benchmark pallet \
    --pallet pallet_treasury_config \
    --extrinsic "*" \
    --steps 50 \
    --repeat 20
```

## Security Considerations

- Only Root origin can update signatories (typically requires governance approval)
- Genesis configuration must have valid signatories and threshold
- Address generation is deterministic and verifiable
- All changes emit events for transparency

## License

Apache-2.0

