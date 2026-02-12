# Reversible Transfers Pallet

## Motivation

To have accounts for which all outgoing transfers are subject to a variable time during which they may be cancelled. The idea is this could be used to deter theft as well as correct mistakes.

**Use Cases:**
- **High-security custody:** Corporate treasury with guardian oversight
- **Mistake recovery:** Cancel accidental transfers during delay period
- **Theft deterrence:** Guardian can cancel suspicious transfers before execution
- **Regulatory compliance:** Time-delayed transfers with oversight capabilities

## Design

Pallet uses `Scheduler` and `Preimages` pallets internally to handle scheduling and lookup calls, respectively. For every transfer submitted by the reversible account, in order:
1. Preimage note is taken 
2. Stored in the pallet's `PendingTransfers` which maps the unique tx ID `((who, call).hash())` to `(origin, pending_transfer)`. 
3. Schedules the `ReversibleTransfers::execute_transfer` with name `tx_id`, so that user is able to cancel the dispatch by the `tx_id`
4. At the execution block, scheduler calls the `execute_transfer` which *takes* the call from `Preimage` and dispatches it, cleaning up the storage.

NOTE: failed transfers are not retried, in this version

### Delay policy

Pallet currently offers two policies/ways for transaction delaying: `Explicit` and `Intercept`:

- `Explicit`: default behaviour, where reversible accounts need to call delayed transfers through `pallet_reversible_transfers::schedule_transfer` extrinsic. Directly calling the transaction will be rejected by `ReversibleTransactionExtension` as invalid.
- `Intercept`: this is the superset of `Explicit`, and allows the `ReversibleTransactionExtension` to intercept delayed transactions in the validation phase and internally call `do_schedule_dispatch` function. The downside is, since we are delaying the call in validation level, we should reject the transaction as invalid, which is not really good for UX. In theory, it should be possible to introduce `Pending` state to `TransactionValidity` by forking crates, but that's not implemented yet.

### Tracking

Pending/delayed transfers can be tracked at `PendingTransfers` storage and by subscribing to `ReversibleTransfersEvent::TransactionScheduled{..}` event.

### Storages

- `ReversibleAccounts`: list of accounts that are `reversible` accounts. Accounts can call `ReversibleTransfers::set_reversability` extrinsic to join this set.
- `PendingTransfers`: stores current pending dispatches for the user. Maps `tx_id` to `(caller, pending_dispatch)`. We store the caller so that we can validate the user who's canceling the dispatch.
- `AccountPendingIndex`: stores the current count of pending transactions for the user so that they don't exceed `MaxPendingPerAccount`

### Notes

- Transaction id is `((who, call).hash())` where `who` is the account that called the transaction and `call` is the call itself. This is used to identify the transaction in the scheduler and preimage. For identical transfers, there is a counter in `PendingTransfer` to differentiate between them.

## High-Security Integration

This pallet provides the **HighSecurityInspector** trait for integrating high-security features with other pallets (like `pallet-multisig`).

### HighSecurityInspector Trait

```rust
pub trait HighSecurityInspector<AccountId, RuntimeCall> {
    /// Check if account is registered as high-security
    fn is_high_security(who: &AccountId) -> bool;
    
    /// Check if call is whitelisted for high-security accounts
    fn is_whitelisted(call: &RuntimeCall) -> bool;
    
    /// Get guardian account for high-security account (if exists)
    fn guardian(who: &AccountId) -> Option<AccountId>;
}
```

**Purpose:**
- Provides unified interface for high-security checks
- Used by `pallet-multisig` for call whitelisting
- Used by transaction extensions for EOA whitelisting
- Implemented by runtime for call pattern matching

### Implementation

**This pallet provides:**
- Trait definition (`pub trait HighSecurityInspector`)
- Helper functions for runtime implementation:
  - `is_high_security_account(who)` - checks `HighSecurityAccounts` storage
  - `get_guardian(who)` - retrieves guardian from storage
- Default no-op implementation: `impl HighSecurityInspector for ()`

**Runtime implements:**
- The actual `is_whitelisted(call)` logic (requires `RuntimeCall` access)
- Delegates `is_high_security` and `guardian` to pallet helper functions
- Example:

```rust
pub struct HighSecurityConfig;

impl HighSecurityInspector<AccountId, RuntimeCall> for HighSecurityConfig {
    fn is_high_security(who: &AccountId) -> bool {
        // Delegate to pallet helper
        ReversibleTransfers::is_high_security_account(who)
    }

    fn is_whitelisted(call: &RuntimeCall) -> bool {
        // Runtime implements pattern matching (has RuntimeCall access)
        matches!(
            call,
            RuntimeCall::ReversibleTransfers(Call::schedule_transfer { .. }) |
            RuntimeCall::ReversibleTransfers(Call::schedule_asset_transfer { .. }) |
            RuntimeCall::ReversibleTransfers(Call::cancel { .. })
        )
    }

    fn guardian(who: &AccountId) -> Option<AccountId> {
        // Delegate to pallet helper
        ReversibleTransfers::get_guardian(who)
    }
}
```

### Usage by Other Pallets

**pallet-multisig:**
```rust
impl pallet_multisig::Config for Runtime {
    type HighSecurity = HighSecurityConfig;
    // ...
}

// In multisig propose():
if T::HighSecurity::is_high_security(&multisig_address) {
    let decoded_call = RuntimeCall::decode(&call)?;
    ensure!(
        T::HighSecurity::is_whitelisted(&decoded_call),
        Error::CallNotAllowedForHighSecurityMultisig
    );
}
```

**Transaction Extensions:**
```rust
// In ReversibleTransactionExtension::validate():
if HighSecurityConfig::is_high_security(&who) {
    ensure!(
        HighSecurityConfig::is_whitelisted(&call),
        TransactionValidityError::Invalid(InvalidTransaction::Call)
    );
}
```

### Architecture Benefits

**Single Source of Truth:**
- Whitelist defined once in runtime
- Used by multisig, transaction extensions, and any future consumers
- Easy to maintain and update

**Modularity:**
- Trait defined in this pallet (storage owner)
- Implementation in runtime (has `RuntimeCall` access)
- Consumers use trait without coupling to implementation

**Reusability:**
- Same security model for EOAs and multisigs
- Consistent whitelist enforcement across all account types
- Easy to add new consumers (just use the trait)

### Documentation

See `MULTISIG_REQ.md` for complete high-security integration architecture and examples.
