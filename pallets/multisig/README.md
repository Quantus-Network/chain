# Multisig Pallet

A multisignature wallet pallet for the Quantus blockchain with an economic security model.

## Overview

This pallet provides functionality for creating and managing multisig accounts that require multiple approvals before executing transactions. It implements a dual fee+deposit system for spam prevention and storage cleanup mechanisms with grace periods.

## Quick Start

Basic workflow for using a multisig:

```rust
// 1. Create a 2-of-3 multisig (Alice creates, Bob/Charlie/Dave are signers)
Multisig::create_multisig(Origin::signed(alice), vec![bob, charlie, dave], 2);
let multisig_addr = Multisig::derive_multisig_address(&[bob, charlie, dave], 0);

// 2. Bob proposes a transaction
let call = RuntimeCall::Balances(pallet_balances::Call::transfer { dest: eve, value: 100 });
Multisig::propose(Origin::signed(bob), multisig_addr, call.encode(), expiry_block);

// 3. Charlie approves - transaction executes automatically (2/2 threshold reached)
Multisig::approve(Origin::signed(charlie), multisig_addr, proposal_hash);
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

**Validation:**
- No duplicate signers
- Threshold must be > 0
- Threshold cannot exceed number of signers
- Signers count must be ≤ MaxSigners

**Important:** Signers are automatically sorted before storing and address generation. Order doesn't matter:
- `[alice, bob, charlie]` → sorted to `[alice, bob, charlie]` → `address_1`
- `[charlie, bob, alice]` → sorted to `[alice, bob, charlie]` → `address_1` (same!)
- To create multiple multisigs with same signers, the nonce provides uniqueness

**Economic Costs:**
- **MultisigFee**: Non-refundable fee (spam prevention) → burned
- **MultisigDeposit**: Refundable deposit (storage rent) → returned when multisig dissolved

### 2. Propose Transaction
Creates a new proposal for multisig execution.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig account (REQUIRED)
- `call: Vec<u8>` - Encoded RuntimeCall to execute (REQUIRED, max MaxCallSize bytes)
- `expiry: BlockNumber` - Deadline for collecting approvals (REQUIRED)

**Validation:**
- Caller must be a signer
- Call size must be ≤ MaxCallSize
- Multisig cannot have MaxActiveProposals or more open proposals
- Multisig cannot have MaxTotalProposalsInStorage or more total proposals in storage
- Expiry must be in the future (expiry > current_block)
- Expiry must not exceed MaxExpiryDuration blocks from now (expiry ≤ current_block + MaxExpiryDuration)

**Economic Costs:**
- **ProposalFee**: Non-refundable fee (spam prevention, scaled by signer count) → burned
- **ProposalDeposit**: Refundable deposit (storage rent) → returned when proposal removed

**Important:** Fee is ALWAYS paid, even if proposal expires or is cancelled. Only deposit is refundable.

### 3. Approve Transaction
Adds caller's approval to an existing proposal. **If this approval brings the total approvals 
to or above the threshold, the transaction will be automatically executed.**

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_hash: Hash` - Hash of proposal to approve (REQUIRED)

**Validation:**
- Caller must be a signer
- Proposal must exist
- Proposal must not be expired (current_block ≤ expiry)
- Caller must not have already approved

**Auto-Execution:**
When approval count reaches the threshold:
- Encoded call is executed as multisig_address origin
- ProposalDeposit returned to proposer
- Proposal removed from storage
- TransactionExecuted event emitted with execution result

**Economic Costs:** None (only transaction fees, deposit returned on execution)

### 4. Cancel Transaction
Cancels a proposal (proposer only).

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_hash: Hash` - Hash of proposal to cancel (REQUIRED)

**Validation:**
- Caller must be the proposer
- Proposal must exist

**Economic Effects:**
- ProposalDeposit returned to proposer
- Proposal removed from storage

**Economic Costs:** None (deposit returned)

**Note:** ProposalFee is NOT refunded - it was burned at proposal creation.

### 5. Remove Expired
Removes expired proposals from storage (cleanup mechanism). Only signers can call this.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)
- `proposal_hash: Hash` - Hash of expired proposal (REQUIRED)

**Validation:**
- Caller must be a signer of the multisig
- Proposal must exist
- For Active proposals: must be expired (current_block > expiry)
- For Executed/Cancelled proposals: can be removed anytime

**Economic Effects:**
- ProposalDeposit returned to **original proposer** (not caller)
- Proposal removed from storage

**Economic Costs:** None (deposit always returned to proposer)

**Note:** This allows any signer to help cleanup storage, even if the proposer is inactive. The deposit always goes back to the proposer, preventing any incentive for malicious cleanup.

### 6. Claim Deposits
Batch cleanup operation to recover all eligible deposits.

**Required Parameters:**
- `multisig_address: AccountId` - Target multisig (REQUIRED)

**Validation:**
- Only cleans proposals where caller is proposer
- For Active proposals: must be expired (current_block > expiry)
- For Executed/Cancelled proposals: can always be removed

**Economic Effects:**
- Returns all eligible proposal deposits to caller
- Removes all eligible proposals from storage

**Economic Costs:** None (only returns deposits)

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

### Deposits (Refundable, locked as storage rent)
**Purpose:** Compensate for on-chain storage, incentivize cleanup

- **MultisigDeposit**:
  - Reserved on multisig creation
  - Returned when multisig dissolved (via `dissolve_multisig`)
  - Locked until no proposals exist and balance is zero
  - Opportunity cost incentivizes cleanup
  
- **ProposalDeposit**:
  - Reserved on proposal creation
  - Returned when proposal removed (via `remove_expired` or `claim_deposits`)
  - **Grace Period:** Not auto-returned on execution to enable:
    - On-chain queryability for explorers
    - Indexer processing time
    - Audit trail availability
  - Locked capital incentivizes active storage management

### Storage Limits & Configuration
**Purpose:** Prevent unbounded storage growth and resource exhaustion

- **MaxSigners**: Maximum signers per multisig
  - Trade-off: Higher → more flexible governance, more computation per approval
  
- **MaxActiveProposals**: Maximum concurrent active proposals per multisig
  - Trade-off: Lower → better spam protection, may limit legitimate use
  - Prevents flooding attacks
  
- **MaxTotalProposalsInStorage**: Maximum total proposals (Active + Executed + Cancelled)
  - Trade-off: Higher → more flexible, more storage risk
  - Forces periodic cleanup to continue operating
  - Recommend: 2× MaxActiveProposals
  
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
    signers: BoundedVec<AccountId>,    // List of authorized signers
    threshold: u32,                     // Required approvals
    nonce: u64,                         // Unique identifier used in address generation
    deposit: Balance,                   // Reserved deposit (refundable)
    creator: AccountId,                 // Who created it (receives deposit back)
    last_activity: BlockNumber,         // Last action timestamp (for grace period)
    active_proposals: u32,              // Count of open proposals (for MaxActiveProposals check)
}
```

### Proposals: DoubleMap<AccountId, Hash, ProposalData>
Stores proposal data indexed by (multisig_address, proposal_hash):
```rust
ProposalData {
    proposer: AccountId,                // Who proposed (receives deposit back)
    call: BoundedVec<u8>,               // Encoded RuntimeCall to execute
    expiry: BlockNumber,                // Deadline for approvals
    approvals: BoundedVec<AccountId>,   // List of signers who approved
    deposit: Balance,                   // Reserved deposit (refundable)
}
```

### ExecutedProposals: DoubleMap<AccountId, Hash, ExecutedProposalData>
**Archive of successfully executed proposals.** Only proposals that were executed are stored here.
Cancelled or expired proposals are NOT archived (only available in events).

```rust
ExecutedProposalData {
    proposer: AccountId,                // Who proposed
    call: BoundedVec<u8>,               // The call that was executed
    approvers: BoundedVec<AccountId>,   // Full list of who approved
    executed_at: BlockNumber,           // When it was executed
    execution_succeeded: bool,          // Whether the call succeeded
}
```

**Purpose:** Provides permanent on-chain history of all executed multisig transactions.
Can be queried using `Multisig::get_executed_proposal(multisig_address, proposal_hash)`.

### GlobalNonce: u64
Internal counter for generating unique multisig addresses. Not exposed via API.

## Events

- `MultisigCreated { creator, multisig_address, signers, threshold, nonce }`
- `TransactionProposed { multisig_address, proposer, proposal_hash }`
- `ProposalApproved { multisig_address, approver, proposal_hash, approvals_count }`
- `ProposalExecuted { multisig_address, proposal_hash, proposer, call, approvers, result }`
- `ProposalCancelled { multisig_address, proposer, proposal_hash }`
- `ProposalRemoved { multisig_address, proposal_hash, proposer, removed_by }`
- `DepositsClaimed { multisig_address, claimer, total_returned, proposals_removed, multisig_removed }`

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
- `TooManyActiveProposals` - Multisig has MaxActiveProposals open proposals
- `TooManyProposalsInStorage` - Multisig has MaxTotalProposalsInStorage total proposals (cleanup required to create new)
- `ProposalNotExpired` - Proposal not yet expired (for remove_expired)
- `ProposalNotActive` - Proposal is not active (already executed or cancelled)

## Important Behavior

### Signer Order Doesn't Matter
Signers are **automatically sorted** before address generation and storage:
- Input order is irrelevant - signers are always sorted deterministically
- Address is derived from `Hash(PalletId + sorted_signers + nonce)`
- Same signers in any order = same multisig address (with same nonce)
- To create multiple multisigs with same participants, use different creation transactions (nonce auto-increments)

**Example:**
```rust
// These create the SAME multisig address (same signers, same nonce):
create_multisig([alice, bob, charlie], 2) // → multisig_addr_1 (nonce=0)
create_multisig([charlie, bob, alice], 2) // → multisig_addr_1 (SAME! nonce would be 1 but already exists)

// To create another multisig with same signers:
create_multisig([alice, bob, charlie], 2) // → multisig_addr_2 (nonce=1, different address)
```

## Historical Data and Event Indexing

The pallet does **not** maintain on-chain storage of executed proposal history. Instead, all historical data is available through **blockchain events**, which are designed to be efficiently indexed by off-chain indexers like **SubSquid**.

### ProposalExecuted Event

When a proposal is successfully executed, the pallet emits a comprehensive `ProposalExecuted` event containing all relevant data:

```rust
Event::ProposalExecuted {
    multisig_address: T::AccountId,   // The multisig that executed
    proposal_hash: T::Hash,            // Hash of the proposal
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
- MaxActiveProposals limits per-multisig open proposals

### Storage Cleanup
- Grace period allows proposers priority cleanup
- After grace: public cleanup incentivized
- Batch cleanup via claim_deposits for efficiency

### Economic Attacks
- **Multisig Spam:** Costs MultisigFee (burned, reduces supply)
  - No refund even if never used
  - Economic barrier to creation spam
- **Proposal Spam:** Costs ProposalFee (burned, reduces supply) + ProposalDeposit (locked)
  - Fee never returned (even if expired/cancelled)
  - Deposit locked until cleanup
  - Cost scales with multisig size (dynamic pricing)
- **Result:** Spam attempts generate protocol revenue
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
    type MaxActiveProposals = ConstU32<100>;            // Spam protection
    type MaxTotalProposalsInStorage = ConstU32<200>;    // Total cap (recommend: 2× active)
    type MaxCallSize = ConstU32<10240>;                 // Per-proposal storage limit
    type MaxExpiryDuration = ConstU32<100_800>;         // Max proposal lifetime (~2 weeks @ 12s)
    
    // Economic parameters (example values - adjust per runtime)
    type MultisigFee = ConstU128<{ 100 * MILLI_UNIT }>;      // Creation barrier
    type MultisigDeposit = ConstU128<{ 500 * MILLI_UNIT }>;  // Storage rent
    type ProposalFee = ConstU128<{ 1000 * MILLI_UNIT }>;     // Base proposal cost
    type ProposalDeposit = ConstU128<{ 1000 * MILLI_UNIT }>; // Cleanup incentive
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

## License

MIT-0
