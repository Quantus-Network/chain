# Wormhole Soundness Bug Detection Plan

## Overview

This document describes a mechanism to detect potential soundness bugs in the wormhole system (ZK proof-of-burn minting). The core invariant we want to enforce:

```
total_wormhole_exits <= potential_wormhole_balance
```

Where:
- `potential_wormhole_balance` = sum of balances held by "ambiguous" addresses (addresses that have never signed a dilithium transaction)
- `total_wormhole_exits` = sum of all tokens minted via wormhole exits

If this invariant is ever violated, it indicates a soundness bug — more tokens are being exited than could possibly have been deposited into wormhole addresses.

## Key Insight: Nonce-Based Address Classification

On-chain, wormhole addresses are indistinguishable from regular dilithium addresses. However, we can use a heuristic:

- **Ambiguous address**: `account_nonce == 0` (has never signed a transaction)
- **Revealed address**: `account_nonce > 0` (has signed at least one transaction with dilithium)

When an address signs its first transaction, it "reveals" itself as a regular dilithium address (not a wormhole address), and its balance should be subtracted from the potential wormhole pool.

### Accounts that never reveal

The `nonce == 0` heuristic over-counts two kinds of accounts that have a zero nonce but are known *not* to be wormhole deposits. These are excluded via the `NonWormholeAccounts: Contains<AccountId>` config (so they never add to the pool), and the runtime populates it as follows:

- **Multisig accounts** spend through their signatories, so the multisig account itself never signs and never reveals. The runtime excludes any address registered in `pallet_multisig` (`is_multisig`). Because funds can be sent to a *pre-computed* multisig address before it is created, the multisig pallet also calls `TransferProofRecorder::reveal_address` on creation, which subtracts the address's balance from the pool (the multisig analog of a normal account's first-signature reveal). Together these make a multisig net zero into the pool over its lifetime.
- **Keyless accounts** that can never sign at all: the treasury account (a keyless governance account that receives a block reward every block), the `PalletId`-derived pallet accounts, and the sentinel minting addresses used as the `from` side of minted transfers.

## Storage Items

Add to `pallets/wormhole/src/lib.rs`:

```rust
/// Sum of balances held by ambiguous addresses (nonce == 0, might be wormhole).
/// This represents the maximum possible value that could be legitimately exited.
#[pallet::storage]
pub type PotentialWormholeBalance<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

/// Total value of all successful wormhole exits (mints to exit accounts).
#[pallet::storage]
pub type TotalWormholeExits<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;
```

## Implementation

### 1. Track Deposits on Every Transfer

Location: `record_transfer()` in `pallets/wormhole/src/lib.rs` (around line 603)

After recording the transfer in the ZK tree, check if the recipient is ambiguous:

```rust
pub fn record_transfer(
    asset_id: T::AssetId,
    from: &<T as Config>::WormholeAccountId,
    to: &<T as Config>::WormholeAccountId,
    amount: BalanceOf<T>,
) {
    // ... existing ZK tree recording code ...

    // Soundness tracking: if recipient has never signed (nonce == 0),
    // they might be a wormhole address, so add to potential balance
    if asset_id == T::AssetId::default() {
        // Native token only for now
        let to_account: <T as frame_system::Config>::AccountId = to.clone().into();
        let recipient_nonce = frame_system::Pallet::<T>::account_nonce(&to_account);
        
        if recipient_nonce.is_zero() {
            PotentialWormholeBalance::<T>::mutate(|total| {
                *total = total.saturating_add(amount);
            });
        }
    }
    
    // TODO: Add similar tracking for asset transfers when asset wormhole is enabled

    // ... existing event emission code ...
}
```

### 2. Subtract Balance on First Signature (Reveal)

Location: `WormholeProofRecorderExtension::validate()` in `runtime/src/transaction_extensions.rs` (around line 231)

When an address signs its first transaction, subtract its balance from the potential pool:

```rust
fn validate(
    &self,
    origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
    _call: &RuntimeCall,
    _info: &DispatchInfoOf<RuntimeCall>,
    _len: usize,
    _self_implicit: Self::Implicit,
    _inherited_implication: &impl sp_runtime::traits::Implication,
    _source: frame_support::pallet_prelude::TransactionSource,
) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
    // Soundness tracking: detect first-time signers and subtract their balance
    // Note: We're in validate(), so nonce has NOT been incremented yet by CheckNonce
    if let Ok(signer) = frame_system::ensure_signed(origin.clone()) {
        let nonce = frame_system::Pallet::<Runtime>::account_nonce(&signer);
        
        if nonce.is_zero() {
            // First transaction from this address — they're revealing as dilithium signer
            // Subtract their current balance from potential wormhole balance
            let balance = <Runtime as pallet_wormhole::Config>::Currency::free_balance(&signer);
            
            pallet_wormhole::PotentialWormholeBalance::<Runtime>::mutate(|total| {
                *total = total.saturating_sub(balance);
            });
        }
    }
    
    Ok((ValidTransaction::default(), (), origin))
}
```

**Important timing note:** This runs in `validate()`, which executes *before* `CheckNonce::prepare()` increments the nonce. So `nonce == 0` correctly identifies first-time signers.

### 3. Enforce Invariant on Wormhole Exit

Location: `verify_private_batch()` in `pallets/wormhole/src/lib.rs` (around line 394, after computing `total_exit_amount`)

Add the soundness check before processing exits:

```rust
// After computing total_exit_amount (around line 394-396):

// SOUNDNESS CHECK: Verify invariant before allowing exit
let potential_balance = PotentialWormholeBalance::<T>::get();
let current_exits = TotalWormholeExits::<T>::get();
let exits_after = current_exits.saturating_add(total_exit_amount);

ensure!(
    exits_after <= potential_balance,
    Error::<T>::SoundnessInvariantViolation
);

// ... existing exit processing (minting, fee handling, etc.) ...

// After successful exit processing (near end of function, before Ok):
TotalWormholeExits::<T>::put(exits_after);
```

### 4. Add Error Variant

Location: `pallets/wormhole/src/lib.rs`, in the `Error` enum (around line 264)

```rust
#[pallet::error]
pub enum Error<T> {
    // ... existing errors ...
    
    /// Soundness invariant violated: total exits would exceed potential wormhole deposits.
    /// This indicates a potential bug in the ZK proof system.
    SoundnessInvariantViolation,
}
```

## Status: IMPLEMENTED

This plan has been implemented. A few decisions were made during implementation that refine
the original plan; they are documented inline below.

### Seeding an already-running testnet (the key ambiguity)

The original plan only handled fresh chains (where genesis endowments seed the pool at block 1).
On a chain that is **already running**, wormhole deposits made before this tracking existed are
not reflected in `PotentialWormholeBalance`, which would default to `0`. The first legitimate
exit would then trip `SoundnessInvariantViolation` and effectively brick the wormhole.

To fix this we added a versioned storage migration (`migrations::v1::InitSoundnessCounters`, wired
as `MigrateV0ToV1` in the runtime `Migrations` tuple) that runs once on the v0 -> v1 upgrade and
seeds:

```
PotentialWormholeBalance = total_issuance()
```

Every balance held by an ambiguous address is necessarily backed by issued tokens, so
`total_issuance()` is an upper bound on the value that could legitimately be exited. Seeding to it
therefore guarantees the upgrade can never brick a valid exit (including value held by un-revealed
miner-reward accounts). As accounts reveal themselves, the counter tightens toward the true
ambiguous sum. We deliberately avoid an extra configurable buffer: it could only ever loosen the
invariant (total issuance is already a safe ceiling), so it would add no safety and reduce
sensitivity.

The pallet now declares `STORAGE_VERSION = 1`, and `VersionedMigration<0, 1, ...>` runs the seed
only when the on-chain version is 0, then bumps it to 1 (so it executes exactly once). On a fresh
chain the migration is skipped (genesis sets the on-chain version to 1) and the pool is seeded by
the block-1 `record_transfer` calls for genesis endowments instead.

## Edge Cases

### Genesis Endowments

Genesis addresses that receive funds via `GenesisConfig` will have `nonce == 0`, so their balances are correctly counted in `PotentialWormholeBalance`. If they later sign a transaction, they'll be subtracted.

The existing `on_initialize` hook at block 1 calls `record_transfer` for genesis endowments, which will add to `PotentialWormholeBalance`.

### Exit to Revealed Address

When someone exits from wormhole to a revealed address (nonce > 0):
- The exit minting increases `TotalWormholeExits`
- The `record_transfer` call for the exit does NOT add to `PotentialWormholeBalance` (recipient nonce > 0)
- This is correct — tokens exited to a revealed address can't be re-exited via wormhole

### Fees

The current implementation mints `exit_amount` (post-fee) to users. Fees are partially burned and partially paid to miners. The invariant tracks:
- Deposits: full amount transferred to ambiguous addresses
- Exits: amount minted to users (post-fee)

So `exits < deposits` is expected (the gap is fees). The invariant `exits <= deposits` should always hold unless there's a soundness bug.

### Unsigned Transactions

Wormhole exits are unsigned (`ensure_none`). They don't have a signer and won't trigger the reveal logic. This is correct — exits don't reveal anything about the exit destination address.

## Files Modified

1. **`pallets/wormhole/src/lib.rs`**
   - Bumped `STORAGE_VERSION` to 1 (`#[pallet::storage_version]`)
   - Added `PotentialWormholeBalance` and `TotalWormholeExits` storage items (with getters)
   - Added `is_ambiguous_account()` helper (single source of truth for the `nonce == 0` heuristic), now also excluding `NonWormholeAccounts`
   - Added `NonWormholeAccounts: Contains<AccountId>` config and `reveal_account()` helper (the deduction side, shared by the first-signature reveal and multisig creation)
   - Added `SoundnessInvariantViolation` error variant
   - Updated `record_transfer()` to track deposits to ambiguous addresses
   - Updated `verify_private_batch()` to check the invariant and update `TotalWormholeExits`

2. **`pallets/wormhole/src/migrations.rs`** (new)
   - `v1::InitSoundnessCounters` + `MigrateV0ToV1` versioned migration to seed the pool

3. **`primitives/wormhole/src/lib.rs`**
   - Added `reveal_address()` to the `TransferProofRecorder` trait (the cross-pallet wormhole hook), implemented by the wormhole pallet

4. **`pallets/multisig/src/lib.rs`**
   - Added `is_multisig()` helper (registry lookup)
   - Added a `ProofRecorder: TransferProofRecorder` config and a `reveal_address` call on multisig creation, so a multisig is revealed to the soundness counter when created

5. **`runtime/src/transaction_extensions.rs`**
   - Updated `WormholeProofRecorderExtension::validate()` to detect first-time signers and reveal them (and accounted for it in `weight()`)

6. **`runtime/src/configs/mod.rs`**
   - Implemented `NonWormholeAccounts` (registered multisigs + treasury + `PalletId`/sentinel keyless accounts) and wired multisig's `ProofRecorder = Wormhole`

7. **`runtime/src/lib.rs`**
   - Added the `Migrations` tuple (`MigrateV0ToV1`) to `Executive` and bumped `spec_version` to 132

## Testing

1. **Unit tests in wormhole pallet:**
   - Test that transfers to nonce-0 addresses increment `PotentialWormholeBalance`
   - Test that transfers to nonce>0 addresses don't affect `PotentialWormholeBalance`
   - Test that transfers to `NonWormholeAccounts` don't affect `PotentialWormholeBalance`
   - Test that `reveal_account()` subtracts an account's balance from the pool
   - Test that exits increment `TotalWormholeExits`
   - Test that exits fail with `SoundnessInvariantViolation` when invariant would be violated

2. **Multisig pallet test:**
   - Test that creating a multisig reveals its address to the wormhole soundness counter

3. **Integration tests in runtime:**
   - Test that signing a first transaction subtracts balance from `PotentialWormholeBalance`
   - Test that creating a multisig deducts a pre-funded (pre-computed) address's balance and excludes it afterward
   - Test that mining rewards count only the miner's (ambiguous) portion, not the excluded treasury portion
   - Test full flow: deposit -> exit -> verify counters
   - Test reveal flow: deposit -> sign transaction -> verify balance subtracted

## Future Considerations

- **Asset support:** When asset wormhole is enabled, add similar tracking for asset balances. Comments are left in the code indicating where to add this.
- **Monitoring:** Consider adding RPC methods to query `PotentialWormholeBalance` and `TotalWormholeExits` for external monitoring dashboards.
- **Graceful degradation:** If a false positive ever occurs (e.g., due to a bug in the tracking itself), consider adding a sudo call to adjust the counters or temporarily disable the check.
