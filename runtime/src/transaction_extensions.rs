//! Custom signed extensions for the runtime.
extern crate alloc;
use crate::*;
use codec::{Decode, DecodeLimit, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::marker::PhantomData;
use frame_support::pallet_prelude::{
	InvalidTransaction, TransactionValidityError, ValidTransaction,
};
use qp_high_security::HighSecurityInspector;
use qp_wormhole::TransferProofRecorder;
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::{
	traits::{
		AsSystemOriginSigner, DispatchInfoOf, PostDispatchInfoOf, TransactionExtension, Zero,
	},
	DispatchResult, Weight,
};

/// `InvalidTransaction::Custom` code for a high-security signer attaching a tip.
/// Distinct from the whitelist rejection (`Custom(1)`).
pub const HIGH_SECURITY_TIP_FORBIDDEN: u8 = 2;

/// `InvalidTransaction::Custom` code when a high-security signer has already
/// included `MaxHighSecurityTxsPerWindow` extrinsics in the rolling window.
pub const HIGH_SECURITY_TX_QUOTA_EXCEEDED: u8 = 3;

/// `InvalidTransaction::Custom` code when a high-security signer's extrinsic
/// exceeds `MAX_HIGH_SECURITY_EXTRINSIC_LEN` encoded bytes.
pub const HIGH_SECURITY_EXTRINSIC_TOO_LARGE: u8 = 4;

/// `InvalidTransaction::Custom` code when a high-security signer's extrinsic
/// would cost more than `MAX_HIGH_SECURITY_INCLUSION_FEE` at zero tip.
pub const HIGH_SECURITY_FEE_LIMIT_EXCEEDED: u8 = 5;

/// Transaction extension for reversible accounts
///
/// This extension is used to intercept delayed transactions for users that opted in
/// for reversible transactions. Based on the policy set by the user, the transaction
/// will either be denied or intercepted and delayed.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct ReversibleTransactionExtension<T: pallet_reversible_transfers::Config>(PhantomData<T>);

impl<T: pallet_reversible_transfers::Config + Send + Sync> ReversibleTransactionExtension<T> {
	/// Creates new `TransactionExtension` to check genesis hash.
	pub fn new() -> Self {
		Self(core::marker::PhantomData)
	}
}

impl<T: pallet_reversible_transfers::Config + Send + Sync + alloc::fmt::Debug>
	TransactionExtension<RuntimeCall> for ReversibleTransactionExtension<T>
{
	/// Whether the signer is high-security, decided once in `validate` and
	/// carried forward: `prepare` records the quota only on the high-security
	/// path, and `post_dispatch_details` refunds the unused quota weight on
	/// every other path.
	type Pre = bool;
	type Val = bool;
	type Implicit = ();

	const IDENTIFIER: &'static str = "ReversibleTransactionExtension";

	fn weight(&self, _call: &RuntimeCall) -> Weight {
		// Worst case — a high-security signer, four reads and one write:
		//   1r `HighSecurityAccounts` classification        (validate)
		//   1r `NextFeeMultiplier` for the fee ceiling      (validate)
		//   1r `HighSecurityTxQuota` ring, admission check  (validate)
		//   1r+1w `HighSecurityTxQuota` ring, recording     (prepare)
		// The pallet helpers (`hs_quota_has_room` / `record_hs_quota`) do
		// not re-read `HighSecurityAccounts`, so it is read exactly once.
		// All other traffic
		// performs only the classification read; the surplus is returned in
		// `post_dispatch_details`. Walking `batch_all` children in
		// `is_whitelisted` is in-memory only. Proof size is deliberately not
		// modeled: this is a solo PoW chain (no PoV), matching `DbWeight`
		// usage across the runtime.
		T::DbWeight::get().reads_writes(4, 1)
	}

	fn prepare(
		self,
		val: Self::Val,
		origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// `val` is fresh: during block execution `validate` runs immediately
		// before `prepare` on the same state, so the whitelist and length
		// gates in `validate` are consensus-enforced without a re-check here.
		if !val {
			return Ok(false);
		}
		let who = origin
			.as_system_origin_signer()
			.ok_or(TransactionValidityError::Invalid(InvalidTransaction::BadSigner))?;
		// Record here rather than in `validate`: mempool validation is not
		// sequenced with other same-account extrinsics in this block, so the
		// ring mutation must happen at inclusion time. `hs_ring_record`
		// re-checks ring admission; the high-security classification itself is
		// taken from `val` (fresh: `validate` ran just before on this state).
		pallet_reversible_transfers::Pallet::<Runtime>::record_hs_quota(who).map_err(|_| {
			TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TX_QUOTA_EXCEEDED,
			))
		})?;
		Ok(true)
	}

	fn validate(
		&self,
		origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		let Some(who) = origin.as_system_origin_signer() else {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner));
		};

		// The one `HighSecurityAccounts` classification read, shared by the
		// whitelist check below, the quota check, the recording in `prepare`
		// and the weight refund in `post_dispatch_details`.
		let is_high_security = crate::configs::HighSecurityConfig::is_high_security(who);

		// Enforce the high-security whitelist on the top-level signer.
		// `is_whitelisted` walks `batch_all` children so a mixed batch is rejected here.
		// Origin-rewriting wrappers (multisig execution) re-check the whitelist at the
		// effective origin inside their own pallets at dispatch time.
		if !crate::configs::HighSecurityConfig::is_call_allowed_given(is_high_security, call) {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(1)));
		}

		// Cap the encoded length: the length fee is charged on the full
		// extrinsic pre-dispatch and never refunded, so a stolen key must not
		// be able to pad a future variable-length field and grind free
		// balance out to a colluding block author.
		if is_high_security && len as u32 > crate::configs::MAX_HIGH_SECURITY_EXTRINSIC_LEN {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_EXTRINSIC_TOO_LARGE,
			)));
		}

		// Bound the zero-tip inclusion fee itself, not just its inputs: a
		// future whitelisted call with an unforeseen length or weight surface
		// (the fee input the length cap above cannot see) cannot reopen the
		// fee-drain channel. Deterministic because `FeeMultiplierUpdate` is a
		// constant one.
		if is_high_security &&
			pallet_transaction_payment::Pallet::<Runtime>::compute_fee(len as u32, info, 0) >
				crate::configs::MAX_HIGH_SECURITY_INCLUSION_FEE
		{
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_FEE_LIMIT_EXCEEDED,
			)));
		}

		if is_high_security &&
			!pallet_reversible_transfers::Pallet::<Runtime>::hs_quota_has_room(who)
		{
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TX_QUOTA_EXCEEDED,
			)));
		}

		Ok((ValidTransaction::default(), is_high_security, origin))
	}

	fn post_dispatch_details(
		pre: Self::Pre,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_post_info: &PostDispatchInfoOf<RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<Weight, TransactionValidityError> {
		if pre {
			// High-security path: the full declared weight was used.
			return Ok(Weight::zero());
		}
		// Everyone else only did the classification read; return the fee
		// ceiling's multiplier read and the quota ring reads/write reserved
		// by `weight()`. This extension precedes the payment extension in
		// `TxExtension`, so the refund reaches the payer's fee, not just
		// block capacity.
		Ok(T::DbWeight::get().reads_writes(3, 1))
	}
}

/// The fee adapter that actually moves funds; [`HighSecurityFungibleAdapter`]
/// only vets the tip before delegating.
type InnerFeeAdapter = pallet_transaction_payment::FungibleAdapter<
	Balances,
	pallet_mining_rewards::TransactionFeesCollector<Runtime>,
>;

/// `OnChargeTransaction` adapter that forbids tips from high-security signers.
///
/// The call whitelist cannot see the tip: it lives on the payment extension,
/// not on `RuntimeCall`. Enforcing it in a wrapper extension proved fragile —
/// the wrapper had to impersonate the stock `ChargeTransactionPayment`
/// `IDENTIFIER`, so a refactor back to the unwrapped extension would have
/// compiled with byte-identical metadata while silently reopening the tip
/// channel. This adapter sees both `who` and `tip` on every fee path —
/// `can_withdraw_fee` on the (mempool and consensus) validation path and
/// `withdraw_fee` at inclusion — so the policy survives any change to the
/// extension tuple.
///
/// High-security accounts are delayed by design and do not need priority
/// bidding; a non-zero tip is rejected with `Custom(HIGH_SECURITY_TIP_FORBIDDEN)`
/// before anything is withdrawn.
///
/// Weight note: the `HighSecurityAccounts` read happens only on the
/// tip-carrying path (`can_withdraw_fee` during validation, `withdraw_fee` at
/// inclusion). The stock benchmarked payment weight cannot see this branch,
/// so [`PaymentWeightsWithTipPolicy`] — the configured payment `WeightInfo` —
/// declares both reads unconditionally.
pub struct HighSecurityFungibleAdapter;

/// `pallet_transaction_payment` weights adjusted for Quantus-owned work the
/// stock kitchensink benchmark never measures:
///
/// * two `HighSecurityAccounts` reads from the tip policy in [`HighSecurityFungibleAdapter`]
///   (`can_withdraw_fee` then `withdraw_fee` on a tipped transaction). Many distinct tipped signers
///   can appear in one block, so these are not warm-cache hits.
/// * one unique-key `CollectedFees` read and write from
///   [`pallet_mining_rewards::TransactionFeesCollector`] on every nonzero corrected fee. The
///   follow-on `get` for the event is same-key.
///
/// The high-security reads are charged unconditionally: the payment extension
/// has no tip-keyed refund hook, so zero-tip traffic overpays those two reads
/// (~50µs ref_time) — an error in the safe direction. The collector access
/// runs on every paid extrinsic, so it is not an overcharge.
pub struct PaymentWeightsWithTipPolicy;

impl pallet_transaction_payment::WeightInfo for PaymentWeightsWithTipPolicy {
	fn charge_transaction_payment() -> Weight {
		let db = <Runtime as frame_system::Config>::DbWeight::get();
		pallet_transaction_payment::weights::SubstrateWeight::<Runtime>::charge_transaction_payment(
		)
		.saturating_add(db.reads(2))
		.saturating_add(db.reads_writes(1, 1))
	}
}

impl HighSecurityFungibleAdapter {
	fn reject_high_security_tip(
		who: &AccountId,
		tip: Balance,
	) -> Result<(), TransactionValidityError> {
		// Tip compared first so the storage read is skipped on the zero-tip path.
		if !tip.is_zero() && crate::configs::HighSecurityConfig::is_high_security(who) {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TIP_FORBIDDEN,
			)));
		}
		Ok(())
	}
}

impl pallet_transaction_payment::TxCreditHold<Runtime> for HighSecurityFungibleAdapter {
	type Credit = <InnerFeeAdapter as pallet_transaction_payment::TxCreditHold<Runtime>>::Credit;
}

impl pallet_transaction_payment::OnChargeTransaction<Runtime> for HighSecurityFungibleAdapter {
	type Balance = Balance;
	type LiquidityInfo = <InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<
		Runtime,
	>>::LiquidityInfo;

	fn withdraw_fee(
		who: &AccountId,
		call: &RuntimeCall,
		dispatch_info: &DispatchInfoOf<RuntimeCall>,
		fee_with_tip: Self::Balance,
		tip: Self::Balance,
	) -> Result<Self::LiquidityInfo, TransactionValidityError> {
		Self::reject_high_security_tip(who, tip)?;
		<InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<Runtime>>::withdraw_fee(
			who,
			call,
			dispatch_info,
			fee_with_tip,
			tip,
		)
	}

	fn can_withdraw_fee(
		who: &AccountId,
		call: &RuntimeCall,
		dispatch_info: &DispatchInfoOf<RuntimeCall>,
		fee_with_tip: Self::Balance,
		tip: Self::Balance,
	) -> Result<(), TransactionValidityError> {
		Self::reject_high_security_tip(who, tip)?;
		<InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<Runtime>>::can_withdraw_fee(
			who,
			call,
			dispatch_info,
			fee_with_tip,
			tip,
		)
	}

	fn correct_and_deposit_fee(
		who: &AccountId,
		dispatch_info: &DispatchInfoOf<RuntimeCall>,
		post_info: &PostDispatchInfoOf<RuntimeCall>,
		corrected_fee_with_tip: Self::Balance,
		tip: Self::Balance,
		liquidity_info: Self::LiquidityInfo,
	) -> Result<(), TransactionValidityError> {
		// No tip re-check: a high-security tip never gets past
		// `can_withdraw_fee` / `withdraw_fee`, so `tip` is zero here.
		<InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<Runtime>>::correct_and_deposit_fee(
			who,
			dispatch_info,
			post_info,
			corrected_fee_with_tip,
			tip,
			liquidity_info,
		)
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn endow_account(who: &AccountId, amount: Self::Balance) {
		<InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<Runtime>>::endow_account(
			who, amount,
		)
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn minimum_balance() -> Self::Balance {
		<InnerFeeAdapter as pallet_transaction_payment::OnChargeTransaction<Runtime>>::minimum_balance()
	}
}

/// Transaction extension that records transfer proofs in the wormhole pallet
///
/// This extension uses an EVENT-BASED approach to detect transfers:
/// - After successful execution, scans for `Transfer`, `Minted`, `TransferOnHold` and
///   `ReserveRepatriated` events
/// - Records proofs for any transfers that were sent TO a wormhole account
/// - Automatically catches ALL transfers dispatched inside a transaction, regardless of how they're
///   initiated:
///   - Direct transfers (transfer, transfer_keep_alive, transfer_all, etc.)
///   - Batch transfers (utility.batch_all)
///   - Multisig transfers (multisig.execute)
///   - Held-fund seizures/recoveries (reversible_transfers.cancel / recover_funds, which move value
///     with `transfer_on_hold` instead of a free-balance transfer). Owner cancel of a one-time
///     schedule uses `release` instead — hold → free on the same account, not a credit — so it
///     emits no `TransferOnHold` and records no leaf.
///   - Future call-based mechanisms automatically covered, since wrapper calls emit their inner
///     events within the same extrinsic's event range
///
/// COVERAGE BOUNDARY: transaction extensions only run for transactions, so this scan
/// never sees events emitted from hooks (`on_initialize` / `on_finalize`). Every
/// hook-context credit therefore needs — and has — an explicit
/// `TransferProofRecorder::record_transfer_proof` call instead:
///   - reversible-transfers' scheduled execution records its transfer in `do_execute_transfer`
///     (skipped when `from == to`, which moves no value);
///   - mining rewards record theirs in `on_finalize` (`pallet_mining_rewards`). Sub-quantum
///     remainder stays in `CollectedFees` for the next miner (no leaf). Those credits use
///     `mint_into`, which *does* emit `Balances::Minted` (the same event this scanner records);
///     they are safe from double-recording only because distribution runs in `on_finalize`, outside
///     every extrinsic's scan window. Moving that distribution into `on_initialize` or a signed
///     path without also suppressing the scan (or the explicit record) would inflate wormhole exit
///     capacity.
///
/// The one remaining hook-context path is a governance-enacted call: referenda enactment
/// dispatches the approved call via the scheduler in `on_initialize`, so its events are
/// not scanned and no leaf is recorded. This is a
/// known, accepted gap rather than an oversight: the scheduler's `ScheduleOrigin` is
/// Root, the tech-referenda track only accepts Root proposal origins, and sudo is
/// removed — so only Root can reach it, and Root can already forge or delete leaves
/// outright (`set_storage`, runtime upgrades), so there is no invariant left to defend
/// against it. The miss is conservative (the credit exists but gains no ZK-spendable
/// leaf; no unbacked exit capacity is created) and repairable (governance can re-issue
/// the credit as an ordinary signed transfer if a leaf is wanted).
///
/// This addresses audit item EQ-QNT-WORMHOLE-F-05.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug, DecodeWithMemTracking)]
#[scale_info(skip_type_params(T))]
pub struct WormholeProofRecorderExtension<T: pallet_wormhole::Config + Send + Sync>(PhantomData<T>);

impl<T: pallet_wormhole::Config + Send + Sync> WormholeProofRecorderExtension<T> {
	/// Creates new extension
	pub fn new() -> Self {
		Self(PhantomData)
	}

	/// Weight charged per recorded transfer proof.
	///
	/// Per recorded transfer, `record_transfer` touches one `TransferCount` read and one
	/// write, plus the ZK-tree leaf insert. The insert price is FLAT at the circuit
	/// depth ceiling (see [`pallet_zk_tree::INSERT_LEAF_DB_OPS`]), so multiplying this
	/// single price by a transfer count is sound even for multi-transfer calls that
	/// cross capacity boundaries. The path update also puts every tree key it reads
	/// into the PoV ([`pallet_zk_tree::TREE_KEY_POV`] each); omitting that proof-size
	/// term would let deep-tree blocks exceed the PoV budget validators re-execute
	/// against. Recording finally deposits [`Self::EVENTS_PER_RECORDED_PROOF`] events;
	/// those land after the scan snapshot, so this reservation is the only place their
	/// System work can be charged.
	fn per_transfer_weight() -> Weight {
		let (tree_reads, tree_writes) = pallet_zk_tree::INSERT_LEAF_DB_OPS;
		let hash_time = pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS;
		T::DbWeight::get()
			.reads_writes(1u64.saturating_add(tree_reads), 1u64.saturating_add(tree_writes))
			.saturating_add(Weight::from_parts(
				hash_time,
				tree_reads.saturating_mul(pallet_zk_tree::TREE_KEY_POV),
			))
			.saturating_add(Self::event_deposit_weight(Self::EVENTS_PER_RECORDED_PROOF))
	}

	/// Events deposited by one successfully recorded proof: `ZkTree::LeafInserted`
	/// plus the wormhole's `NativeTransferred` / `AssetTransferred`. (`TreeGrew` is
	/// deliberately not modeled: the tree grows at most [`pallet_zk_tree::MAX_TREE_DEPTH`]
	/// times over the chain's entire life, and the flat depth-ceiling insert pricing
	/// already carries margin for young trees.)
	const EVENTS_PER_RECORDED_PROOF: u64 = 2;

	/// Weight of depositing `events` records from proof recording. Each
	/// `frame_system::deposit_event` reads and writes `EventCount` and appends the
	/// encoded record to `Events`. These deposits happen after the scan snapshot is
	/// taken, so no scan charge can cover them — they must be part of the static
	/// per-transfer reservation.
	fn event_deposit_weight(events: u64) -> Weight {
		T::DbWeight::get().reads_writes(events, events.saturating_mul(2))
	}

	/// Worst-case `ref_time` (picoseconds) of the *structural* work to decode one
	/// `EventRecord` in [`Self::record_proofs_from_events_since`]: phase + event enum
	/// dispatch + topics. Payload size is charged separately per byte (see
	/// [`Self::EVENT_SCAN_DECODE_BYTE_REF_TIME_PS`]); 1µs is a conservative ceiling
	/// for the fixed part.
	const EVENT_SCAN_DECODE_REF_TIME_PS: u64 = 1_000_000;

	/// Worst-case `ref_time` (picoseconds) to stream-decode one *byte* of the events
	/// blob. Record size is caller-influenced — a successful `Multisig::execute` emits
	/// `ProposalExecuted` carrying the full stored call (up to `MaxCallSize` = 10 KiB)
	/// plus the approver vector (up to `MaxSigners` = 100 accounts), an order of
	/// magnitude past any fixed per-record assumption — so decode work must be priced
	/// by size, not record count alone. Payload decode is essentially a bounds-checked
	/// memcopy (~1ns/byte with allocation on reference hardware); 10ns/byte is a
	/// conservative ceiling.
	const EVENT_SCAN_DECODE_BYTE_REF_TIME_PS: u64 = 10_000;

	/// Weight of the post-dispatch event scan when `events` records totalling
	/// `event_bytes` encoded bytes are present at scan time. The scan fetches the
	/// encoded `Events` value exactly once (one read, one copy — see
	/// `read_events_no_consensus_single_copy`; the 2 KiB-buffered `stream_iter` would
	/// re-materialize the whole overlay value per refill, O(bytes²) for a full scan)
	/// and then decodes EVERY record present — `Iterator::skip` discards but still
	/// decodes the pre-snapshot prefix — so the cost is per record *present*, not per
	/// record matched or recorded, and it scales with the payload bytes those records
	/// carry (a single legitimate `Multisig::ProposalExecuted` can be ~13 KiB).
	fn event_scan_weight(events: u32, event_bytes: u32) -> Weight {
		if events == 0 {
			return Weight::zero();
		}
		T::DbWeight::get().reads(1).saturating_add(Weight::from_parts(
			Self::EVENT_SCAN_DECODE_REF_TIME_PS
				.saturating_mul(u64::from(events))
				.saturating_add(
					Self::EVENT_SCAN_DECODE_BYTE_REF_TIME_PS.saturating_mul(u64::from(event_bytes)),
				),
			0,
		))
	}

	fn count_transfers(call: &RuntimeCall) -> u64 {
		// NOTE: this must stay in sync with the events matched by `record_proofs_from_events_since`
		// — we only weight calls whose emitted events we actually record.
		match call {
			// `execute` carries only `(multisig_address, proposal_id)`. The stored
			// call is immutable after propose, so walk it with the same matcher —
			// a byte-density bound under-counts `recover_funds` (17 credits in 34
			// bytes) composed with packed transfers. Missing / undecodable
			// proposals charge 0; execute will fail on them anyway. Nested
			// stored `execute` is not chased: the inner call is dispatched as
			// the derived address, which is not a signer, so it cannot succeed.
			RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address,
				proposal_id,
			}) => Self::count_stored_execute(multisig_address, *proposal_id),
			_ => Self::count_visible_transfers(call),
		}
	}

	fn count_stored_execute(multisig_address: &AccountId, proposal_id: u32) -> u64 {
		let Some(proposal) =
			pallet_multisig::Proposals::<Runtime>::get(multisig_address, proposal_id)
		else {
			return 0;
		};
		let Ok(inner) = RuntimeCall::decode_with_depth_limit(
			pallet_multisig::MAX_MULTISIG_CALL_DEPTH,
			&mut &proposal.call[..],
		) else {
			return 0;
		};
		Self::count_visible_transfers(&inner)
	}

	/// One `Proposals` read plus a worst-case decode/walk of the stored call.
	/// Charged twice on `execute`: `weight()` walks to reserve proof inserts, and
	/// `validate()` walks again to carry the count into `prepare` so that path
	/// does not perform a third traversal.
	fn execute_proposal_walk_weight() -> Weight {
		let max_call = u64::from(<Runtime as pallet_multisig::Config>::MaxCallSize::get());
		let proposal_pov = pallet_multisig::ProposalDataOf::<Runtime>::max_encoded_len() as u64;
		T::DbWeight::get().reads(1).saturating_add(Weight::from_parts(
			Self::EVENT_SCAN_DECODE_REF_TIME_PS
				.saturating_add(Self::EVENT_SCAN_DECODE_BYTE_REF_TIME_PS.saturating_mul(max_call)),
			proposal_pov,
		))
	}

	fn is_multisig_execute(call: &RuntimeCall) -> bool {
		matches!(call, RuntimeCall::Multisig(pallet_multisig::Call::execute { .. }))
	}

	fn count_visible_transfers(call: &RuntimeCall) -> u64 {
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. }) => 1,

			// Guardian cancel seizes the hold with `transfer_on_hold` and emits one
			// `TransferOnHold` the scan records. Owner self-cancel of a one-time
			// schedule uses `release` instead (hold → free on the same account) and
			// records zero proofs. The static count of one is a conservative
			// reservation: `weight()` cannot see which branch will run, and
			// post-dispatch reconciliation refunds the unused charge.
			RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
				..
			}) => 1,

			// `recover_funds` seizes every pending hold to the guardian (one `TransferOnHold`
			// each) and then sweeps the account with a dispatched `transfer_all` (one
			// `Transfer`). How many holds are pending is on-chain state the submitted call
			// does not reveal — and same-block calls could even grow it after this count is
			// taken — so charge the static worst case. The overcharge on accounts with fewer
			// pending transfers is accepted: this is a rare emergency path, and `weight()`
			// must not depend on mutable state.
			RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::recover_funds { .. },
			) => u64::from(
				<Runtime as pallet_reversible_transfers::Config>::MaxPendingPerAccount::get(),
			)
			.saturating_add(1),

			RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) =>
				calls.iter().map(Self::count_visible_transfers).sum(),

			// Vesting calls fall through to 0 deliberately: the pallet records its
			// payouts itself (so Root calls enacted by the scheduler are captured too)
			// and carries the recording cost in its own benchmarked weights, while the
			// event scan below skips every pot-touching transfer. Counting them here
			// would charge twice for work this extension never performs.
			//
			// The converse — a plain `Balances` transfer whose *destination* is the
			// vesting pot (endowing it with its existential-deposit buffer) — is
			// charged for a leaf insert the scan then skips. That overcharge is
			// accepted: resolving the destination here would mean a `Lookup` on the
			// hottest call in the runtime to spare a handful of one-off bootstrap
			// transfers, and the direction is conservative. Pot-as-*source* needs no
			// handling at all: the pot is a keyless pallet account, so no signed call
			// this extension weighs can move funds out of it (the runtime has no
			// force-transfer extrinsic at all).
			_ => 0,
		}
	}

	/// Scan events and record transfer proofs for any transfers that occurred
	/// since the given event count (to avoid re-processing previous events
	/// within the same block).
	///
	/// `event_count_before` is the value from `frame_system::Pallet::event_count()`
	/// captured in `prepare()`.
	///
	/// Returns the number of transfer proofs recorded (so callers can reconcile the
	/// actual proof-recording work against the statically charged weight) and the
	/// encoded byte size of the scanned `Events` value (the input to
	/// [`Self::event_scan_weight`]'s per-byte term — taken from the very buffer the
	/// scan decodes, so the charge and the work can't drift apart).
	fn record_proofs_from_events_since(event_count_before: u32) -> (u64, u32) {
		// The single-copy reader fetches the encoded `Events` value exactly once and
		// decodes records from that in-memory snapshot, keeping the scan linear in the
		// value's size (`stream_iter` would re-materialize the whole overlay value on
		// every 2 KiB buffer refill — O(bytes²) for a full scan, superlinear work the
		// linear `event_scan_weight` charge could never bound). The snapshot also means
		// depositing events while iterating cannot corrupt the stream; we still collect
		// all transfers before recording (which deposits new events) for clarity.

		// The vesting pot's flows are excluded: the vesting pallet records its own
		// pot -> beneficiary payouts (also for scheduler-enacted Root calls this
		// extension never sees), so scanning them here would double-record; and pot
		// inbound/refund legs (treasury <-> pot) need no leaves — the pot is a keyless
		// pallet account and the treasury spends by signature, so neither can ever
		// exit through the wormhole.
		//
		// Derived lazily: it costs a Blake2b hash and the overwhelming majority of
		// extrinsics emit no `Transfer` event at all.
		let mut vesting_pot: Option<AccountId> = None;

		let (event_bytes, events) =
			frame_system::Pallet::<Runtime>::read_events_no_consensus_single_copy();

		// Collect transfers to record - (asset_id, from, to, amount)
		let transfers_to_record: alloc::vec::Vec<(Option<AssetId>, AccountId, AccountId, Balance)> =
			events
				.skip(event_count_before as usize)
				.filter_map(|event_record| {
					match event_record.event {
						// Native balance transfers
						RuntimeEvent::Balances(pallet_balances::Event::Transfer {
							from,
							to,
							amount,
						}) => {
							let pot = vesting_pot.get_or_insert_with(
								pallet_vesting::Pallet::<Runtime>::pot_account_id,
							);
							(&from != pot && &to != pot).then_some((None, from, to, amount))
						},
						// Native balance mints
						RuntimeEvent::Balances(pallet_balances::Event::Minted { who, amount }) => {
							let minting_account = crate::configs::MintingAccount::get();
							Some((None, minting_account, who, amount))
						},
						// Held-balance transfers. The reversible-transfers pallet releases
						// seized/recovered funds to the guardian with `transfer_on_hold`
						// (`Restriction::Free`), so the destination receives ordinary free
						// balance — a genuine credit that needs a leaf exactly like a
						// `Transfer`, it just emits a different event. Owner self-cancel
						// uses `release` instead of `transfer_on_hold`, so it never reaches
						// this arm; `record_transfer_proof` also drops `source == dest` as
						// a backstop. (`TransferAndHold` is deliberately not matched:
						// nothing in the runtime emits it.)
						RuntimeEvent::Balances(pallet_balances::Event::TransferOnHold {
							source,
							dest,
							amount,
							..
						}) => Some((None, source, dest, amount)),
						// Reserved-balance repatriations. `repatriate_reserved` emits this
						// instead of a `Transfer`. The event is only emitted for
						// cross-account moves (self-repatriations return early), and the
						// credit belongs to `to` whether it lands free or reserved, so
						// record it unconditionally.
						RuntimeEvent::Balances(pallet_balances::Event::ReserveRepatriated {
							from,
							to,
							amount,
							..
						}) => Some((None, from, to, amount)),
						_ => None, // Ignore all other events
					}
				})
				.collect();

		// Now record the proofs - this is safe because we're no longer iterating over Events.
		// Count only credits that were actually recorded: `record_transfer_proof` returns
		// `false` for deliberately dropped credits, and counting those as recorded would
		// over-reserve fees and falsely register extra block weight on opaque paths.
		let mut recorded = 0u64;
		for (asset_id, from, to, amount) in transfers_to_record {
			if <Wormhole as TransferProofRecorder<AccountId, AssetId, Balance>>::record_transfer_proof(
				asset_id, from, to, amount,
			) {
				recorded = recorded.saturating_add(1);
			}
		}
		(recorded, event_bytes)
	}
}

impl<T: pallet_wormhole::Config + Send + Sync + alloc::fmt::Debug> TransactionExtension<RuntimeCall>
	for WormholeProofRecorderExtension<T>
{
	/// `(event_count_snapshot, statically_charged_transfer_count)`. The snapshot bounds the
	/// event scan in `post_dispatch_details`; the charged count lets it reconcile actual
	/// proof-recording work against the weight reserved by `weight()` — registering any
	/// shortfall against the block and refunding any overcharge as unspent weight. A count
	/// is sufficient because the per-transfer price is flat (see
	/// [`Self::per_transfer_weight`]).
	type Pre = (u32, u64);
	/// Transfer count from the `validate()` walk; `prepare()` must not re-read the
	/// stored proposal.
	type Val = u64;
	type Implicit = ();

	const IDENTIFIER: &'static str = "WormholeProofRecorderExtension";

	fn weight(&self, call: &RuntimeCall) -> Weight {
		let n = Self::count_transfers(call);
		let proofs =
			if n > 0 { Self::per_transfer_weight().saturating_mul(n) } else { Weight::zero() };
		if Self::is_multisig_execute(call) {
			// Always reserve two worst-case proposal walks: `weight()` itself
			// walks to size the proof reservation, and `validate()` walks to
			// carry the count into `prepare`. A 10 KiB no-transfer proposal
			// must not be free — the pallet can reject a non-signer after one
			// `Multisigs` read, and that error path must not refund this work.
			proofs.saturating_add(Self::execute_proposal_walk_weight().saturating_mul(2))
		} else {
			proofs
		}
	}

	fn prepare(
		self,
		val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// Snapshot current event count so we only process events added by this tx
		// (and any events from previous txs in the same block). The transfer
		// count comes from `validate()` so a stored `execute` proposal is not
		// read and decoded a third time.
		Ok((frame_system::Pallet::<Runtime>::event_count(), val))
	}

	fn validate(
		&self,
		origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &DispatchInfoOf<RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		Ok((ValidTransaction::default(), Self::count_transfers(call), origin))
	}

	fn post_dispatch_details(
		pre: Self::Pre,
		info: &DispatchInfoOf<RuntimeCall>,
		_post_info: &PostDispatchInfoOf<RuntimeCall>,
		_len: usize,
		result: &DispatchResult,
	) -> Result<Weight, TransactionValidityError> {
		let (event_count_before, charged_transfers) = pre;

		// A failed dispatch rolled back its events: nothing is scanned and nothing is
		// recorded, so the static per-transfer reservation is unspent. Returning it
		// refunds both the fee (this extension precedes `ChargeTransactionPayment`
		// in `TxExtension`, so payment sees the corrected weight) and block
		// capacity (via the trailing `WeightReclaim`). The execute proposal-walk
		// reservation is not returned: that work already ran in `weight()` /
		// `validate()`, including on the non-signer reject path.
		if result.is_err() {
			return Ok(Self::per_transfer_weight().saturating_mul(charged_transfers));
		}

		// Captured BEFORE recording deposits new events: this is exactly the number
		// of records the scan below decodes. The scanned byte size comes back from
		// the scan itself — measured on the very buffer it decoded.
		let events_at_scan = frame_system::Pallet::<Runtime>::event_count();
		let (recorded, event_bytes_at_scan) =
			Self::record_proofs_from_events_since(event_count_before);

		// Two pieces of caller-influenced work here are invisible to the static
		// `weight()` and are therefore registered against the block post-hoc (this
		// keeps block-capacity accounting sound; it is not fee-charged):
		//
		// 1. The event scan itself: any call can emit events the scan must decode (e.g. batched
		//    `remark_with_event`), and the decode cost is per record AND per byte present at scan
		//    time — see `event_scan_weight`.
		//
		// 2. Recording shortfall: a stored inner call that is denser than the static bound, or a
		//    new opaque wrapper, can still emit more transfer events than `weight()` reserved. The
		//    flat per-transfer price times the count difference covers the block. `recover_funds`
		//    is charged its worst case up front, and `Multisig::execute` walks the stored call, so
		//    they refund rather than shortfall on the honest path.
		//
		// "Post-hoc and not fee-charged" is a deliberate, accepted trade-off, not an
		// oversight (security review 2026-08):
		//
		// - It cannot be fee-charged: the scan cost depends on how many events EARLIER transactions
		//   in the same block emitted, unknowable when the fee is computed.
		//   `TransactionExtension::weight()` is static by design, and post-dispatch fee correction
		//   only refunds downward — `actual_weight` is capped at the pre-charged weight. The only
		//   alternative, reserving a worst-case whole-block scan in every transaction upfront,
		//   would collapse throughput to insure against microseconds of work.
		//
		// - Block capacity stays sound: `register_extra_weight_unchecked` accrues into
		//   `BlockWeight` before the NEXT transaction's `CheckWeight` admission, so later
		//   transactions are refused once the block fills. The worst case is a bounded
		//   one-transaction overshoot at the block boundary (the same accepted pattern FRAME uses
		//   for `on_initialize` overruns).
		//
		// - The economics don't invert: emitting events is fully fee-charged through the emitting
		//   calls' benchmarked weights, and the uncharged decode here is ~1µs/record + ~10ns/byte —
		//   orders of magnitude below what the attacker pays to produce those events.
		let mut extra = Self::event_scan_weight(events_at_scan, event_bytes_at_scan);
		if recorded > charged_transfers {
			extra = extra.saturating_add(
				Self::per_transfer_weight()
					.saturating_mul(recorded.saturating_sub(charged_transfers)),
			);
		}
		if extra != Weight::zero() {
			frame_system::Pallet::<Runtime>::register_extra_weight_unchecked(extra, info.class);
		}

		// The converse of the shortfall: `weight()` reserves the worst case, and any
		// statically over-charged transfers are unspent — `recover_funds` is charged
		// `MaxPendingPerAccount + 1` regardless of how many holds were pending,
		// `Multisig::execute` walks the stored call (so a one-transfer proposal is
		// charged one, not a packed-blob guess), and a `batch_all` that fails after
		// some children still reserved weight for every child in the submitted call.
		// The per-transfer price is flat, so the unspent amount is exactly the count
		// difference times that price (and by construction never exceeds this
		// extension's declared weight).
		Ok(Self::per_transfer_weight().saturating_mul(charged_transfers.saturating_sub(recorded)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_support::{assert_ok, pallet_prelude::TransactionValidityError};
	use pallet_transaction_payment::WeightInfo;
	use sp_runtime::{traits::TxBaseImplication, AccountId32};
	fn alice() -> AccountId {
		AccountId32::from([1; 32])
	}

	fn bob() -> AccountId {
		AccountId32::from([2; 32])
	}
	fn charlie() -> AccountId {
		AccountId32::from([3; 32])
	}
	fn dave() -> AccountId {
		AccountId32::from([4; 32])
	}

	fn funded_threshold1_multisig() -> AccountId {
		let signers = vec![alice(), bob()];
		assert_ok!(Multisig::create_multisig(
			RuntimeOrigin::signed(alice()),
			signers.clone(),
			1,
			0,
		));
		let multisig_address =
			pallet_multisig::Pallet::<Runtime>::derive_multisig_address(&signers, 1, 0);
		assert_ok!(Balances::transfer_keep_alive(
			RuntimeOrigin::signed(alice()),
			MultiAddress::Id(multisig_address.clone()),
			EXISTENTIAL_DEPOSIT * 1000,
		));
		multisig_address
	}

	fn propose_inner(multisig_address: &AccountId, inner: RuntimeCall) {
		let encoded: pallet_multisig::BoundedCallOf<Runtime> =
			inner.encode().try_into().expect("test inner call fits MaxCallSize");
		assert_ok!(Multisig::propose(
			RuntimeOrigin::signed(alice()),
			multisig_address.clone(),
			encoded,
			System::block_number() + 100,
		));
	}

	// Build genesis storage according to the mock runtime.
	pub fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();

		pallet_balances::GenesisConfig::<Runtime> {
			balances: vec![
				(alice(), EXISTENTIAL_DEPOSIT * 10000),
				(bob(), EXISTENTIAL_DEPOSIT * 2),
				(charlie(), EXISTENTIAL_DEPOSIT * 100),
			],
			dev_accounts: None,
		}
		.assimilate_storage(&mut t)
		.unwrap();

		// high security account is charlie
		// guardian is alice
		pallet_reversible_transfers::GenesisConfig::<Runtime> {
			initial_high_security_accounts: vec![(charlie(), alice(), 10)],
		}
		.assimilate_storage(&mut t)
		.unwrap();

		// Treasury account is required for mining-reward fallback credits. It
		// must be explicit: the genesis default no longer configures anything (the old
		// default account was the keyless `[1u8; 32]` minting sentinel).
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(AccountId32::from([9u8; 32])),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		sp_io::TestExternalities::new(t)
	}

	fn run_scheduler_to(n: u32) {
		use frame_support::traits::{OnFinalize, OnInitialize};
		while System::block_number() < n {
			let b = System::block_number();
			Scheduler::on_finalize(b);
			System::set_block_number(b + 1);
			Scheduler::on_initialize(b + 1);
		}
	}

	fn newest_leaf() -> pallet_zk_tree::ZkLeaf<AccountId, AssetId, Balance> {
		let n = ZkTree::leaf_count();
		assert!(n > 0, "expected at least one leaf");
		pallet_zk_tree::Leaves::<Runtime>::get(n - 1).expect("newest leaf exists")
	}

	fn scan_since(event_count_before: u32) -> u64 {
		WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(
			event_count_before,
		)
		.0
	}

	#[test]
	fn test_reversible_transaction_extension() {
		new_test_ext().execute_with(|| {
			// Other calls should not be intercepted
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });

			let origin = RuntimeOrigin::signed(alice());
			let ext = ReversibleTransactionExtension::<Runtime>::new();

			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);

			// we should not fail here
			assert_ok!(result);

			// Test that non-high-security accounts can make balance transfers
			let ext = ReversibleTransactionExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Full lifecycle: validate decides the high-security status once and
			// prepare consumes it. Alice is not high-security, so this succeeds
			// and prepare reports the refundable (non-HS) path.
			let (_, val, _) = ext
				.clone()
				.validate(
					origin.clone(),
					&call,
					&Default::default(),
					0,
					(),
					&TxBaseImplication::<()>(()),
					frame_support::pallet_prelude::TransactionSource::External,
				)
				.expect("alice is not high-security");
			assert!(!val, "alice must be classified as non-high-security");
			let pre = ext.prepare(val, &origin, &call, &Default::default(), 0).unwrap();
			assert!(!pre);

			// Charlie is already configured as high-security from genesis
			// Verify Charlie is high-security
			assert!(ReversibleTransfers::is_high_security(&charlie()).is_some());

			// High-security accounts can call schedule_transfer
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);

			// Test the validate method
			let result = check_call(call);
			assert_ok!(result);

			// High-security accounts can call cancel
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			let result = check_call(call);
			assert_ok!(result);

			// All other calls are disallowed for high-security accounts
			// (use transfer_keep_alive - not in whitelist for prod or runtime-benchmarks)
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	// Run the reversible transaction extension's `validate` for `call` signed by `signer`.
	fn validate_with(
		signer: AccountId,
		call: &RuntimeCall,
	) -> Result<(), TransactionValidityError> {
		validate_with_len(signer, call, 0)
	}

	// As `validate_with`, but with an explicit encoded length so the length gate can
	// be exercised without building a multi-KiB extrinsic.
	fn validate_with_len(
		signer: AccountId,
		call: &RuntimeCall,
		len: usize,
	) -> Result<(), TransactionValidityError> {
		ReversibleTransactionExtension::<Runtime>::new()
			.validate(
				RuntimeOrigin::signed(signer),
				call,
				&Default::default(),
				len,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.map(|_| ())
	}

	// As `validate_with`, but with an explicit `DispatchInfo` so the fee ceiling
	// can be exercised against a hypothetical heavy-weight call.
	fn validate_with_info(
		signer: AccountId,
		call: &RuntimeCall,
		info: &frame_support::dispatch::DispatchInfo,
	) -> Result<(), TransactionValidityError> {
		ReversibleTransactionExtension::<Runtime>::new()
			.validate(
				RuntimeOrigin::signed(signer),
				call,
				info,
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.map(|_| ())
	}

	fn check_call(call: RuntimeCall) -> Result<(), TransactionValidityError> {
		// Verify Charlie is high-security
		assert!(ReversibleTransfers::is_high_security(&charlie()).is_some());

		let origin = RuntimeOrigin::signed(charlie());
		let ext = ReversibleTransactionExtension::<Runtime>::new();

		// Full lifecycle: validate classifies the signer, prepare records the quota.
		let (_, val, _) = ext.clone().validate(
			origin.clone(),
			&call,
			&Default::default(),
			0,
			(),
			&TxBaseImplication::<()>(()),
			frame_support::pallet_prelude::TransactionSource::External,
		)?;
		assert!(val, "charlie must be classified as high-security");
		ext.prepare(val, &origin, &call, &Default::default(), 0).map(|_| ())
	}

	#[test]
	fn test_high_security_transfer_keep_alive() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_transfer_allow_death() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_transfer_all() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_all {
				dest: MultiAddress::Id(bob()),
				keep_alive: true,
			});
			let result = check_call(call);

			// High-security accounts cannot make balance transfers
			assert_eq!(
				result.unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_schedule_transfer_allowed() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			// High-security accounts can call schedule_transfer
			assert_ok!(check_call(call));
		});
	}

	#[test]
	fn test_high_security_schedule_transfer_raw_dest_rejected() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Raw(vec![0u8; 1024]),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_schedule_transfer_address32_dest_rejected() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Address32([2u8; 32]),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_cancel_allowed() {
		new_test_ext().execute_with(|| {
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			assert_ok!(check_call(call));
		});
	}

	// A call that clears the whitelist is still rejected for a high-security signer
	// once the encoded extrinsic exceeds the length cap. The gate lives in
	// `validate` only, which is consensus-enforced: `dispatch_transaction` runs
	// it immediately before `prepare` during block execution.
	#[test]
	fn test_high_security_oversized_extrinsic_rejected() {
		new_test_ext().execute_with(|| {
			let cap = crate::configs::MAX_HIGH_SECURITY_EXTRINSIC_LEN as usize;
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			let too_large = TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_EXTRINSIC_TOO_LARGE,
			));
			assert_eq!(validate_with_len(charlie(), &call, cap + 1).unwrap_err(), too_large);
		});
	}

	// The cap is inclusive: an extrinsic exactly at the limit is accepted, so a
	// legitimate worst-case `batch_all` is never rejected for length.
	#[test]
	fn test_high_security_extrinsic_at_cap_allowed() {
		new_test_ext().execute_with(|| {
			let cap = crate::configs::MAX_HIGH_SECURITY_EXTRINSIC_LEN as usize;
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::schedule_transfer {
					dest: MultiAddress::Id(bob()),
					amount: 10 * EXISTENTIAL_DEPOSIT,
				},
			);
			assert_ok!(validate_with_len(charlie(), &call, cap));
		});
	}

	// Weight is the fee input the length cap cannot see: a whitelisted call
	// with a huge (e.g. future mis-benchmarked) weight must not reopen the
	// fee-drain channel for a high-security signer.
	#[test]
	fn test_high_security_overweight_extrinsic_rejected() {
		new_test_ext().execute_with(|| {
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			// ~2 UNIT of weight fee at IdentityFee — double the ceiling.
			let info = frame_support::dispatch::DispatchInfo {
				call_weight: Weight::from_parts(2_000_000_000_000, 0),
				..Default::default()
			};
			assert_eq!(
				validate_with_info(charlie(), &call, &info).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(
					HIGH_SECURITY_FEE_LIMIT_EXCEEDED
				))
			);
			// Normal signers are not fee-capped.
			assert_ok!(validate_with_info(alice(), &call, &info));
		});
	}

	/// Pins the declared storage weights to the executed footprint (enumerated
	/// in the `weight()` comment), so an edit to either side trips this test
	/// instead of silently under-declaring database work.
	#[test]
	fn weight_declarations_match_the_executed_storage_footprint() {
		let db = <Runtime as frame_system::Config>::DbWeight::get();
		let ext = ReversibleTransactionExtension::<Runtime>::new();
		let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });

		// High-security worst case: classification + `NextFeeMultiplier` +
		// quota-ring admission read in `validate`, ring read/write in
		// `prepare`. The quota helpers do not re-read
		// `HighSecurityAccounts`, so it is read exactly once.
		assert_eq!(
			<ReversibleTransactionExtension<Runtime> as TransactionExtension<RuntimeCall>>::weight(
				&ext, &call
			),
			db.reads_writes(4, 1)
		);

		// Non-high-security traffic executes only the classification read;
		// everything else is refunded.
		let refund = <ReversibleTransactionExtension<Runtime> as TransactionExtension<
			RuntimeCall,
		>>::post_dispatch_details(
			false, &Default::default(), &Default::default(), 0, &Ok(())
		)
		.unwrap();
		assert_eq!(refund, db.reads_writes(3, 1));

		// A tipped transaction additionally reads `HighSecurityAccounts` in
		// both `can_withdraw_fee` and `withdraw_fee` of the fee adapter.
		// Every paid extrinsic also mutates `CollectedFees` in the collector
		// (one unique-key read + write; the follow-on get is same-key).
		assert_eq!(
			<PaymentWeightsWithTipPolicy as pallet_transaction_payment::WeightInfo>::charge_transaction_payment(),
			pallet_transaction_payment::weights::SubstrateWeight::<Runtime>::charge_transaction_payment()
				.saturating_add(db.reads(2))
				.saturating_add(db.reads_writes(1, 1))
		);
	}

	// The zero-tip policy lives in the fee adapter, so it fires on every fee
	// path (mempool and consensus validation, and inclusion-time withdrawal)
	// regardless of how the extension tuple is composed.
	#[test]
	fn test_high_security_fee_adapter_rejects_tip() {
		use pallet_transaction_payment::OnChargeTransaction;
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
			let info = Default::default();
			let forbidden = TransactionValidityError::Invalid(InvalidTransaction::Custom(
				HIGH_SECURITY_TIP_FORBIDDEN,
			));

			// Charlie is high-security from genesis: any non-zero tip is refused
			// before funds move, on both the check and the withdrawal paths.
			assert_eq!(
				<HighSecurityFungibleAdapter as OnChargeTransaction<Runtime>>::can_withdraw_fee(
					&charlie(),
					&call,
					&info,
					10,
					10
				)
				.unwrap_err(),
				forbidden
			);
			assert_eq!(
				<HighSecurityFungibleAdapter as OnChargeTransaction<Runtime>>::withdraw_fee(
					&charlie(),
					&call,
					&info,
					10,
					10
				)
				.unwrap_err(),
				forbidden
			);

			// Zero tip from a high-security signer and any tip from a normal
			// signer both pass through to the inner adapter.
			assert_ok!(
				<HighSecurityFungibleAdapter as OnChargeTransaction<Runtime>>::can_withdraw_fee(
					&charlie(),
					&call,
					&info,
					10,
					0
				)
			);
			assert_ok!(
				<HighSecurityFungibleAdapter as OnChargeTransaction<Runtime>>::can_withdraw_fee(
					&alice(),
					&call,
					&info,
					10,
					5
				)
			);
		});
	}

	// Normal accounts are not length-capped: only high-security signers are.
	#[test]
	fn test_non_high_security_large_extrinsic_allowed() {
		new_test_ext().execute_with(|| {
			let cap = crate::configs::MAX_HIGH_SECURITY_EXTRINSIC_LEN as usize;
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			assert_ok!(validate_with_len(alice(), &call, cap * 4));
		});
	}

	#[test]
	fn test_high_security_batch_all_of_whitelisted_calls_is_allowed() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![
					RuntimeCall::ReversibleTransfers(
						pallet_reversible_transfers::Call::schedule_transfer {
							dest: MultiAddress::Id(bob()),
							amount: 10 * EXISTENTIAL_DEPOSIT,
						},
					),
					RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
						tx_id: Default::default(),
					}),
				],
			});
			assert_ok!(check_call(call));
		});
	}

	#[test]
	fn test_high_security_empty_batch_all_is_rejected() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all { calls: vec![] });
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_nested_batch_all_is_rejected() {
		new_test_ext().execute_with(|| {
			let inner = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![RuntimeCall::ReversibleTransfers(
					pallet_reversible_transfers::Call::cancel { tx_id: Default::default() },
				)],
			});
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all { calls: vec![inner] });
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_batch_all_rejects_more_than_max_batch_len_children() {
		new_test_ext().execute_with(|| {
			let max = crate::configs::MaxHighSecurityBatchLen::get() as usize;
			let child =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: Default::default(),
				});
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![child; max + 1],
			});
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_batch_all_rejects_raw_dest_child() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![RuntimeCall::ReversibleTransfers(
					pallet_reversible_transfers::Call::schedule_transfer {
						dest: MultiAddress::Raw(vec![0u8; 1024]),
						amount: 10 * EXISTENTIAL_DEPOSIT,
					},
				)],
			});
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_vesting_claim_is_rejected() {
		new_test_ext().execute_with(|| {
			// Deliberately not whitelisted: `claim` is permissionless, so a third
			// party can claim on the HS beneficiary's behalf; on the HS signer it
			// was only another no-op fee path.
			let call = RuntimeCall::Vesting(pallet_vesting::Call::claim { schedule_id: 0 });
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn test_high_security_batch_all_rejects_non_whitelisted_child() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![
					RuntimeCall::ReversibleTransfers(
						pallet_reversible_transfers::Call::schedule_transfer {
							dest: MultiAddress::Id(bob()),
							amount: 10 * EXISTENTIAL_DEPOSIT,
						},
					),
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(bob()),
						value: 10 * EXISTENTIAL_DEPOSIT,
					}),
				],
			});
			assert_eq!(
				check_call(call).unwrap_err(),
				TransactionValidityError::Invalid(InvalidTransaction::Custom(1))
			);
		});
	}

	#[test]
	fn high_security_account_can_dispatch_batch_all_of_schedule_transfers() {
		new_test_ext().execute_with(|| {
			assert_ok!(Utility::batch_all(
				RuntimeOrigin::signed(charlie()),
				vec![
					RuntimeCall::ReversibleTransfers(
						pallet_reversible_transfers::Call::schedule_transfer {
							dest: MultiAddress::Id(bob()),
							amount: 10 * EXISTENTIAL_DEPOSIT,
						},
					),
					RuntimeCall::ReversibleTransfers(
						pallet_reversible_transfers::Call::schedule_transfer {
							dest: MultiAddress::Id(bob()),
							amount: 11 * EXISTENTIAL_DEPOSIT,
						},
					),
				],
			));
			assert_eq!(ReversibleTransfers::next_transaction_id(), 2);
		});
	}

	// =========================================================================
	// Tests for event-based WormholeProofRecorderExtension
	// =========================================================================
	//
	// Note: The event-based approach records proofs by scanning Transfer events
	// in post_dispatch. The actual integration testing happens in the wormhole
	// pallet tests. Here we just verify the extension structure is correct.

	#[test]
	fn wormhole_proof_recorder_extension_has_correct_weight() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();

			// Non-transfer calls trigger no proof-recording work and carry no weight.
			let non_transfer =
				RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
			let base_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &non_transfer);
			assert_eq!(base_weight, Weight::zero());

			let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &transfer);
			// A transfer is charged the per-transfer proof-recording cost.
			assert!(weight.ref_time() > base_weight.ref_time());

			let batch = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(bob()),
						value: 50 * UNIT,
					}),
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(charlie()),
						value: 30 * UNIT,
					}),
				],
			});
			let batch_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &batch);
			assert!(batch_weight.ref_time() > weight.ref_time());
		});
	}

	#[test]
	fn wormhole_proof_recorder_ignores_vesting_calls_and_pot_events() {
		new_test_ext().execute_with(|| {
			// Vesting calls charge no extension weight: the pallet records its own
			// payouts (covering scheduler-enacted Root calls too) and its benchmarked
			// weights carry that cost, while the event scan skips pot-touching
			// transfers.
			for call in [
				RuntimeCall::Vesting(pallet_vesting::Call::claim { schedule_id: 0 }),
				RuntimeCall::Vesting(pallet_vesting::Call::create_schedule {
					beneficiary: alice(),
					start: 0,
					cliff: 0,
					end: 1,
					total: 1,
				}),
				RuntimeCall::Vesting(pallet_vesting::Call::end_schedule { schedule_id: 0 }),
				RuntimeCall::Vesting(pallet_vesting::Call::retarget_schedule {
					schedule_id: 0,
					new_beneficiary: alice(),
				}),
			] {
				assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&call), 0);
			}

			// And the event scan must not record pot-sourced payouts (the pallet
			// already did): a pot -> alice transfer event yields no new proof.
			System::set_block_number(1);
			let pot = pallet_vesting::Pallet::<Runtime>::pot_account_id();
			let count_before = Wormhole::transfer_count(&alice());
			System::deposit_event(RuntimeEvent::Balances(pallet_balances::Event::Transfer {
				from: pot,
				to: alice(),
				amount: EXISTENTIAL_DEPOSIT * 100,
			}));
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);
			assert_eq!(Wormhole::transfer_count(&alice()), count_before);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_reversible_cancel() {
		new_test_ext().execute_with(|| {
			// `ReversibleTransfers::cancel` is statically visible, so the proof reservation
			// is fee-charged rather than only reconciled post-hoc against block capacity.
			// Guardian cancel records one `TransferOnHold`; owner self-cancel records
			// zero (it `release`s). The count of one is the conservative reservation.
			let cancel =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: Default::default(),
				});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&cancel),
				1,
				"cancel seizes held funds via transfer_on_hold and must be charged one proof"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_execute_without_proposal_still_reserves_the_walk() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: alice(),
				proposal_id: 0,
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				0,
				"execute with no stored proposal must not reserve a packed-blob guess"
			);
			assert_eq!(
				<WormholeProofRecorderExtension<Runtime> as TransactionExtension<RuntimeCall>>::weight(
					&ext,
					&call
				),
				WormholeProofRecorderExtension::<Runtime>::execute_proposal_walk_weight()
					.saturating_mul(2),
				"a missing proposal still pays for the two proposal walks"
			);
		});
	}

	/// A 10 KiB no-transfer proposal plus a non-signer execute used to reserve
	/// zero extension weight, then refund that zero after the pallet rejected
	/// on the `Multisigs` read. The two proposal walks must stay charged.
	#[test]
	fn multisig_execute_failed_non_signer_keeps_proposal_walk_weight() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let multisig_address = funded_threshold1_multisig();
			let inner =
				RuntimeCall::System(frame_system::Call::remark { remark: vec![0u8; 8 * 1024] });
			propose_inner(&multisig_address, inner);

			let execute = RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: multisig_address.clone(),
				proposal_id: 0,
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&execute),
				0,
				"a remark proposal records no proofs"
			);
			let walk = WormholeProofRecorderExtension::<Runtime>::execute_proposal_walk_weight()
				.saturating_mul(2);
			let (info, post_info) = run_lifecycle_with_result(
				&charlie(),
				execute,
				Err(sp_runtime::DispatchError::BadOrigin),
				|| {
					assert!(
						Multisig::execute(RuntimeOrigin::signed(charlie()), multisig_address, 0,)
							.is_err(),
						"charlie is not a signer"
					);
				},
			);
			assert_eq!(info.extension_weight, walk);
			assert_eq!(
				post_info.actual_weight,
				Some(info.total_weight()),
				"failed execute must not refund the proposal-walk reservation"
			);
		});
	}

	/// A one-transfer stored call is charged exactly one transfer plus the
	/// proposal walks; there is no packed-blob over-reservation left to refund.
	#[test]
	fn multisig_execute_charges_the_stored_inner_call() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let multisig_address = funded_threshold1_multisig();
			propose_inner(
				&multisig_address,
				RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
					dest: MultiAddress::Id(charlie()),
					value: EXISTENTIAL_DEPOSIT * 100,
				}),
			);

			let execute = RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: multisig_address.clone(),
				proposal_id: 0,
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&execute),
				1,
				"execute must walk the stored transfer, not charge a MaxCallSize guess"
			);

			let (info, post_info) = run_lifecycle_with_result(&alice(), execute, Ok(()), || {
				assert_ok!(Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, 0,));
			});

			assert_eq!(Wormhole::transfer_count(&charlie()), 1);
			let walk = WormholeProofRecorderExtension::<Runtime>::execute_proposal_walk_weight()
				.saturating_mul(2);
			assert_eq!(
				info.extension_weight,
				walk.saturating_add(
					WormholeProofRecorderExtension::<Runtime>::per_transfer_weight()
				),
				"execute reserves the proposal walks plus the one stored transfer"
			);
			assert_eq!(
				post_info.actual_weight,
				Some(info.total_weight()),
				"a one-transfer stored call has no unused recorder reservation"
			);
		});
	}

	/// The old `MaxCallSize / 36` execute bound under-counted a stored
	/// `batch_all` of `recover_funds` (17 credits in 34 bytes) plus transfers.
	/// Walking the stored call must charge the composition so recorded <= charged.
	#[test]
	fn multisig_execute_mixed_recover_funds_and_transfers_records_at_most_charged() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let multisig_address = funded_threshold1_multisig();
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(dave()),
				EXISTENTIAL_DEPOSIT * 200,
			));
			assert_ok!(ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(dave()),
				qp_scheduler::BlockNumberOrTimestamp::BlockNumber(10),
				multisig_address.clone(),
			));

			let pending = 3u64;
			for _ in 0..pending {
				assert_ok!(ReversibleTransfers::schedule_transfer(
					RuntimeOrigin::signed(dave()),
					MultiAddress::Id(bob()),
					EXISTENTIAL_DEPOSIT * 10,
				));
			}

			let extra_transfers = 2u64;
			let mut calls = vec![RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::recover_funds { account: dave() },
			)];
			for _ in 0..extra_transfers {
				calls.push(RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
					dest: MultiAddress::Id(bob()),
					value: 1,
				}));
			}
			propose_inner(
				&multisig_address,
				RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }),
			);

			let execute = RuntimeCall::Multisig(pallet_multisig::Call::execute {
				multisig_address: multisig_address.clone(),
				proposal_id: 0,
			});
			let max_pending = u64::from(
				<Runtime as pallet_reversible_transfers::Config>::MaxPendingPerAccount::get(),
			);
			let charged = WormholeProofRecorderExtension::<Runtime>::count_transfers(&execute);
			assert_eq!(
				charged,
				max_pending + 1 + extra_transfers,
				"execute must walk recover_funds plus every stored transfer"
			);

			let leaves_before = ZkTree::leaf_count();
			run_lifecycle_with_result(&alice(), execute, Ok(()), || {
				assert_ok!(Multisig::execute(RuntimeOrigin::signed(alice()), multisig_address, 0,));
			});
			let recorded = ZkTree::leaf_count().saturating_sub(leaves_before);
			assert_eq!(
				recorded,
				pending + 1 + extra_transfers,
				"maxed-hold recover plus the extra transfers must each create a leaf"
			);
			assert!(
				recorded <= charged,
				"mixed recover_funds + transfers must not record more proofs than charged"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_recover_funds_at_worst_case() {
		new_test_ext().execute_with(|| {
			// `recover_funds` seizes every pending hold to the guardian (one `TransferOnHold`
			// each, up to `MaxPendingPerAccount`) and then sweeps the account with a
			// dispatched `transfer_all` (one `Transfer`). The realized count depends on
			// on-chain state, so the static charge must cover the worst case.
			let call = RuntimeCall::ReversibleTransfers(
				pallet_reversible_transfers::Call::recover_funds { account: charlie() },
			);
			let max_pending = u64::from(
				<Runtime as pallet_reversible_transfers::Config>::MaxPendingPerAccount::get(),
			);
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				max_pending + 1,
				"recover_funds must be charged for up to MaxPendingPerAccount hold seizures \
				 plus the transfer_all sweep"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_guardian_cancel_shortfall_is_fee_charged_not_block_only() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// charlie is high-security (guardian = alice, from genesis). Schedule a
			// transfer so there is a pending hold for the guardian to seize.
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(charlie()),
				MultiAddress::Id(bob()),
				EXISTENTIAL_DEPOSIT * 10,
			));
			let tx_id =
				pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(charlie())[0];

			let cancel =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id,
				});
			let guardian = alice();
			let count_before = Wormhole::transfer_count(&guardian);
			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			// Run the real guardian cancel through the extension lifecycle with the cancel
			// call itself presented to weight()/prepare().
			let scanned = core::cell::Cell::new(0u32);
			let scanned_bytes = core::cell::Cell::new(0u32);
			run_lifecycle(&guardian, cancel, || {
				assert_ok!(ReversibleTransfers::cancel(
					RuntimeOrigin::signed(guardian.clone()),
					tx_id
				));
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
				scanned_bytes.set(frame_system::Pallet::<Runtime>::event_bytes());
			});
			assert_eq!(
				Wormhole::transfer_count(&guardian),
				count_before + 1,
				"the seizure must have been recorded as a proof"
			);

			// The recorded proof was statically charged by weight(), so post_dispatch must
			// register ONLY the event-scan weight against the block — no proof-recording
			// shortfall may be shifted from the transaction fee to block capacity.
			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::event_scan_weight(
					scanned.get(),
					scanned_bytes.get()
				),
				"a statically visible cancel must have its proof insert fee-charged, \
				 leaving no shortfall to register against the block"
			);
		});
	}

	#[test]
	fn per_transfer_weight_includes_tree_hash_compute() {
		new_test_ext().execute_with(|| {
			// Recording a transfer inserts a ZK-tree leaf; the path update's Poseidon
			// hashing must be charged on top of the DB ops.
			let weight = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();

			let (tree_reads, tree_writes) = pallet_zk_tree::INSERT_LEAF_DB_OPS;
			let db_time = <Runtime as frame_system::Config>::DbWeight::get()
				.reads_writes(1u64.saturating_add(tree_reads), 1u64.saturating_add(tree_writes))
				.ref_time();
			assert!(
				weight.ref_time() >= db_time + pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS,
				"per-transfer weight must charge the leaf insert's Poseidon hashing \
				 on top of its DB ops"
			);
			// The leaf insert's path update also puts every tree key it reads into the
			// PoV; a recorded transfer that declares no proof size lets deep-tree
			// blocks exceed the PoV budget validators re-execute against.
			assert_eq!(
				weight.proof_size(),
				tree_reads.saturating_mul(pallet_zk_tree::TREE_KEY_POV),
				"per-transfer weight must charge PoV for the tree keys the insert reads"
			);
			// Recording also deposits events (LeafInserted + NativeTransferred), each an
			// `EventCount` read/write and an `Events` append. Those deposits happen after
			// the scan snapshot, so nothing downstream can charge them: the static
			// reservation must.
			let event_deposits = WormholeProofRecorderExtension::<Runtime>::event_deposit_weight(
				WormholeProofRecorderExtension::<Runtime>::EVENTS_PER_RECORDED_PROOF,
			);
			assert!(
				weight.ref_time() >=
					db_time +
						pallet_zk_tree::INSERT_LEAF_HASH_REF_TIME_PS +
						event_deposits.ref_time(),
				"per-transfer weight must charge the event deposits proof recording performs"
			);
		});
	}

	/// Pins the multiplier behind the modeled event-deposit charge: one recorded proof
	/// deposits exactly `EVENTS_PER_RECORDED_PROOF` events. If recording ever starts
	/// emitting more, the static reservation must be updated with it. (Capacity-boundary
	/// inserts additionally emit `TreeGrew`, deliberately unmodeled: it happens at most
	/// `MAX_TREE_DEPTH` times over the chain's whole life — the warm-up insert below
	/// steps the fresh test tree past the first boundary.)
	#[test]
	fn recording_one_proof_deposits_exactly_the_modeled_events() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let record_one_transfer = || {
				System::deposit_event(RuntimeEvent::Balances(pallet_balances::Event::Transfer {
					from: alice(),
					to: bob(),
					amount: EXISTENTIAL_DEPOSIT * 50,
				}));
				let scan_from = System::event_count() - 1;
				let events_before = System::event_count();
				let (recorded, _) =
					WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(
						scan_from,
					);
				assert_eq!(recorded, 1);
				u64::from(System::event_count() - events_before)
			};

			// Warm-up: the very first insert grows the empty tree and emits `TreeGrew`.
			record_one_transfer();

			assert_eq!(
				record_one_transfer(),
				WormholeProofRecorderExtension::<Runtime>::EVENTS_PER_RECORDED_PROOF,
				"the modeled events-per-proof multiplier must match what recording deposits"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_registers_extra_weight_for_uncounted_transfers() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// The presented call is opaque to the static matcher (a `remark` has no
			// transfer children), but the dispatch emits a real transfer event that
			// post_dispatch must record.
			let opaque_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&opaque_call), 0);

			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			let scanned = core::cell::Cell::new(0u32);
			let scanned_bytes = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), opaque_call, || {
				assert_ok!(Balances::transfer_keep_alive(
					RuntimeOrigin::signed(alice()),
					MultiAddress::Id(bob()),
					EXISTENTIAL_DEPOSIT * 50,
				));
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
				scanned_bytes.set(frame_system::Pallet::<Runtime>::event_bytes());
			});

			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::per_transfer_weight().saturating_add(
					WormholeProofRecorderExtension::<Runtime>::event_scan_weight(
						scanned.get(),
						scanned_bytes.get()
					)
				),
				"the uncounted recorded transfer must be registered as extra block weight, \
				 on top of the always-registered event-scan weight"
			);
		});
	}

	/// The post-dispatch scan streams `System::Events` through a decoding iterator —
	/// and `skip()` still decodes the records it discards — so every event record
	/// present at scan time costs decode work even when nothing is recorded. A signed
	/// caller can emit arbitrarily many events with zero-transfer calls (e.g. batched
	/// `remark_with_event`), so that work must be registered against the block.
	#[test]
	fn wormhole_proof_recorder_registers_event_scan_weight() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&call), 0);

			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			// Capture the event count and encoded size at the end of the dispatch
			// closure: that is exactly what the post-dispatch scan decodes.
			let scanned = core::cell::Cell::new(0u32);
			let scanned_bytes = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), call, || {
				for i in 0..7u8 {
					assert_ok!(System::remark_with_event(RuntimeOrigin::signed(alice()), vec![i],));
				}
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
				scanned_bytes.set(frame_system::Pallet::<Runtime>::event_bytes());
			});
			assert!(scanned.get() >= 7, "the remarks must have emitted events");

			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::event_scan_weight(
					scanned.get(),
					scanned_bytes.get()
				),
				"the per-event decode work of the scan must be registered as block weight"
			);
		});
	}

	/// `weight()` reserves the static worst case, so a dispatch that performs fewer
	/// proof inserts than charged (`batch_all` of two transfers that only records one,
	/// `recover_funds` with fewer pending holds than `MaxPendingPerAccount`) must have
	/// the difference refunded via `post_dispatch_details`, not kept forever.
	#[test]
	fn statically_overcharged_transfers_are_refunded_post_dispatch() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// A batch_all of two transfers is charged two per-transfer reservations.
			// Simulate a dispatch that only completed the first transfer.
			let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
				dest: MultiAddress::Id(bob()),
				value: 10 * EXISTENTIAL_DEPOSIT,
			});
			let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
				calls: vec![transfer.clone(), transfer],
			});
			assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&call), 2);

			let (info, post_info) = run_lifecycle_with_result(&alice(), call, Ok(()), || {
				assert_ok!(Balances::transfer_keep_alive(
					RuntimeOrigin::signed(alice()),
					MultiAddress::Id(bob()),
					EXISTENTIAL_DEPOSIT * 50,
				));
			});

			let per_transfer = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();
			assert_eq!(
				post_info.actual_weight,
				Some(info.total_weight().saturating_sub(per_transfer)),
				"the unused second per-transfer reservation must be refunded"
			);
		});
	}

	/// A failed dispatch rolls back its events: nothing is scanned or recorded, so the
	/// entire static per-transfer reservation is unspent and must be refunded.
	#[test]
	fn failed_dispatch_refunds_the_entire_static_transfer_charge() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: EXISTENTIAL_DEPOSIT * 50,
			});
			let count_before = Wormhole::transfer_count(&bob());
			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			let (info, post_info) = run_lifecycle_with_result(
				&alice(),
				call,
				Err(sp_runtime::DispatchError::BadOrigin),
				|| {},
			);

			let per_transfer = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();
			assert_eq!(
				post_info.actual_weight,
				Some(info.total_weight().saturating_sub(per_transfer)),
				"a failed dispatch records nothing, so its full static charge is unspent"
			);
			assert_eq!(Wormhole::transfer_count(&bob()), count_before, "nothing recorded");
			assert_eq!(
				frame_system::Pallet::<Runtime>::block_weight().total(),
				weight_before,
				"no scan runs on failure, so no extra weight is registered"
			);
		});
	}

	/// The scan stream-decodes complete records before filtering, and record size is
	/// caller-influenced: a successful `Multisig::execute` emits `ProposalExecuted`
	/// carrying the full stored call (up to 10 KiB) and the approver vector (up to 100
	/// accounts) — an order of magnitude past the ~300-byte structural assumption
	/// behind the per-record charge. Because `skip()` still decodes discarded records,
	/// every later transaction in the block re-decodes such records too. The registered
	/// scan weight must therefore scale with the bytes present, not record count alone.
	#[test]
	fn wormhole_proof_recorder_scan_weight_scales_with_event_bytes() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			let bytes_at_scan = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), call, || {
				// A worst-case legitimate record: full-size stored call, max approvers.
				System::deposit_event(RuntimeEvent::Multisig(
					pallet_multisig::Event::ProposalExecuted {
						multisig_address: alice(),
						proposal_id: 0,
						proposer: alice(),
						call: vec![0u8; 10_240],
						approvers: vec![alice(); 100],
						result: Ok(()),
					},
				));
				bytes_at_scan.set(frame_system::Pallet::<Runtime>::event_bytes());
			});
			assert!(
				bytes_at_scan.get() > 10_240,
				"the oversized record must be in the scanned stream"
			);

			let registered = frame_system::Pallet::<Runtime>::block_weight()
				.total()
				.saturating_sub(weight_before);
			let byte_floor =
				WormholeProofRecorderExtension::<Runtime>::EVENT_SCAN_DECODE_BYTE_REF_TIME_PS
					.saturating_mul(u64::from(bytes_at_scan.get()));
			assert!(
				registered.ref_time() >= byte_floor,
				"scan weight must cover byte-proportional decode work: registered {} < byte floor {}",
				registered.ref_time(),
				byte_floor,
			);
		});
	}

	/// Pins the pipeline mechanics the refund depends on: `CheckWeight` reclaims block
	/// weight BEFORE this extension's refund exists, so the trailing
	/// `frame_system::WeightReclaim` in `TxExtension` is what actually returns the
	/// refund to block capacity (idempotently, via `ExtrinsicWeightReclaimed`).
	#[test]
	fn trailing_weight_reclaim_returns_the_refund_to_block_capacity() {
		use frame_system::{CheckWeight, WeightReclaim};
		use sp_runtime::traits::{ExtensionPostDispatchWeightHandler, TxBaseImplication};

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Presented call is charged one per-transfer reservation; the dispatch
			// performs no transfer, so the whole reservation is unspent.
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: EXISTENTIAL_DEPOSIT * 50,
			});
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let ext_weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &call);
			let info = frame_support::dispatch::DispatchInfo {
				call_weight: Weight::from_parts(1_000_000, 0),
				extension_weight: ext_weight,
				..Default::default()
			};

			// Admission: CheckWeight accrues the full declared weight.
			let (_, next_len) = CheckWeight::<Runtime>::do_validate(&info, 0).unwrap();
			assert_ok!(CheckWeight::<Runtime>::do_prepare(&info, 0, next_len));
			let admitted = frame_system::Pallet::<Runtime>::block_weight().total();

			let origin = RuntimeOrigin::signed(alice());
			let (_, val, _) = ext
				.validate(
					origin.clone(),
					&call,
					&info,
					0,
					(),
					&TxBaseImplication::<()>(()),
					frame_support::pallet_prelude::TransactionSource::External,
				)
				.expect("validate should succeed");
			let pre = ext
				.clone()
				.prepare(val, &origin, &call, &info, 0)
				.expect("prepare should succeed");

			// (dispatch performs no transfer)

			// Post-dispatch in real pipeline order.
			let mut post_info = frame_support::dispatch::PostDispatchInfo::default();
			post_info.set_extension_weight(&info);
			let events_at_scan = frame_system::Pallet::<Runtime>::event_count();
			let event_bytes_at_scan = frame_system::Pallet::<Runtime>::event_bytes();
			assert_ok!(CheckWeight::<Runtime>::post_dispatch_details(
				(),
				&info,
				&post_info,
				0,
				&Ok(())
			));
			<WormholeProofRecorderExtension<Runtime> as TransactionExtension<RuntimeCall>>::post_dispatch(
				pre,
				&info,
				&mut post_info,
				0,
				&Ok(()),
			)
			.expect("post_dispatch should succeed");
			assert_ok!(WeightReclaim::<Runtime>::post_dispatch_details(
				(),
				&info,
				&post_info,
				0,
				&Ok(())
			));

			// The refunded per-transfer reservation left block capacity; the (post-hoc)
			// event-scan registration is the only addition.
			let per_transfer = WormholeProofRecorderExtension::<Runtime>::per_transfer_weight();
			let scan = WormholeProofRecorderExtension::<Runtime>::event_scan_weight(
				events_at_scan,
				event_bytes_at_scan,
			);
			assert_eq!(
				frame_system::Pallet::<Runtime>::block_weight().total(),
				admitted.saturating_sub(per_transfer).saturating_add(scan),
				"the trailing WeightReclaim must return the wormhole refund to block capacity"
			);
		});
	}

	/// The scan must decode exactly what the streaming reader would: the single-copy
	/// reader exists purely to make the scan linear (one `storage::get` instead of
	/// per-2-KiB refills that each re-materialize the whole overlay value), not to
	/// change what is read.
	#[test]
	fn single_copy_event_reader_matches_the_streaming_reader() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// A mixed stream: small records, a transfer, and an oversized record.
			assert_ok!(System::remark_with_event(RuntimeOrigin::signed(alice()), vec![1]));
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				EXISTENTIAL_DEPOSIT * 50,
			));
			System::deposit_event(RuntimeEvent::Multisig(
				pallet_multisig::Event::ProposalExecuted {
					multisig_address: alice(),
					proposal_id: 0,
					proposer: alice(),
					call: vec![0u8; 10_240],
					approvers: vec![alice(); 100],
					result: Ok(()),
				},
			));

			let streamed: alloc::vec::Vec<_> =
				frame_system::Pallet::<Runtime>::read_events_no_consensus()
					.map(|boxed| *boxed)
					.collect();
			let (bytes, single_copy) =
				frame_system::Pallet::<Runtime>::read_events_no_consensus_single_copy();
			let single_copy: alloc::vec::Vec<_> = single_copy.collect();

			assert!(!streamed.is_empty(), "the fixture must have produced events");
			assert_eq!(
				streamed, single_copy,
				"the single-copy reader must decode the identical records"
			);
			assert_eq!(
				bytes,
				frame_system::Pallet::<Runtime>::event_bytes(),
				"the returned size must be the encoded length of the Events value"
			);
		});
	}

	/// `stream_iter()` refills its 2 KiB buffer via `sp_io::storage::read`, and each
	/// such host call materializes the complete overlay-resident `Events` value before
	/// slicing out the window — O(bytes²) total copying for a full scan, which the
	/// linear per-byte charge can never bound (review measured 16× bytes → ~69× time on
	/// that path). The scan therefore reads the value once and decodes in memory; this
	/// pins the linear charge against the many-small-record workload (e.g. nested
	/// batches of `remark_with_event`) that made the quadratic path reachable.
	#[test]
	fn wormhole_proof_recorder_scan_of_many_small_records_registers_formula_weight() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			let scanned = core::cell::Cell::new(0u32);
			let scanned_bytes = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), call, || {
				for _ in 0..20_000u32 {
					System::deposit_event(RuntimeEvent::System(frame_system::Event::Remarked {
						sender: alice(),
						hash: Default::default(),
					}));
				}
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
				scanned_bytes.set(frame_system::Pallet::<Runtime>::event_bytes());
			});
			assert!(scanned.get() >= 20_000, "the deposits must be in the scanned stream");

			let registered = frame_system::Pallet::<Runtime>::block_weight()
				.total()
				.saturating_sub(weight_before);
			assert_eq!(
				registered,
				WormholeProofRecorderExtension::<Runtime>::event_scan_weight(
					scanned.get(),
					scanned_bytes.get()
				),
				"a scan across tens of thousands of small records must register exactly \
				 the per-record + per-byte formula weight"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_extension_prepare_succeeds() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Prepare should succeed and return current event count
			let charged = WormholeProofRecorderExtension::<Runtime>::count_transfers(&call);
			let result = ext.prepare(charged, &origin, &call, &Default::default(), 0);
			assert_ok!(result);
		});
	}

	#[test]
	fn wormhole_proof_recorder_extension_validate_succeeds() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();
			let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(bob()),
				value: 100 * UNIT,
			});
			let origin = RuntimeOrigin::signed(alice());

			// Validate should always succeed (no validation needed)
			use sp_runtime::traits::TxBaseImplication;
			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			assert_ok!(result);
		});
	}

	// =========================================================================
	// Integration tests for event-based transfer proof recording
	// =========================================================================
	//
	// These tests verify that transfers via various paths result in proofs
	// being recorded. We simulate what post_dispatch does by:
	// 1. Executing the transfer (which emits events)
	// 2. Calling record_proofs_from_events_since(0) directly
	// 3. Verifying proofs were recorded in wormhole storage

	#[test]
	fn event_based_proof_recording_native_transfer() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use a smaller amount
			let transfer_amount = EXISTENTIAL_DEPOSIT * 100;
			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);
			let leaves_before = ZkTree::leaf_count();
			let events_before = frame_system::Pallet::<Runtime>::event_count();

			// Execute a transfer (this emits pallet_balances::Event::Transfer)
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				transfer_amount,
			));

			assert_eq!(scan_since(events_before), 1, "a plain transfer records exactly one proof");
			assert_eq!(ZkTree::leaf_count(), leaves_before + 1);
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
			let leaf = newest_leaf();
			assert_eq!(leaf.to, bob_account);
			assert_eq!(leaf.amount, transfer_amount);
		});
	}

	#[test]
	fn event_based_proof_recording_transfer_allow_death() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use a smaller amount
			let transfer_amount = EXISTENTIAL_DEPOSIT * 50;
			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);

			// Execute transfer_allow_death
			assert_ok!(Balances::transfer_allow_death(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				transfer_amount,
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_transfer_all() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let count_before = Wormhole::transfer_count(&bob_account);

			// Execute transfer_all (transfers entire balance minus ED)
			assert_ok!(Balances::transfer_all(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				false, // keep_alive = false
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_batch_all_transfers() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let charlie_account = charlie();
			let bob_count_before = Wormhole::transfer_count(&bob_account);
			let charlie_count_before = Wormhole::transfer_count(&charlie_account);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use smaller amounts
			// Execute a batch_all with multiple transfers
			assert_ok!(Utility::batch_all(
				RuntimeOrigin::signed(alice()),
				vec![
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(bob()),
						value: EXISTENTIAL_DEPOSIT * 50,
					}),
					RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
						dest: MultiAddress::Id(charlie()),
						value: EXISTENTIAL_DEPOSIT * 30,
					}),
				],
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify both transfers were recorded (proofs are now in ZK trie)
			assert_eq!(Wormhole::transfer_count(&bob_account), bob_count_before + 1);
			assert_eq!(Wormhole::transfer_count(&charlie_account), charlie_count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_guardian_seizure_via_transfer_on_hold() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let amount = EXISTENTIAL_DEPOSIT * 10;
			let guardian = alice();
			let count_before = Wormhole::transfer_count(&guardian);
			let leaves_before = ZkTree::leaf_count();

			// charlie is high-security (guardian = alice, from genesis); scheduling a
			// transfer places the funds on hold.
			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(charlie()),
				MultiAddress::Id(bob()),
				amount,
			));
			let tx_id =
				pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(charlie())[0];

			// The guardian cancels: the held funds (minus the volume fee) are seized to
			// the guardian via `transfer_on_hold`, which emits `Balances::TransferOnHold`
			// — not a free-balance `Transfer`. The credit is real spendable value landing
			// on the guardian's free balance, so the recorder must create a leaf for it
			// exactly as it would for a plain transfer. The pallet itself must not also
			// call ProofRecorder (that would double-insert).
			let events_before = frame_system::Pallet::<Runtime>::event_count();
			assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(guardian.clone()), tx_id));

			assert_eq!(
				scan_since(events_before),
				1,
				"hold-transfer seizure must record exactly one proof"
			);
			assert_eq!(ZkTree::leaf_count(), leaves_before + 1);
			assert_eq!(Wormhole::transfer_count(&guardian), count_before + 1);
			let leaf = newest_leaf();
			assert_eq!(leaf.to, guardian);
			let fee = <Runtime as pallet_reversible_transfers::Config>::VolumeFee::get() * amount;
			assert_eq!(leaf.amount, amount - fee);
		});
	}

	#[test]
	fn event_based_proof_recording_owner_self_cancel_is_not_a_credit() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let amount = EXISTENTIAL_DEPOSIT * 10;
			let sender = alice();
			let dest = bob();
			let leaves_before = ZkTree::leaf_count();
			let count_before = Wormhole::transfer_count(&sender);

			assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(sender.clone()),
				MultiAddress::Id(dest),
				amount,
				qp_scheduler::BlockNumberOrTimestamp::BlockNumber(2),
			));
			let tx_id =
				pallet_reversible_transfers::PendingTransfersBySender::<Runtime>::get(&sender)[0];

			let events_before = frame_system::Pallet::<Runtime>::event_count();
			assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(sender.clone()), tx_id));

			assert_eq!(
				scan_since(events_before),
				0,
				"owner self-cancel releases the hold to the sender and must not record a leaf"
			);
			assert_eq!(ZkTree::leaf_count(), leaves_before);
			assert_eq!(Wormhole::transfer_count(&sender), count_before);
		});
	}

	#[test]
	fn event_based_proof_recording_self_transfer_is_not_a_credit() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let leaves_before = ZkTree::leaf_count();
			let events_before = frame_system::Pallet::<Runtime>::event_count();
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(alice()),
				EXISTENTIAL_DEPOSIT * 10,
			));

			assert_eq!(
				scan_since(events_before),
				0,
				"a self-directed transfer_keep_alive emits no Transfer and records no leaf"
			);
			assert_eq!(ZkTree::leaf_count(), leaves_before);
		});
	}

	#[test]
	fn event_based_proof_recording_self_directed_transfer_on_hold_is_dropped() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let leaves_before = ZkTree::leaf_count();
			let events_before = frame_system::Pallet::<Runtime>::event_count();
			System::deposit_event(RuntimeEvent::Balances(pallet_balances::Event::TransferOnHold {
				reason: pallet_reversible_transfers::HoldReason::ScheduledTransfer.into(),
				source: alice(),
				dest: alice(),
				amount: EXISTENTIAL_DEPOSIT * 10,
			}));

			assert_eq!(
				scan_since(events_before),
				0,
				"a self-directed TransferOnHold is not a credit and must be dropped at the chokepoint"
			);
			assert_eq!(ZkTree::leaf_count(), leaves_before);
		});
	}

	#[test]
	fn recover_funds_records_one_proof_per_real_credit() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let amount = EXISTENTIAL_DEPOSIT * 10;
			let guardian = alice();
			let leaves_before = ZkTree::leaf_count();
			let guardian_count_before = Wormhole::transfer_count(&guardian);

			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(charlie()),
				MultiAddress::Id(bob()),
				amount,
			));

			let events_before = frame_system::Pallet::<Runtime>::event_count();
			assert_ok!(ReversibleTransfers::recover_funds(
				RuntimeOrigin::signed(guardian.clone()),
				charlie(),
			));

			// One TransferOnHold (pending seizure) plus one Transfer (free-balance sweep).
			// The pallet must not also call ProofRecorder on the hold path, or this
			// would be 3.
			assert_eq!(
				scan_since(events_before),
				2,
				"recover_funds records exactly one proof per real credit"
			);
			assert_eq!(ZkTree::leaf_count(), leaves_before + 2);
			assert_eq!(Wormhole::transfer_count(&guardian), guardian_count_before + 2);
		});
	}

	#[test]
	fn scheduled_execution_records_exactly_one_proof() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let amount = EXISTENTIAL_DEPOSIT * 10;
			let dest = bob();
			let dest_count_before = Wormhole::transfer_count(&dest);
			let leaves_before = ZkTree::leaf_count();

			assert_ok!(ReversibleTransfers::schedule_transfer(
				RuntimeOrigin::signed(charlie()),
				MultiAddress::Id(dest.clone()),
				amount,
			));
			assert_eq!(
				ZkTree::leaf_count(),
				leaves_before,
				"scheduling holds funds and must not record a leaf"
			);

			// Charlie's genesis delay is 10 blocks; schedule at block 1 executes at 11.
			run_scheduler_to(11);

			assert_eq!(
				ZkTree::leaf_count(),
				leaves_before + 1,
				"scheduled execution records exactly one leaf via ProofRecorder"
			);
			assert_eq!(Wormhole::transfer_count(&dest), dest_count_before + 1);
			let leaf = newest_leaf();
			assert_eq!(leaf.to, dest);
			assert_eq!(leaf.amount, amount);

			// Hook-context events sit below every extrinsic's prepare() snapshot, so a
			// later scan must not double-record the inner transfer_keep_alive.
			let snapshot = frame_system::Pallet::<Runtime>::event_count();
			assert_eq!(scan_since(snapshot), 0);
			assert_eq!(ZkTree::leaf_count(), leaves_before + 1);
		});
	}

	#[test]
	fn self_directed_scheduled_execution_records_no_proof() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let sender = alice();
			let amount = EXISTENTIAL_DEPOSIT * 10;
			let leaves_before = ZkTree::leaf_count();
			let count_before = Wormhole::transfer_count(&sender);
			let free_before = Balances::free_balance(&sender);

			assert_ok!(ReversibleTransfers::schedule_transfer_with_delay(
				RuntimeOrigin::signed(sender.clone()),
				MultiAddress::Id(sender.clone()),
				amount,
				qp_scheduler::BlockNumberOrTimestamp::BlockNumber(2),
			));

			run_scheduler_to(3);

			assert_eq!(
				ZkTree::leaf_count(),
				leaves_before,
				"a self-directed scheduled execution moves no value and must not record a leaf"
			);
			assert_eq!(Wormhole::transfer_count(&sender), count_before);
			assert_eq!(Balances::free_balance(&sender), free_before);

			let snapshot = frame_system::Pallet::<Runtime>::event_count();
			assert_eq!(scan_since(snapshot), 0);
			assert_eq!(ZkTree::leaf_count(), leaves_before);
		});
	}

	#[test]
	fn event_based_proof_recording_reserve_repatriation() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let count_before = Wormhole::transfer_count(&alice());
			let events_before = frame_system::Pallet::<Runtime>::event_count();

			// `repatriate_reserved` emits `ReserveRepatriated` instead of `Transfer`.
			// The credit is real spendable value landing on alice, so the recorder
			// must create a leaf for it.
			System::deposit_event(RuntimeEvent::Balances(
				pallet_balances::Event::ReserveRepatriated {
					from: bob(),
					to: alice(),
					amount: EXISTENTIAL_DEPOSIT * 100,
					destination_status: frame_support::traits::tokens::BalanceStatus::Free,
				},
			));

			let (recorded, _) =
				WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(
					events_before,
				);

			assert_eq!(recorded, 1, "reserve repatriation must be recorded as a transfer proof");
			assert_eq!(Wormhole::transfer_count(&alice()), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_no_proof_for_non_transfer() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let bob_count_before = Wormhole::transfer_count(&bob_account);

			// Execute a non-transfer call
			assert_ok!(System::remark(RuntimeOrigin::signed(alice()), vec![1, 2, 3]));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify no proofs were recorded
			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_before,
				"No transfer count should change for non-transfer calls"
			);
		});
	}

	#[test]
	fn event_based_proof_recording_minted_event() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Create a new account to receive minted tokens
			let recipient = AccountId::from([99u8; 32]);
			let mint_amount = 1000 * UNIT;
			let count_before = Wormhole::transfer_count(&recipient);

			// Mint tokens directly; this emits pallet_balances::Event::Minted.
			use frame_support::traits::fungible::Mutate as _;
			assert_ok!(Balances::mint_into(&recipient, mint_amount));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// The Minted event is scanned; the proof uses MintingAccount as 'from'.
			let count_after = Wormhole::transfer_count(&recipient);
			assert!(count_after >= count_before, "Transfer count should not decrease");
		});
	}

	// =========================================================================
	// Regression test: multiple txs in one block must NOT duplicate proofs
	// =========================================================================
	//
	// Before the event_count snapshot fix, record_proofs_from_events scanned
	// ALL events in the block. The second tx's post_dispatch would re-process
	// the first tx's Transfer event, creating a duplicate proof. This test
	// simulates that exact scenario and asserts exactly 1 proof per transfer.

	#[test]
	fn no_duplicate_proofs_across_transactions_in_same_block() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let charlie_account = charlie();
			let bob_count_start = Wormhole::transfer_count(&bob_account);
			let charlie_count_start = Wormhole::transfer_count(&charlie_account);

			// --- Tx 1: Alice sends to Bob ---
			let snapshot_1 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				EXISTENTIAL_DEPOSIT * 50,
			));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_1);

			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx1: Bob should have exactly 1 new proof"
			);

			// --- Tx 2: Alice sends to Charlie ---
			let snapshot_2 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(charlie()),
				EXISTENTIAL_DEPOSIT * 30,
			));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_2);

			assert_eq!(
				Wormhole::transfer_count(&charlie_account),
				charlie_count_start + 1,
				"Tx2: Charlie should have exactly 1 new proof"
			);
			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx2 must NOT re-record Bob's proof from Tx1"
			);

			// --- Tx 3: a non-transfer tx should not create any proofs ---
			let snapshot_3 = frame_system::Pallet::<Runtime>::event_count();

			assert_ok!(System::remark(RuntimeOrigin::signed(alice()), vec![0xCA, 0xFE]));

			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(snapshot_3);

			assert_eq!(
				Wormhole::transfer_count(&bob_account),
				bob_count_start + 1,
				"Tx3: Bob count unchanged after non-transfer tx"
			);
			assert_eq!(
				Wormhole::transfer_count(&charlie_account),
				charlie_count_start + 1,
				"Tx3: Charlie count unchanged after non-transfer tx"
			);
		});
	}

	// =========================================================================
	// Tests for multisig transfer proof recording
	// =========================================================================

	#[test]
	fn event_based_proof_recording_multisig_transfer() {
		use codec::Encode;

		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// Create a multisig with alice and bob as signers, threshold 2
			let signers = vec![alice(), bob()];
			let threshold = 2u32;
			let nonce = 0u64;

			// Create the multisig
			assert_ok!(Multisig::create_multisig(
				RuntimeOrigin::signed(alice()),
				signers.clone(),
				threshold,
				nonce,
			));

			// Derive the multisig address
			let multisig_address = pallet_multisig::Pallet::<Runtime>::derive_multisig_address(
				&signers, threshold, nonce,
			);

			// Fund the multisig account
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(multisig_address.clone()),
				EXISTENTIAL_DEPOSIT * 1000,
			));

			// Clear events from setup
			System::reset_events();

			// Create a proposal to transfer from multisig to charlie
			let transfer_amount = EXISTENTIAL_DEPOSIT * 100;
			let inner_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
				dest: MultiAddress::Id(charlie()),
				value: transfer_amount,
			});

			// Encode the call and set expiry
			let encoded_call: pallet_multisig::BoundedCallOf<Runtime> =
				inner_call.encode().try_into().unwrap();
			let expiry = System::block_number() + 100;

			// Alice proposes
			assert_ok!(Multisig::propose(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				encoded_call.clone(),
				expiry,
			));

			// Bob approves (reaches threshold), resubmitting the proposal's call
			assert_ok!(Multisig::approve(
				RuntimeOrigin::signed(bob()),
				multisig_address.clone(),
				0, // proposal_id
				encoded_call,
			));

			// Get charlie's transfer count before execution
			let charlie_account = charlie();
			let count_before = Wormhole::transfer_count(&charlie_account);

			// Execute the proposal
			assert_ok!(Multisig::execute(
				RuntimeOrigin::signed(alice()),
				multisig_address.clone(),
				0, // proposal_id
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			// The transfer is FROM the multisig address
			let count_after = Wormhole::transfer_count(&charlie_account);
			assert_eq!(
				count_after,
				count_before + 1,
				"Transfer count should increment for multisig transfer"
			);
		});
	}

	/// Like [`run_lifecycle`], but mirrors the pipeline's post-dispatch weight handling:
	/// `post_info.set_extension_weight` runs before `post_dispatch` (as
	/// `dispatch_transaction` does), so refunds made by the extension are visible in the
	/// returned `PostDispatchInfo`. `result` is the dispatch result presented to
	/// `post_dispatch`.
	fn run_lifecycle_with_result(
		from: &AccountId,
		call: RuntimeCall,
		result: DispatchResult,
		dispatch: impl FnOnce(),
	) -> (frame_support::dispatch::DispatchInfo, frame_support::dispatch::PostDispatchInfo) {
		use sp_runtime::traits::{ExtensionPostDispatchWeightHandler, TxBaseImplication};

		let ext = WormholeProofRecorderExtension::<Runtime>::new();
		let info = frame_support::dispatch::DispatchInfo {
			call_weight: Weight::from_parts(1_000_000, 0),
			extension_weight: <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &call),
			..Default::default()
		};
		let origin = RuntimeOrigin::signed(from.clone());

		let (_, val, _) = ext
			.validate(
				origin.clone(),
				&call,
				&info,
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.expect("validate should succeed");
		let pre = ext
			.clone()
			.prepare(val, &origin, &call, &info, 0)
			.expect("prepare should succeed");

		dispatch();

		let mut post_info = frame_support::dispatch::PostDispatchInfo::default();
		post_info.set_extension_weight(&info);
		<WormholeProofRecorderExtension<Runtime> as TransactionExtension<RuntimeCall>>::post_dispatch(
			pre,
			&info,
			&mut post_info,
			0,
			&result,
		)
		.expect("post_dispatch should succeed");
		(info, post_info)
	}

	/// Run a full transaction (whatever `dispatch` performs) through the extension lifecycle.
	/// `call` is the call presented to validate/prepare; `dispatch` performs the real execution.
	fn run_lifecycle(from: &AccountId, call: RuntimeCall, dispatch: impl FnOnce()) {
		use sp_runtime::traits::TxBaseImplication;

		let ext = WormholeProofRecorderExtension::<Runtime>::new();
		let origin = RuntimeOrigin::signed(from.clone());

		let (_, val, _) = ext
			.validate(
				origin.clone(),
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			)
			.expect("validate should succeed");

		// prepare(): snapshot the event count before the call emits its Transfer event(s).
		let pre = ext
			.clone()
			.prepare(val, &origin, &call, &Default::default(), 0)
			.expect("prepare should succeed");

		// Execute the real call (emits the Transfer event(s) that post_dispatch scans).
		dispatch();

		// post_dispatch(): records the transfer proof(s).
		let mut post_info = frame_support::dispatch::PostDispatchInfo::default();
		<WormholeProofRecorderExtension<Runtime> as TransactionExtension<RuntimeCall>>::post_dispatch(
			pre,
			&Default::default(),
			&mut post_info,
			0,
			&Ok(()),
		)
		.expect("post_dispatch should succeed");
	}
}
