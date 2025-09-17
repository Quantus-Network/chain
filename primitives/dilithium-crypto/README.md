# Quantus - Dilithium Crypto

A Rust implementation of Dilithium post-quantum cryptographic signatures for Substrate-based blockchains.

## Overview

This crate provides Dilithium digital signature functionality optimized for use in Substrate runtime environments. Dilithium is a post-quantum cryptographic signature scheme that is part of the NIST Post-Quantum Cryptography Standardization process.

## Features

- **No-std compatible**: Can be used in Substrate runtime environments
- **Substrate integration**: Built-in support for Substrate's cryptographic traits
- **Configurable features**: Optional std support and full crypto features
- **Serde support**: Optional serialization/deserialization support

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
qp-dilithium-crypto = "0.1.0"
```

### Basic Example

```rust
use qp_dilithium_crypto::DilithiumSignature;

// Example usage will be added here
```

## Features

- `default`: Enables `std` feature
- `std`: Standard library support
- `full_crypto`: Enables full cryptographic functionality
- `serde`: Enables serialization support

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.