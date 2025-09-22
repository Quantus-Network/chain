# Quantus Miner API

This crate defines the shared data structures and API contract used for communication between a Quantus Network node and external miner services.

It includes:

*   Request structures (e.g., `MiningRequest`).
*   Response structures (e.g., `MiningResponse`, `MiningResult`).
*   Status enums (`ApiResponseStatus`) used in responses.

By using this crate, both the node and external miner implementations can ensure they are using compatible data formats for submitting jobs and retrieving results.

## Usage

Add this crate as a dependency in the `Cargo.toml` of both the node and the external miner implementation.

**Node:**
```toml
[dependencies]
quantus-miner-api = { path = "../miner-api", default-features = false } 
# ... other dependencies
```

**External Miner:**
```toml
[dependencies]
quantus-miner-api = { path = "../miner-api" }
# Or if published:
# quantus-miner-api = "0.1.0"
# ... other dependencies
```

Then, import the types:

```rust
use quantus_miner_api::*;
``` 