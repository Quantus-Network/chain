### Motivation

To have accounts for which all outgoing transfer are subject to a variable time during which they may be cancelled. The idea is this could be used to deter theft as well as correct mistakes.

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
