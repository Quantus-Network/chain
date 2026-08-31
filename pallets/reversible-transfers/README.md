# Reversible Transfers Pallet

## Motivation

To have accounts for which all outgoing transfers are subject to a variable time during which they may be cancelled. The idea is this could be used to deter theft as well as correct mistakes.

**Use Cases:**
- **High-security custody:** Corporate treasury with guardian oversight
- **Mistake recovery:** Cancel accidental transfers during delay period
- **Theft deterrence:** Guardian can cancel suspicious transfers before execution
- **Regulatory compliance:** Time-delayed transfers with oversight capabilities

## Design

Pallet uses the `Scheduler` pallet internally to handle scheduling transfer execution. For every transfer submitted by a high-security account:

1. The transfer details (from, to, guardian, amount, asset_id) are stored in `PendingTransfers`
2. Funds are held (frozen) from the sender's balance
3. A unique `tx_id` is generated using `hash(who, call, NextTransactionId)`
4. The `execute_transfer` call is scheduled via the Scheduler pallet
5. At execution time, the scheduler calls `execute_transfer` which performs the actual transfer and cleans up storage

NOTE: Failed transfers are not retried in this version.

### Delay Policy

High-security accounts **must** use the pallet's transfer extrinsics. Direct balance transfers are blocked by the `ReversibleTransactionExtension` which validates that high-security accounts only call whitelisted operations.

### Tracking

Pending/delayed transfers can be tracked via:
- `PendingTransfers` storage (by `tx_id`)
- `PendingTransfersBySender` storage (list of `tx_id`s per sender)
- `TransactionScheduled` event

### Storage Items

| Storage | Description |
|---------|-------------|
| `HighSecurityAccounts` | Maps account to `HighSecurityAccountData { guardian, delay }`. Accounts call `set_high_security` to join. |
| `PendingTransfers` | Maps `tx_id` to `PendingTransfer { from, to, guardian, asset_id, amount }`. |
| `PendingTransfersBySender` | Maps sender to list of their pending `tx_id`s (bounded by `MaxPendingPerAccount`). |
| `GuardianIndex` | Maps guardian to list of accounts they protect (bounded by `MaxGuardianAccounts`). |
| `NextTransactionId` | Monotonic counter for unique `tx_id` generation. |

### Transaction ID

Transaction ID is computed as `hash((who, call, NextTransactionId))` where:
- `who` is the sender account
- `call` is the transfer call being scheduled  
- `NextTransactionId` is a monotonically increasing counter

This ensures unique IDs even for identical transfers.

## High-Security Mode

### Overview

High-security mode provides enhanced protection for accounts by requiring all outgoing transfers to go through a time-delayed, cancellable process with guardian oversight.

### Permanence (By Design)

**Enabling high-security mode is permanent and irreversible.** There is no extrinsic to disable or downgrade a high-security account back to a normal account.

This is an intentional security design, not a limitation:

| Design Goal | Rationale |
|-------------|-----------|
| **Attacker resistance** | An attacker who compromises account keys cannot disable protections to steal funds immediately |
| **Social engineering defense** | Users cannot be tricked into disabling security during a scam |
| **Consistent security model** | Guardian can always rely on the delay period being enforced |
| **Regulatory clarity** | Compliance processes can depend on the immutable security configuration |

### Allowed Operations for High-Security Accounts

Once an account becomes high-security, it can **only** perform these operations:

| Operation | Description |
|-----------|-------------|
| `schedule_transfer` | Schedule a delayed native token transfer |
| `schedule_asset_transfer` | Schedule a delayed asset transfer |
| `cancel` | Cancel a pending transfer (owner or guardian) |
| `recover_funds` | Guardian-initiated emergency recovery of all funds |

All other blockchain operations (staking, governance, contract calls, etc.) are blocked by the transaction extension whitelist.

### Exiting High-Security Mode

While accounts cannot disable high-security mode, users who no longer want the high-security restrictions can simply transfer their funds to a different account using `schedule_transfer` or `schedule_asset_transfer`. After the delay period, the funds will be available in a normal account without restrictions.

This ensures that users always have a straightforward path to unrestricted funds, while attackers cannot bypass the delay period and guardian oversight.

## High-Security Integration

The **HighSecurityInspector** trait enables integration of high-security features with other pallets (like `pallet-multisig`) and transaction extensions.

### Trait Location

The trait is defined in the **`qp-high-security`** primitives crate (`primitives/high-security/`), not in this pallet. This separation allows:
- Runtime-level implementation with access to `RuntimeCall` for whitelist pattern matching
- Consumption by multiple pallets without circular dependencies
- Clean separation between storage (this pallet) and inspection interface (primitives)

### HighSecurityInspector Trait

```rust
// Defined in qp-high-security crate
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

### Implementation Pattern

**This pallet provides helper functions:**
```rust
impl<T: Config> Pallet<T> {
    /// Check if account is registered as high-security
    pub fn is_high_security_account(who: &T::AccountId) -> bool;
    
    /// Get guardian for high-security account
    pub fn get_guardian(who: &T::AccountId) -> Option<T::AccountId>;
}
```

**Runtime implements the trait** (in `runtime/src/configs/mod.rs`):
```rust
pub struct HighSecurityConfig;

impl qp_high_security::HighSecurityInspector<AccountId, RuntimeCall> for HighSecurityConfig {
    fn is_high_security(who: &AccountId) -> bool {
        // Delegate to pallet helper
        ReversibleTransfers::is_high_security_account(who)
    }

    fn is_whitelisted(call: &RuntimeCall) -> bool {
        // Runtime implements pattern matching (has RuntimeCall access)
        matches!(
            call,
            RuntimeCall::ReversibleTransfers(
                pallet_reversible_transfers::Call::schedule_transfer { .. } |
                pallet_reversible_transfers::Call::schedule_asset_transfer { .. } |
                pallet_reversible_transfers::Call::cancel { .. } |
                // ... other whitelisted calls
            )
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

// In multisig as_multi():
if T::HighSecurity::is_high_security(&multisig_address) {
    ensure!(
        T::HighSecurity::is_whitelisted(&call),
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
- Trait defined in primitives crate (shared dependency)
- Storage and helpers in this pallet
- Implementation in runtime (has `RuntimeCall` access)
- Consumers use trait without coupling to implementation

**Reusability:**
- Same security model for EOAs and multisigs
- Consistent whitelist enforcement across all account types
- Easy to add new consumers (just use the trait)

### Documentation

See `MULTISIG_REQ.md` for complete high-security integration architecture and examples.
