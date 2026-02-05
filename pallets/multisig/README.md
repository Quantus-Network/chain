# Multisig Pallet

A multisignature wallet pallet for the Quantus blockchain with an economic security model.

## Overview

This pallet provides functionality for creating and managing multisig accounts that require multiple approvals before executing transactions. It implements a dual fee+deposit system for spam prevention and storage cleanup mechanisms with grace periods.

## Quick Start

Basic workflow for using a multisig:

```rust
// 1. Create a 2-of-3 multisig (Alice creates, Bob/Charlie/Dave are signers)
Multisig::create_multisig(Origin::signed(alice), vec![bob, charlie, dave], 2, 0);
//                                                                        ^ threshold ^ nonce
let multisig_addr = Multisig::derive_multisig_address(&[bob, charlie, dave], 2, 0);
//                                                                           ^ threshold ^ nonce

// 2. Bob proposes a transaction
let call = RuntimeCall::Balances(pallet_balances::Call::transfer { dest: eve, value: 100 });
Multisig::propose(Origin::signed(bob), multisig_addr, call.encode(), expiry_block);

// 3. Charlie approves - transaction executes automatically (2/2 threshold reached)
Multisig::approve(Origin::signed(charlie), multisig_addr, proposal_id);
// ✅ Transaction executed! No separate call needed.
```

**Key Point:** Once the threshold is reached, the transaction is **automatically executed**. 
There is no separate `execute()` call exposed to users.

## Core Functionality

### 1. Create Multisig
Creates a new multisig account with deterministic address generation.

**Required Parameters:**
- `signers: Vec<AccountId>` - List of authorized signers (REQUIRED, 1 to MaxSigners)
- `threshold: u32` - Number of approvals needed (REQUIRED, 1 ≤ threshold ≤ signers.len())
- `nonce: u64` - User-provided nonce for address uniqueness (REQUIRED)

**Validation:**
- No duplicate signers
- Threshold must be > 0
- Threshold cannot exceed number of signers
- Signers count must be ≤ MaxSigners
- Multisig address (derived from signers+threshold+nonce) must not already exist

**Important:** Signers are automatically sorted before storing and address generation. Order doesn't matter:
- `[alice, bob, charlie]` + threshold=2 + nonce=0 → `address_1`
- `[charlie, bob, alice]` + threshold=2 + nonce=0 → `address_1` (same!)
- To create multiple multisigs with same signers, use different nonce:
  - `signers=[alice, bob], threshold=2, nonce=0` → `address_A`
  - `signers=[alice, bob], threshold=2, nonce=1` → `address_B` (different!)

**Economic Costs:**
- **MultisigFee**: Non-refundable fee (spam prevention) → burned immediately
- **MultisigDeposit**: Reserved deposit (storage bond) → returned to creator when multisig dissolved

### 2. Propose Transaction
Creates a new proposal for multisig execution.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig account (REQUIRED)
- `call: Vec<u8>` - Encoded RuntimeCall to execute (REQUIRED, max MaxCallSize bytes)
- `expiry: BlockNumber` - Deadline for collecting approvals (REQUIRED)

**Validation:**
- Caller must be a signer
- **High-Security Check:** If multisig is high-security, only whitelisted calls are allowed (see High-Security Integration section)
- Call size must be ≤ MaxCallSize
- Multisig cannot have MaxTotalProposalsInStorage or more total proposals in storage
- Caller cannot exceed their per-signer proposal limit (`MaxTotalProposalsInStorage / signers_count`)
- Expiry must be in the future (expiry > current_block)
- Expiry must not exceed MaxExpiryDuration blocks from now (expiry ≤ current_block + MaxExpiryDuration)

**Auto-Cleanup Before Creation:**
Before creating a new proposal, the system **automatically removes all proposer's expired Active proposals**:
- Only proposer's expired proposals are cleaned (not all proposals)
- Expired proposals are identified (current_block > expiry)
- Deposits are returned to original proposer
- Storage is cleaned up
- Counters are decremented (active_proposals, proposals_per_signer)
- Events are emitted for each removed proposal

This ensures proposers get their deposits back and free up their quota automatically.

**Threshold=1 Auto-Execution:**
If the multisig has `threshold=1`, the proposal **executes immediately** after creation:
- Proposer's approval counts as the first (and only required) approval
- Call is dispatched automatically
- Proposal is removed from storage immediately
- Deposit is returned to proposer immediately
- No separate `approve()` call needed

**Economic Costs:**
- **ProposalFee**: Non-refundable fee (spam prevention, scaled by signer count) → burned
- **ProposalDeposit**: Refundable deposit (storage rent) → returned when proposal removed

**Important:** Fee is ALWAYS paid, even if proposal expires or is cancelled. Only deposit is refundable.

### 3. Approve Transaction
Adds caller's approval to an existing proposal. **If this approval brings the total approvals 
to or above the threshold, the transaction will be automatically executed and immediately removed from storage.**

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the proposal to approve (REQUIRED)

**Validation:**
- Caller must be a signer
- Proposal must exist
- Proposal must not be expired (current_block ≤ expiry)
- Caller must not have already approved

**Auto-Execution:**
When approval count reaches the threshold:
- Encoded call is executed as multisig_address origin
- Proposal **immediately removed** from storage
- ProposalDeposit **immediately returned** to proposer
- TransactionExecuted event emitted with execution result

**Economic Costs:** None (deposit immediately returned on execution)

**Note:** `approve()` does NOT perform auto-cleanup of expired proposals (removed for predictable gas costs).

### 4. Cancel Transaction
Cancels a proposal and immediately removes it from storage (proposer only).

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the proposal to cancel (REQUIRED)

**Validation:**
- Caller must be the proposer
- Proposal must exist and be Active

**Economic Effects:**
- Proposal **immediately removed** from storage
- ProposalDeposit **immediately returned** to proposer
- Counters decremented (active_proposals, proposals_per_signer)

**Economic Costs:** None (deposit immediately returned)

**Note:** 
- ProposalFee is NOT refunded - it was burned at proposal creation.
- `cancel()` does NOT perform auto-cleanup of expired proposals (removed for predictable gas costs).

### 5. Remove Expired
Manually removes expired proposals from storage. Only signers can call this.

**Important:** This is rarely needed because proposer's expired proposals are automatically cleaned up when that proposer calls `propose()` or `claim_deposits()`.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the expired proposal (REQUIRED)

**Validation:**
- Caller must be a signer of the multisig
- Proposal must exist and be Active
- Must be expired (current_block > expiry)

**Note:** Executed/Cancelled proposals are automatically removed immediately, so this only applies to Active+Expired proposals.

**Economic Effects:**
- ProposalDeposit returned to **original proposer** (not caller)
- Proposal removed from storage
- Counters decremented (active_proposals, proposals_per_signer)

**Economic Costs:** None (deposit always returned to proposer)

**Auto-Cleanup:** When a proposer calls `propose()`, all their expired proposals are automatically removed. This function is useful for cleaning up proposals from inactive proposers.

### 6. Claim Deposits
Batch cleanup operation to recover all caller's expired proposal deposits.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)

**Validation:**
- Only cleans proposals where caller is proposer
- Only removes Active+Expired proposals (Executed/Cancelled already auto-removed)
- Must be expired (current_block > expiry)

**Behavior:**
- Iterates through ALL proposals in the multisig
- Removes all that match: proposer=caller AND expired AND status=Active
- No iteration limits - cleans all in one call

**Economic Effects:**
- Returns all eligible proposal deposits to caller
- Removes all expired proposals from storage
- Counters decremented (active_proposals, proposals_per_signer)

**Economic Costs:** 
- Gas cost proportional to total proposals in storage (iteration cost)
- Dynamic weight refund based on actual proposals cleaned

**Note:** Same functionality as the auto-cleanup in `propose()`, but caller can trigger it manually without creating a new proposal.

### 7. Approve Dissolve
Approve dissolving a multisig account. Requires threshold approvals to complete.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)

**Pre-conditions:**
- Caller must be a signer
- NO proposals can exist (any status)
- Multisig balance MUST be zero

**Approval Process:**
- Each signer calls `approve_dissolve()`
- Approvals are tracked in `DissolveApprovals` storage
- When threshold reached, multisig is automatically dissolved

**Post-conditions (when threshold reached):**
- MultisigDeposit is **returned to creator**
- Multisig removed from storage
- DissolveApprovals cleared
- Cannot be used after dissolution

**Economic Costs:** None (deposit returned to creator)

**Important:** 
- MultisigFee is NEVER returned (burned on creation)
- MultisigDeposit IS returned to the original creator
- Requires threshold approvals (not just any signer or creator)

## Use Cases

**Payroll Multisig (transfers only):**
```rust
// Only allow keep_alive transfers to prevent account deletion
matches!(call, RuntimeCall::Balances(Call::transfer_keep_alive { .. }))
```

**Treasury Multisig (governance + transfers):**
```rust
matches!(call,
    RuntimeCall::Balances(Call::transfer_keep_alive { .. }) |
    RuntimeCall::Scheduler(Call::schedule { .. }) |  // Time-locked ops
    RuntimeCall::Democracy(Call::veto { .. })        // Emergency stops
)
```

## Economic Model

### Fees (Non-refundable, burned)
**Purpose:** Spam prevention and deflationary pressure

- **MultisigFee**:
  - Charged on multisig creation
  - Burned immediately (reduces total supply)
  - **Never returned** (even if multisig dissolved)
  - Creates economic barrier to prevent spam multisig creation
  
- **ProposalFee**:
  - Charged on proposal creation
  - **Dynamically scaled** by signer count: `BaseFee × (1 + SignerCount × StepFactor)`
  - Burned immediately (reduces total supply)
  - **Never returned** (even if proposal expires or is cancelled)
  - Makes spam expensive, scales cost with multisig complexity
  
**Why burned (not sent to treasury)?**
- Creates deflationary pressure on token supply
- Simpler implementation (no treasury dependency)
- Spam attacks reduce circulating supply
- Lower transaction costs (withdraw vs transfer)

### Deposits (Locked as storage rent)
**Purpose:** Compensate for on-chain storage, incentivize cleanup

- **MultisigDeposit**:
  - Reserved on multisig creation
  - **Burned** when multisig dissolved (via `approve_dissolve`)
  - Locked until no proposals exist and balance is zero
  - Opportunity cost incentivizes cleanup
  - **NOT refundable** (acts as permanent storage bond)
  
- **ProposalDeposit**:
  - Reserved on proposal creation
  - **Refundable** - returned in following scenarios:
  - **Auto-Returned Immediately:**
    - When proposal executed (threshold reached)
    - When proposal cancelled (proposer cancels)
  - **Auto-Cleanup:** Proposer's expired proposals are automatically removed when proposer calls `propose()`
    - Only proposer's proposals are cleaned (not all)
    - Deposits returned to proposer
    - Frees up proposer's quota automatically
  - **Manual Cleanup:** For inactive proposers via `remove_expired()` or `claim_deposits()`

### Storage Limits & Configuration
**Purpose:** Prevent unbounded storage growth and resource exhaustion

- **MaxSigners**: Maximum signers per multisig
  - Trade-off: Higher → more flexible governance, more computation per approval
  
- **MaxTotalProposalsInStorage**: Maximum total proposals (Active + Executed + Cancelled)
  - Trade-off: Higher → more flexible, more storage risk
  - Forces periodic cleanup to continue operating
  - **Auto-cleanup**: Expired proposals are automatically removed when new proposals are created
  - **Per-Signer Limit**: Each signer gets `MaxTotalProposalsInStorage / signers_count` quota
    - Prevents single signer from monopolizing storage (filibuster protection)
    - Fair allocation ensures all signers can participate
    - Example: 20 total, 5 signers → 4 proposals max per signer
  
- **MaxCallSize**: Maximum encoded call size in bytes
  - Trade-off: Larger → more flexibility, more storage per proposal
  - Should accommodate common operations (transfers, staking, governance)
  
- **MaxExpiryDuration**: Maximum blocks in the future for proposal expiry
  - Trade-off: Shorter → faster turnover, may not suit slow decision-making
  - Prevents infinite-duration deposit locks
  - Should exceed typical multisig decision timeframes

**Configuration values are runtime-specific.** See runtime config for production values.

## Storage

### Multisigs: Map<AccountId, MultisigData>
Stores multisig account data:
```rust
MultisigData {
    creator: AccountId,                                     // Original creator (receives deposit back on dissolve)
    signers: BoundedVec<AccountId>,                        // List of authorized signers (sorted)
    threshold: u32,                                         // Required approvals
    proposal_nonce: u32,                                    // Counter for unique proposal IDs
    deposit: Balance,                                       // Reserved deposit (returned to creator on dissolve)
    active_proposals: u32,                                  // Count of active proposals (for limits)
    proposals_per_signer: BoundedBTreeMap<AccountId, u32>,  // Per-signer proposal count (filibuster protection)
}
```

**Note:** Address is deterministically derived from `hash(pallet_id || sorted_signers || threshold || nonce)` where nonce is user-provided at creation time.

### Proposals: DoubleMap<AccountId, u32, ProposalData>
Stores proposal data indexed by (multisig_address, proposal_id):
```rust
ProposalData {
    proposer: AccountId,                // Who proposed (receives deposit back)
    call: BoundedVec<u8>,               // Encoded RuntimeCall to execute
    expiry: BlockNumber,                // Deadline for approvals
    approvals: BoundedVec<AccountId>,   // List of signers who approved
    deposit: Balance,                   // Reserved deposit (refundable)
    status: ProposalStatus,             // Active only (Executed/Cancelled are removed immediately)
}
```

**Important:** Only **Active** proposals are stored. Executed and Cancelled proposals are **immediately removed** from storage and their deposits are returned. Historical data is available through events (see Historical Data section below).

### DissolveApprovals: Map<AccountId, BoundedVec<AccountId>>
Tracks which signers have approved dissolving each multisig.
- Key: Multisig address
- Value: List of signers who approved dissolution
- Cleared when multisig is dissolved or when threshold reached

## Events

- `MultisigCreated { creator, multisig_address, signers, threshold, nonce }`
- `ProposalCreated { multisig_address, proposer, proposal_id }`
- `ProposalApproved { multisig_address, approver, proposal_id, approvals_count }`
- `ProposalExecuted { multisig_address, proposal_id, proposer, call, approvers, result }`
- `ProposalCancelled { multisig_address, proposer, proposal_id }`
- `ProposalRemoved { multisig_address, proposal_id, proposer, removed_by }`
- `DepositsClaimed { multisig_address, claimer, total_returned, proposals_removed, multisig_removed }`
- `DissolveApproved { multisig_address, approver, approvals_count }`
- `MultisigDissolved { multisig_address, deposit_returned, approvers }`

## Errors

- `NotEnoughSigners` - Less than 1 signer provided
- `ThresholdZero` - Threshold cannot be 0
- `ThresholdTooHigh` - Threshold exceeds number of signers
- `TooManySigners` - Exceeds MaxSigners limit
- `DuplicateSigner` - Duplicate address in signers list
- `MultisigAlreadyExists` - Multisig with this address already exists
- `MultisigNotFound` - Multisig does not exist
- `NotASigner` - Caller is not authorized signer
- `ProposalNotFound` - Proposal does not exist
- `NotProposer` - Caller is not the proposer (for cancel)
- `AlreadyApproved` - Signer already approved this proposal
- `NotEnoughApprovals` - Threshold not met (internal error, should not occur)
- `ExpiryInPast` - Proposal expiry is not in the future (for propose)
- `ExpiryTooFar` - Proposal expiry exceeds MaxExpiryDuration (for propose)
- `ProposalExpired` - Proposal deadline passed (for approve)
- `CallTooLarge` - Encoded call exceeds MaxCallSize
- `InvalidCall` - Call decoding failed during execution
- `InsufficientBalance` - Not enough funds for fee/deposit
- `TooManyProposalsInStorage` - Multisig has MaxTotalProposalsInStorage total proposals (cleanup required to create new)
- `TooManyProposalsPerSigner` - Caller has reached their per-signer proposal limit (`MaxTotalProposalsInStorage / signers_count`)
- `ProposalNotExpired` - Proposal not yet expired (for remove_expired)
- `ProposalNotActive` - Proposal is not active (already executed or cancelled)
- `ProposalsExist` - Cannot dissolve multisig while proposals exist
- `MultisigAccountNotZero` - Cannot dissolve multisig with non-zero balance

## Important Behavior

### Simple Proposal IDs (Not Hashes)
Proposals are identified by a simple **nonce (u32)** instead of a hash:
- **More efficient:** 4 bytes instead of 32 bytes (Blake2_256 hash)
- **Simpler:** No need to hash `(call, nonce)`, just use nonce directly
- **Better UX:** Sequential IDs (0, 1, 2...) easier to read than random hashes
- **Easier queries:** Can iterate proposals by ID without needing call data

**Example:**
```rust
propose(...) // → proposal_id: 0
propose(...) // → proposal_id: 1
propose(...) // → proposal_id: 2

// Approve by ID (not hash)
approve(multisig, 1) // Approve proposal #1
```

### Signer Order Doesn't Matter
Signers are **automatically sorted** before address generation and storage:
- Input order is irrelevant - signers are always sorted deterministically
- Address is derived from `Hash(PalletId + sorted_signers + threshold + nonce)`
- Same signers+threshold+nonce in any order = same multisig address
- User must provide unique nonce to create multiple multisigs with same signers

**Example:**
```rust
// These create the SAME multisig address (same signers, threshold, nonce):
create_multisig([alice, bob, charlie], 2, 0) // → multisig_addr_1
create_multisig([charlie, bob, alice], 2, 0) // → multisig_addr_1 (SAME!)

// To create another multisig with same signers, use different nonce:
create_multisig([alice, bob, charlie], 2, 1) // → multisig_addr_2 (different!)

// Different threshold = different address (even with same nonce):
create_multisig([alice, bob, charlie], 3, 0) // → multisig_addr_3 (different!)
```

## Historical Data and Event Indexing

The pallet does **not** maintain on-chain storage of executed proposal history. Instead, all historical data is available through **blockchain events**, which are designed to be efficiently indexed by off-chain indexers like **SubSquid**.

### ProposalExecuted Event

When a proposal is successfully executed, the pallet emits a comprehensive `ProposalExecuted` event containing all relevant data:

```rust
Event::ProposalExecuted {
    multisig_address: T::AccountId,   // The multisig that executed
    proposal_id: u32,                  // ID (nonce) of the proposal
    proposer: T::AccountId,            // Who originally proposed it
    call: Vec<u8>,                     // The encoded call that was executed
    approvers: Vec<T::AccountId>,      // All accounts that approved
    result: DispatchResult,            // Whether execution succeeded or failed
}
```

### Indexing with SubSquid

This event structure is optimized for indexing by SubSquid and similar indexers:
- **Complete data**: All information needed to reconstruct the full proposal history
- **Queryable**: Indexers can efficiently query by multisig address, proposer, approvers, etc.
- **Execution result**: Both successful and failed executions are recorded
- **No storage bloat**: Events don't consume on-chain storage long-term

**All events** for complete history:
- `MultisigCreated` - When a multisig is created
- `ProposalCreated` - When a proposal is submitted
- `ProposalApproved` - Each time someone approves (includes current approval count)
- `ProposalExecuted` - When a proposal is executed (includes full execution details)
- `ProposalCancelled` - When a proposal is cancelled by proposer
- `ProposalRemoved` - When a proposal is removed from storage (deposits returned)
- `DepositsClaimed` - Batch removal of multiple proposals

### Benefits of Event-Based History

- ✅ **No storage costs**: Events don't occupy chain storage after archival
- ✅ **Complete history**: All actions are recorded permanently in events
- ✅ **Efficient querying**: Off-chain indexers provide fast, flexible queries
- ✅ **No DoS risk**: No on-chain iteration over unbounded storage
- ✅ **Standard practice**: Follows Substrate best practices for historical data

## Security Considerations

### Spam Prevention
- Fees (non-refundable, burned) prevent proposal spam
- Deposits (refundable) prevent storage bloat
- MaxTotalProposalsInStorage caps total storage per multisig
- Per-signer limits prevent single signer from monopolizing storage (filibuster protection)
- Auto-cleanup of expired proposals reduces storage pressure

### Storage Cleanup
- Auto-cleanup in `propose()`: proposer's expired proposals removed automatically
- Manual cleanup via `remove_expired()`: any signer can clean any expired proposal
- Batch cleanup via `claim_deposits()`: proposer recovers all their expired deposits at once

### Economic Attacks
- **Multisig Spam:** Costs MultisigFee (burned, reduces supply)
  - No refund even if never used
  - Economic barrier to creation spam
- **Proposal Spam:** Costs ProposalFee (burned, reduces supply) + ProposalDeposit (locked)
  - Fee never returned (even if expired/cancelled)
  - Deposit locked until cleanup
  - Cost scales with multisig size (dynamic pricing)
- **Filibuster Attack (Single Signer Monopolization):**
  - **Attack:** One signer tries to fill entire proposal queue
  - **Defense:** Per-signer limit caps each at `MaxTotalProposalsInStorage / signers_count`
  - **Effect:** Other signers retain their fair quota
  - **Cost:** Attacker still pays fees for their proposals (burned)
- **Result:** Spam attempts reduce circulating supply
- **No global limits:** Only per-multisig limits (decentralized resistance)

### Call Execution
- Calls execute with multisig_address as origin
- Multisig can call ANY pallet (including recursive multisig calls)
- Call validation happens at execution time
- Failed calls emit event with error but don't revert proposal removal

## Configuration Example


```rust
impl pallet_multisig::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    
    // Storage limits (prevent unbounded growth)
    type MaxSigners = ConstU32<100>;                    // Max complexity
    type MaxTotalProposalsInStorage = ConstU32<200>;    // Total storage cap (auto-cleanup on propose)
    type MaxCallSize = ConstU32<10240>;                 // Per-proposal storage limit
    type MaxExpiryDuration = ConstU32<100_800>;         // Max proposal lifetime (~2 weeks @ 12s)
    
    // Economic parameters (example values - adjust per runtime)
    type MultisigFee = ConstU128<{ 100 * MILLI_UNIT }>;      // Creation barrier (burned)
    type MultisigDeposit = ConstU128<{ 500 * MILLI_UNIT }>;  // Storage bond (returned to creator on dissolve)
    type ProposalFee = ConstU128<{ 1000 * MILLI_UNIT }>;     // Base proposal cost (burned)
    type ProposalDeposit = ConstU128<{ 1000 * MILLI_UNIT }>; // Storage rent (refundable)
    type SignerStepFactor = Permill::from_percent(1);        // Dynamic pricing (1% per signer)
    
    type PalletId = ConstPalletId(*b"py/mltsg");
    type WeightInfo = pallet_multisig::weights::SubstrateWeight<Runtime>;
}
```

**Parameter Selection Considerations:**
- **High-value chains:** Lower fees, higher deposits, tighter limits
- **Low-value chains:** Higher fees (maintain spam protection), lower deposits
- **Enterprise use:** Higher MaxSigners, longer MaxExpiryDuration
- **Public use:** Moderate limits, shorter expiry for faster turnover

## High-Security Integration

The multisig pallet integrates with **pallet-reversible-transfers** to support high-security multisigs with call whitelisting and delayed execution.

### Overview

**Standard Multisig:**
- Proposes any `RuntimeCall`
- Executes immediately on threshold
- No restrictions

**High-Security Multisig:**
- **Whitelist enforced:** Only allowed calls can be proposed
- **Delayed execution:** Via `ReversibleTransfers::schedule_transfer()`
- **Guardian oversight:** Guardian can cancel during delay period
- **Use case:** Corporate treasury, regulated operations, high-value custody

### ⚠️ Important: Enabling High-Security

**Risk Window:**
When enabling high-security for an existing multisig with active proposals:
1. **Existing proposals** are NOT automatically blocked
2. **Whitelist check** only happens at proposal creation time (`propose()`)
3. **Proposals created before HS** can still be executed after HS is enabled

**Mitigation:**
Before enabling high-security, ensure:
- ✅ All active proposals are **completed** (executed or cancelled)
- ✅ All proposals have **expired** or been **removed**
- ✅ No pending approvals exist

**Safe workflow:**
```rust
// 1. Check for active proposals
let proposals = query_proposals(multisig_address);
assert_eq!(proposals.len(), 0, "Must cleanup proposals first");

// 2. Cancel or wait for expiry
for proposal_id in proposals {
    Multisig::cancel(Origin::signed(proposer), multisig_address, proposal_id);
    // OR: wait for expiry
}

// 3. NOW enable high-security
ReversibleTransfers::set_high_security(
    Origin::signed(multisig_address),
    delay: 100_800,
    guardian: guardian_account
);
```

**Why this design:**
- **Simplicity:** Single check point (`propose`) easier to reason about
- **Gas efficiency:** No decode overhead on every approval
- **User control:** Explicit transition management
- **Trade-off:** Performance and simplicity over defense-in-depth

**Could be changed:**
Adding whitelist check in `approve()` (before execution) would close this window,
at the cost of:
- Higher gas on every approval for HS multisigs (~70M units for decode + check)
- More complex execution path
- Would make this a non-issue

### How It Works

1. **Setup:** Multisig account calls `ReversibleTransfers::set_high_security(delay, guardian)`
2. **Propose:** Only whitelisted calls allowed:
   - ✅ `ReversibleTransfers::schedule_transfer`
   - ✅ `ReversibleTransfers::schedule_asset_transfer`
   - ✅ `ReversibleTransfers::cancel`
   - ❌ All other calls → `CallNotAllowedForHighSecurityMultisig` error
3. **Approve:** Standard multisig approval process
4. **Execute:** Threshold reached → transfer scheduled with delay
5. **Guardian:** Can cancel via `ReversibleTransfers::cancel(tx_id)` during delay

### Code Example

```rust
// 1. Create standard 3-of-5 multisig
let multisig_addr = Multisig::create_multisig(
    Origin::signed(alice),
    vec![alice, bob, charlie, dave, eve],
    3,
    0 // nonce
);

// 2. Enable high-security (via multisig proposal + approvals)
// Propose and get 3 approvals for:
ReversibleTransfers::set_high_security(
    Origin::signed(multisig_addr),
    delay: 100_800, // 2 weeks @ 12s blocks
    guardian: guardian_account
);

// 3. Now only whitelisted calls work
// ✅ ALLOWED: Schedule delayed transfer
Multisig::propose(
    Origin::signed(alice),
    multisig_addr,
    RuntimeCall::ReversibleTransfers(
        Call::schedule_transfer { dest: recipient, amount: 1000 }
    ).encode(),
    expiry
);
// → Whitelist check passes
// → Collect approvals
// → Transfer scheduled with 2-week delay
// → Guardian can cancel if suspicious

// ❌ REJECTED: Direct transfer
Multisig::propose(
    Origin::signed(alice),
    multisig_addr,
    RuntimeCall::Balances(
        Call::transfer { dest: recipient, amount: 1000 }
    ).encode(),
    expiry
);
// → ERROR: CallNotAllowedForHighSecurityMultisig
// → Proposal fails immediately
```

### Performance Impact

High-security multisigs have higher costs due to call validation:

- **+1 DB read:** Check `ReversibleTransfers::HighSecurityAccounts`
- **+Decode overhead:** Variable cost based on call size (O(call_size))
- **+Whitelist check:** ~10k units for pattern matching
- **Total overhead:** Base cost + decode cost proportional to call size

**Dynamic weight refund:**
Normal multisigs automatically get refunded for unused high-security overhead.

**Weight calculation:**
- `propose()` charges upfront for worst-case: 
  - `propose_high_security(call.len(), MaxTotalProposalsInStorage, MaxTotalProposalsInStorage.saturating_div(2))`
  - Second parameter (`i`): worst-case proposals iterated (MaxTotal)
  - Third parameter (`r`): worst-case proposals removed/cleaned (MaxTotal/2, based on 2-signer minimum)
- Actual weight based on:
  - Call size (actual, not worst-case)
  - Proposals actually iterated during cleanup (`i`)
  - Proposals actually removed/cleaned (`r`)
- If multisig is NOT HS, refunds decode overhead based on actual path taken
- If multisig IS HS, charges correctly for decode cost (scales with call size)
- Auto-cleanup returns both iteration count AND cleaned count for accurate weight calculation

**Security notes:**
- Call size is validated BEFORE decode to prevent DoS via oversized payloads
- Weight formula includes O(call_size) component for decode to prevent underpayment
- **Separate charging for iteration cost (reads) vs cleanup cost (writes)**:
  - `i` parameter: proposals iterated (O(N) read cost)
  - `r` parameter: proposals removed (O(M) write cost, where M ≤ N)
- No refund for the iteration that actually happened (prevents undercharging attack)
- Single-pass optimization: cleanup counts proposals during iteration (no extra pass needed)
- Benchmarks must be regenerated to capture accurate decode costs

See `MULTISIG_REQ.md` for detailed cost breakdown and benchmarking instructions.

### Configuration

```rust
impl pallet_multisig::Config for Runtime {
    type HighSecurity = runtime::HighSecurityConfig;
    // ... other config
}

// Runtime implements HighSecurityInspector trait
// (trait defined in primitives/high-security crate)
pub struct HighSecurityConfig;
impl qp_high_security::HighSecurityInspector<AccountId, RuntimeCall> for HighSecurityConfig {
    fn is_high_security(who: &AccountId) -> bool {
        ReversibleTransfers::is_high_security_account(who)
    }
    
    fn is_whitelisted(call: &RuntimeCall) -> bool {
        matches!(call,
            RuntimeCall::ReversibleTransfers(Call::schedule_transfer { .. }) |
            RuntimeCall::ReversibleTransfers(Call::schedule_asset_transfer { .. }) |
            RuntimeCall::ReversibleTransfers(Call::cancel { .. })
        )
    }
    
    fn guardian(who: &AccountId) -> Option<AccountId> {
        ReversibleTransfers::get_guardian(who)
    }
}
```

### Documentation

- See `MULTISIG_REQ.md` for complete high-security integration requirements
- See `pallet-reversible-transfers` docs for guardian management and delay configuration

## License

MIT-0
