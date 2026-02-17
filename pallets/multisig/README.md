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

// 3. Charlie approves (2/2 threshold reached → proposal status becomes Approved)
Multisig::approve(Origin::signed(charlie), multisig_addr, proposal_id);

// 4. Any signer executes the approved proposal
Multisig::execute(Origin::signed(charlie), multisig_addr, proposal_id);
// ✅ Transaction executed! Proposal removed from storage, deposit returned to proposer.
```

**Key Point:** Approval and execution are **separate**. When the threshold is reached, the proposal status becomes `Approved`; any signer must then call `execute()` to dispatch the call.

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

**No auto-cleanup in propose:** The pallet does **not** remove expired proposals when creating a new one. To free slots and recover deposits from expired proposals, the proposer must call `claim_deposits()` or any signer can call `remove_expired()` for individual proposals.

**Threshold=1 behaviour:**
If the multisig has `threshold=1`, the proposal becomes **Approved** immediately after creation (proposer counts as the only required approval). The proposer (or any signer) must then call `execute()` to dispatch the call and remove the proposal.

**Economic Costs:**
- **ProposalFee**: Non-refundable fee (spam prevention, scaled by signer count) → burned
- **ProposalDeposit**: Refundable deposit (storage rent) → returned when proposal removed

**Important:** Fee is ALWAYS paid, even if proposal expires or is cancelled. Only deposit is refundable.

### 3. Approve Transaction
Adds caller's approval to an existing proposal. **If this approval brings the total approvals to or above the threshold, the proposal status becomes `Approved`**; the call is **not** executed here—use `execute()` for that.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the proposal to approve (REQUIRED)

**Validation:**
- Caller must be a signer
- Proposal must exist and be Active
- Proposal must not be expired (current_block ≤ expiry)
- Caller must not have already approved

**When threshold is reached:**
- Proposal status is set to `Approved`
- `ProposalReadyToExecute` event is emitted
- Any signer can then call `execute()` to dispatch the call

**Economic Costs:** None (deposit is returned when the proposal is executed or cancelled).

### 4. Cancel Transaction
Cancels a proposal and immediately removes it from storage (proposer only).

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the proposal to cancel (REQUIRED)

**Validation:**
- Caller must be the proposer
- Proposal must exist and be **Active or Approved** (both can be cancelled)

**Economic Effects:**
- Proposal **immediately removed** from storage
- ProposalDeposit **immediately returned** to proposer
- Counters decremented (active_proposals, proposals_per_signer)

**Economic Costs:** None (deposit immediately returned)

**Note:** ProposalFee is NOT refunded (it was burned at proposal creation).

### 5. Execute Transaction
Dispatches an **Approved** proposal. Can be called by any signer of the multisig once the approval threshold has been reached.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the proposal to execute (REQUIRED)

**Validation:**
- Caller must be a signer
- Proposal must exist and have status **Approved**
- Proposal must not be expired (current_block ≤ expiry)

**Effects:**
- Call is decoded and dispatched with multisig_address as origin
- Proposal is removed from storage
- ProposalDeposit is returned to the proposer
- `ProposalExecuted` event is emitted

**Economic Costs:** Weight depends on call size (charged upfront for MaxCallSize, refunded for actual size).

### 6. Remove Expired
Manually removes a single expired **Active** proposal from storage. Only signers can call this. Deposit is returned to the original proposer.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_id: u32` - ID (nonce) of the expired proposal (REQUIRED)

**Validation:**
- Caller must be a signer of the multisig
- Proposal must exist and be Active
- Must be expired (current_block > expiry)

**Note:** Executed/Cancelled proposals are removed immediately when executed/cancelled. This extrinsic only applies to **Active** proposals that are past expiry.

**Economic Effects:**
- ProposalDeposit returned to **original proposer** (not caller)
- Proposal removed from storage
- Counters decremented (active_proposals, proposals_per_signer)

**Economic Costs:** None (deposit always returned to proposer)

### 7. Claim Deposits
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
- Gas cost proportional to proposals iterated and cleaned (dynamic weight; charged upfront for worst-case, refunded for actual work)

**Note:** This is the main way to clean up a proposer's expired proposals and free per-signer quota (there is no auto-cleanup in `propose()`).

### 8. Approve Dissolve
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
  - **Returned to creator** when multisig is dissolved (via `approve_dissolve` after threshold approvals)
  - Locked until no proposals exist and balance is zero
  - Opportunity cost incentivizes cleanup
  
- **ProposalDeposit**:
  - Reserved on proposal creation
  - **Refundable** - returned in following scenarios:
  - **When proposal is executed:** Any signer calls `execute()` on an Approved proposal → deposit returned to proposer
  - **When proposal is cancelled:** Proposer calls `cancel()` (Active or Approved) → deposit returned to proposer
  - **Expired proposals:** No auto-cleanup in `propose()`. Proposer recovers deposits via `claim_deposits()`; any signer can remove a single expired proposal via `remove_expired()` (deposit → proposer)

### Storage Limits & Configuration
**Purpose:** Prevent unbounded storage growth and resource exhaustion

- **MaxSigners**: Maximum signers per multisig
  - Trade-off: Higher → more flexible governance, more computation per approval
  
- **MaxTotalProposalsInStorage**: Maximum total proposals (Active + Approved; Executed/Cancelled are removed immediately)
  - Trade-off: Higher → more flexible, more storage risk
  - Forces periodic cleanup to continue operating (via `claim_deposits()` or `remove_expired()`)
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
    status: ProposalStatus,             // Active | Approved (Executed/Cancelled are removed immediately)
}

enum ProposalStatus {
    Active,    // Collecting approvals
    Approved,  // Threshold reached; any signer can call execute()
    // Executed and Cancelled are not stored — proposal is removed immediately
}
```

**Important:** Only **Active** and **Approved** proposals are stored. When a proposal is executed or cancelled, it is **immediately removed** from storage and the deposit is returned. Historical data is available through events (see Historical Data section below).

### DissolveApprovals: Map<AccountId, BoundedVec<AccountId>>
Tracks which signers have approved dissolving each multisig.
- Key: Multisig address
- Value: List of signers who approved dissolution
- Cleared when multisig is dissolved or when threshold reached

## Events

- `MultisigCreated { creator, multisig_address, signers, threshold, nonce }`
- `ProposalCreated { multisig_address, proposer, proposal_id }`
- `ProposalApproved { multisig_address, approver, proposal_id, approvals_count }`
- `ProposalReadyToExecute { multisig_address, proposal_id, approvals_count }` — emitted when threshold is reached (approve or propose with threshold=1); proposal is Approved until someone calls `execute()`
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
- `ProposalNotActive` - Proposal is not active or approved (already executed or cancelled)
- `ProposalNotApproved` - Proposal is not in Approved status (for `execute()`)
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
- Explicit cleanup (claim_deposits, remove_expired) keeps storage under control

### Storage Cleanup
- No auto-cleanup in `propose()` (predictable weight; proposer must free slots via cleanup)
- Manual cleanup via `remove_expired()`: any signer can remove a single expired Active proposal (deposit → proposer)
- Batch cleanup via `claim_deposits()`: proposer recovers all their expired proposal deposits at once and frees per-signer quota

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
    type MaxTotalProposalsInStorage = ConstU32<200>;    // Total storage cap (cleanup via claim_deposits/remove_expired)
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
- `propose()` charges upfront for worst-case high-security path: `propose_high_security(call.len())`. Actual weight refunded based on path: `propose(call_size)` for normal multisig, `propose_high_security(call_size)` for HS. No cleanup in propose (no iteration/cleanup parameters).
- `execute()` charges upfront for `execute(MaxCallSize)`; actual weight refunded as `execute(actual_call_size)`.
- `claim_deposits()` charges upfront for worst-case iteration and cleanup; actual weight based on proposals iterated and cleaned (dynamic refund).

**Security notes:**
- Call size is validated BEFORE decode to prevent DoS via oversized payloads
- Weight formula includes O(call_size) component for decode (HS path) to prevent underpayment
- Benchmarks must be regenerated after logic changes (see README / MULTISIG_REQ benchmarking section)

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
