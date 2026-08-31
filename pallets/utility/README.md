# Utility Module
A stateless module with helpers for dispatch management which does no re-authentication.

- [`utility::Config`](https://docs.rs/pallet-utility/latest/pallet_utility/pallet/trait.Config.html)
- [`Call`](https://docs.rs/pallet-utility/latest/pallet_utility/pallet/enum.Call.html)

## Overview

This module exposes a single dispatchable: atomic batch dispatch. Any origin except
`None` can execute multiple calls in one extrinsic; if any child fails, the whole
transaction rolls back.

Other FRAME utility combinators (`batch`, `force_batch`, `if_else`, `as_derivative`,
`dispatch_as`, `with_weight`) are omitted to keep the runtime call surface small.

## Interface

### Dispatchable Functions

- `batch_all` - Dispatch multiple calls from the sender's origin, atomically.

[`Call`]: ./enum.Call.html
[`Config`]: ./trait.Config.html

License: Apache-2.0
