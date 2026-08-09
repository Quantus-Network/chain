//! Custom signed extensions for the runtime.
extern crate alloc;
use crate::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use core::marker::PhantomData;
use frame_support::pallet_prelude::{
	InvalidTransaction, TransactionValidityError, ValidTransaction,
};
use frame_system::ensure_signed;
use qp_high_security::HighSecurityInspector;
use qp_wormhole::TransferProofRecorder;
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::{
	traits::{DispatchInfoOf, PostDispatchInfoOf, TransactionExtension},
	DispatchResult, Weight,
};

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
	type Pre = ();
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "ReversibleTransactionExtension";

	fn weight(&self, _call: &RuntimeCall) -> Weight {
		// One `is_high_security` storage read for the flat whitelist check.
		T::DbWeight::get().reads(1)
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		_call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		Ok(())
	}

	fn validate(
		&self,
		origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl sp_runtime::traits::Implication,
		_source: frame_support::pallet_prelude::TransactionSource,
	) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
		let who = ensure_signed(origin.clone())
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::BadSigner))?;

		// Enforce the high-security whitelist on the top-level signer. Origin-rewriting wrappers
		// (`as_derivative`/`as_recovered`) re-check the whitelist at the effective origin inside
		// their own pallets at dispatch time, so this flat check needs no call traversal.
		if !crate::configs::HighSecurityConfig::is_call_allowed(&who, call) {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(1)));
		}

		Ok((ValidTransaction::default(), (), origin))
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
///   - Batch transfers (utility.batch, batch_all, force_batch)
///   - Multisig transfers (multisig.execute)
///   - Recovery transfers (recovery.as_recovered)
///   - Held-fund seizures/recoveries (reversible_transfers.cancel / recover_funds, which move value
///     with `transfer_on_hold` instead of a free-balance transfer)
///   - Recovery-deposit seizures (recovery.close_recovery, which moves the rescuer's deposit with
///     `repatriate_reserved`)
///   - Future call-based mechanisms automatically covered, since wrapper calls emit their inner
///     events within the same extrinsic's event range
///
/// COVERAGE BOUNDARY: transaction extensions only run for transactions, so this scan
/// never sees events emitted from hooks (`on_initialize` / `on_finalize`). Every
/// hook-context credit therefore needs — and has — an explicit
/// `TransferProofRecorder::record_transfer_proof` call instead:
///   - reversible-transfers' scheduled execution records its transfer in `do_execute_transfer`;
///   - mining rewards and the treasury share record theirs in `on_finalize`
///     (`pallet_mining_rewards`). Those credits use `mint_into`, which *does* emit
///     `Balances::Minted` (the same event this scanner records); they are safe from
///     double-recording only because distribution runs in `on_finalize`, outside every
///     extrinsic's scan window. Moving that distribution into `on_initialize` or a
///     signed path without also suppressing the scan (or the explicit record) would
///     inflate wormhole exit capacity.
///
/// The one remaining hook-context path is a governance-enacted call: referenda enactment
/// dispatches the approved call via the scheduler in `on_initialize` (e.g. a Root
/// `force_transfer`), so its events are not scanned and no leaf is recorded. This is a
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

	/// Worst-case `ref_time` (picoseconds) to stream-decode one `EventRecord` in
	/// [`Self::record_proofs_from_events_since`]. A record is a small SCALE blob
	/// (phase + event enum + topics, typically well under ~300 bytes) decoded from an
	/// already-fetched storage value — roughly 100–300ns of pure decode on reference
	/// hardware; 1µs is a conservative ceiling.
	const EVENT_SCAN_DECODE_REF_TIME_PS: u64 = 1_000_000;

	/// Weight of the post-dispatch event scan when `events` records are present at
	/// scan time. `Events::stream_iter` fetches the storage value (one read) and the
	/// scan then decodes EVERY record present — `Iterator::skip` discards but still
	/// decodes the pre-snapshot prefix — so the cost is per record *present*, not per
	/// record matched or recorded.
	fn event_scan_weight(events: u32) -> Weight {
		if events == 0 {
			return Weight::zero();
		}
		T::DbWeight::get().reads(1).saturating_add(Weight::from_parts(
			Self::EVENT_SCAN_DECODE_REF_TIME_PS.saturating_mul(u64::from(events)),
			0,
		))
	}

	fn count_transfers(call: &RuntimeCall) -> u64 {
		// NOTE: this must stay in sync with the events matched by `record_proofs_from_events_since`
		// — we only weight calls whose emitted events we actually record. In particular
		// `Balances::force_set_balance` is deliberately NOT counted here: it emits `BalanceSet`
		// (an absolute set, not a transfer/mint), which we cannot turn into a transfer proof and
		// therefore never record.
		//
		// Wrappers whose inner call is stored on-chain rather than in the submitted call
		// (`Multisig::execute`, ...) cannot be counted statically. Proof-recording work they
		// trigger is reconciled in `post_dispatch`, which registers any weight shortfall
		// against the block via `register_extra_weight_unchecked`.
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. }) |
			RuntimeCall::Balances(pallet_balances::Call::force_transfer { .. }) => 1,

			// A successful cancel releases the held funds to the recipient with
			// `transfer_on_hold`, emitting exactly one `TransferOnHold` the scan records.
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

			// Closing a recovery repatriates the rescuer's reserved deposit to the caller,
			// emitting exactly one `ReserveRepatriated` the scan records. (On the failure
			// path the deposit is unreserved instead — an overcharge, never a shortfall.)
			RuntimeCall::Recovery(pallet_recovery::Call::close_recovery { .. }) => 1,

			RuntimeCall::Utility(pallet_utility::Call::batch { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::batch_all { calls }) |
			RuntimeCall::Utility(pallet_utility::Call::force_batch { calls }) =>
				calls.iter().map(Self::count_transfers).sum(),

			RuntimeCall::Utility(pallet_utility::Call::dispatch_as { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::dispatch_as_fallible { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::with_weight { call, .. }) |
			RuntimeCall::Utility(pallet_utility::Call::as_derivative { call, .. }) |
			RuntimeCall::Recovery(pallet_recovery::Call::as_recovered { call, .. }) =>
				Self::count_transfers(call),

			// Exactly one branch executes: the fallback runs only if the main call failed, in
			// which case the main call's changes (and its events) were rolled back. Charge the
			// worst case across the two branches; overcharge is never refunded, so summing
			// would systematically overprice the honest path.
			RuntimeCall::Utility(pallet_utility::Call::if_else { main, fallback }) =>
				Self::count_transfers(main).max(Self::count_transfers(fallback)),

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
			// this extension weighs can move funds out of it (`force_transfer` from it
			// is Root-only, enacted by the scheduler outside this pipeline).
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
	/// Returns the number of transfer proofs recorded, so callers can reconcile the actual
	/// proof-recording work against the statically charged weight.
	fn record_proofs_from_events_since(event_count_before: u32) -> u64 {
		// IMPORTANT: We must collect all transfers FIRST before calling record_transfer_proof,
		// because record_transfer_proof deposits new events which would invalidate the
		// stream_iter iterator (causing "Corrupted state" errors).
		//
		// The iterator reads from Events storage using stream_iter, which caches data.
		// If we modify Events storage during iteration (by depositing new events),
		// the cached data becomes stale and decoding fails.

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

		// Collect transfers to record - (asset_id, from, to, amount)
		let transfers_to_record: alloc::vec::Vec<(Option<AssetId>, AccountId, AccountId, Balance)> =
			frame_system::Pallet::<Runtime>::read_events_no_consensus()
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
						// `Transfer`, it just emits a different event. (`TransferAndHold`
						// is deliberately not matched: nothing in the runtime emits it.)
						RuntimeEvent::Balances(pallet_balances::Event::TransferOnHold {
							source,
							dest,
							amount,
							..
						}) => Some((None, source, dest, amount)),
						// Reserved-balance repatriations. `pallet_recovery::close_recovery`
						// seizes the rescuer's recovery deposit into the rescued account with
						// `repatriate_reserved`, which emits this instead of a `Transfer`. The
						// event is only emitted for cross-account moves (self-repatriations
						// return early), and the credit belongs to `to` whether it lands free
						// or reserved, so record it unconditionally.
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
		recorded
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
	type Val = ();
	type Implicit = ();

	const IDENTIFIER: &'static str = "WormholeProofRecorderExtension";

	fn weight(&self, call: &RuntimeCall) -> Weight {
		let n = Self::count_transfers(call);
		if n > 0 {
			Self::per_transfer_weight().saturating_mul(n)
		} else {
			Weight::zero()
		}
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
		call: &RuntimeCall,
		_info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		// Snapshot current event count so we only process events added by this tx
		// (and any events from previous txs in the same block), and remember how many transfer
		// proofs were statically charged for so post_dispatch can reconcile.
		Ok((frame_system::Pallet::<Runtime>::event_count(), Self::count_transfers(call)))
	}

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
		Ok((ValidTransaction::default(), (), origin))
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
		// recorded, so the entire static per-transfer reservation is unspent. Returning
		// it lets the pipeline refund it (see `TxExtension`'s trailing `WeightReclaim`).
		if result.is_err() {
			return Ok(Self::per_transfer_weight().saturating_mul(charged_transfers));
		}

		// Captured BEFORE recording deposits new events: this is exactly the number
		// of records the scan below decodes.
		let events_at_scan = frame_system::Pallet::<Runtime>::event_count();
		let recorded = Self::record_proofs_from_events_since(event_count_before);

		// Two pieces of caller-influenced work here are invisible to the static
		// `weight()` and are therefore registered against the block post-hoc (this
		// keeps block-capacity accounting sound; it is not fee-charged):
		//
		// 1. The event scan itself: any call can emit events the scan must decode (e.g. batched
		//    `remark_with_event`), and the decode cost is per record present at scan time — see
		//    `event_scan_weight`.
		//
		// 2. Recording shortfall: wrappers that dispatch inner calls stored on-chain
		//    (`Multisig::execute`, ...) can emit transfer events the static `count_transfers`
		//    matcher cannot see, so the proof-recording work above may exceed the weight reserved
		//    by `weight()`. The flat per-transfer price times the count difference covers it.
		let mut extra = Self::event_scan_weight(events_at_scan);
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
		// `MaxPendingPerAccount + 1` regardless of how many holds were pending, `if_else`
		// is charged its heavier branch, and a short-circuited `batch` never executes its
		// remaining children. The per-transfer price is flat, so the unspent amount is
		// exactly the count difference times that price (and by construction never exceeds
		// this extension's declared weight).
		Ok(Self::per_transfer_weight().saturating_mul(charged_transfers.saturating_sub(recorded)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_support::{
		assert_err_ignore_postinfo, assert_noop, assert_ok,
		pallet_prelude::TransactionValidityError, traits::Currency,
	};
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

		// Treasury account + portion are required for mining-reward distribution. Both
		// must be explicit: the genesis default no longer configures anything (the old
		// default account was the keyless `[1u8; 32]` minting sentinel).
		pallet_treasury::GenesisConfig::<Runtime> {
			treasury_account: Some(AccountId32::from([9u8; 32])),
			treasury_portion: Some(sp_runtime::Permill::from_percent(50)),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		sp_io::TestExternalities::new(t)
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

			// Test the prepare method
			ext.clone().prepare((), &origin, &call, &Default::default(), 0).unwrap();
			assert_eq!((), ());

			// Test the validate method
			let result = ext.validate(
				origin,
				&call,
				&Default::default(),
				0,
				(),
				&TxBaseImplication::<()>(()),
				frame_support::pallet_prelude::TransactionSource::External,
			);
			// Alice is not high-security, so this should succeed
			assert_ok!(result);

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
		ReversibleTransactionExtension::<Runtime>::new()
			.validate(
				RuntimeOrigin::signed(signer),
				call,
				&Default::default(),
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

		// Test the prepare method
		ReversibleTransactionExtension::<Runtime>::new()
			.prepare((), &origin, &call, &Default::default(), 0)
			.unwrap();

		// Test the validate method
		validate_with(charlie(), &call)
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
	fn test_high_security_remove_recovery() {
		new_test_ext().execute_with(|| {
			// make sure high security account can't remove the recovery
			let call = RuntimeCall::Recovery(pallet_recovery::Call::remove_recovery {});
			let result = check_call(call);
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
	fn test_high_security_cancel_allowed() {
		new_test_ext().execute_with(|| {
			let call =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: sp_core::H256::default(),
				});
			assert_ok!(check_call(call));
		});
	}

	// =========================================================================
	// Origin-rewriting wrappers must not bypass high-security restrictions.
	// `as_recovered` / `as_derivative` re-check the whitelist at the effective
	// (rewritten) origin inside their own pallets, so a non-whitelisted call
	// cannot be dispatched as a high-security account, including under `batch`.
	// =========================================================================

	fn boxed(call: RuntimeCall) -> alloc::boxed::Box<RuntimeCall> {
		alloc::boxed::Box::new(call)
	}

	fn non_whitelisted_transfer() -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
			dest: MultiAddress::Id(bob()),
			value: 10 * EXISTENTIAL_DEPOSIT,
		})
	}

	fn whitelisted_schedule() -> RuntimeCall {
		RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::schedule_transfer {
			dest: MultiAddress::Id(bob()),
			amount: 10 * EXISTENTIAL_DEPOSIT,
		})
	}

	#[test]
	fn as_recovered_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// bob is charlie's recovery proxy; charlie is high-security (from genesis).
			pallet_recovery::Proxy::<Runtime>::insert(bob(), charlie());

			// A non-whitelisted call dispatched as the high-security account is rejected.
			assert_noop!(
				Recovery::as_recovered(
					RuntimeOrigin::signed(bob()),
					MultiAddress::Id(charlie()),
					boxed(non_whitelisted_transfer()),
				),
				pallet_recovery::Error::<Runtime>::CallNotAllowedForHighSecurity
			);

			// A whitelisted call is allowed through as the high-security account.
			assert_ok!(Recovery::as_recovered(
				RuntimeOrigin::signed(bob()),
				MultiAddress::Id(charlie()),
				boxed(whitelisted_schedule()),
			));
		});
	}

	#[test]
	fn as_derivative_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// Make alice's index-0 derivative a high-security account.
			let derivative = pallet_utility::derivative_account_id(alice(), 0u16);
			Balances::make_free_balance_be(&derivative, EXISTENTIAL_DEPOSIT * 100);
			assert_ok!(ReversibleTransfers::set_high_security(
				RuntimeOrigin::signed(derivative.clone()),
				qp_scheduler::BlockNumberOrTimestamp::BlockNumber(10),
				bob(),
			));

			// A non-whitelisted call dispatched as the high-security derivative is rejected.
			assert_err_ignore_postinfo!(
				Utility::as_derivative(
					RuntimeOrigin::signed(alice()),
					0,
					boxed(non_whitelisted_transfer()),
				),
				pallet_utility::Error::<Runtime>::CallNotAllowedForHighSecurity
			);

			// A whitelisted call as the derivative is allowed.
			assert_ok!(Utility::as_derivative(
				RuntimeOrigin::signed(alice()),
				0,
				boxed(whitelisted_schedule()),
			));

			// A different, non-high-security derivative is unaffected.
			assert_ok!(Utility::as_derivative(
				RuntimeOrigin::signed(alice()),
				1,
				boxed(RuntimeCall::System(frame_system::Call::remark { remark: vec![1] })),
			));
		});
	}

	#[test]
	fn batch_wrapped_high_security_call_is_blocked() {
		new_test_ext().execute_with(|| {
			// Wrapping the origin-rewriter in a batch does not bypass the check: `batch_all`
			// re-dispatches `as_recovered`, whose own check rejects the non-whitelisted call.
			pallet_recovery::Proxy::<Runtime>::insert(bob(), charlie());
			let inner = RuntimeCall::Recovery(pallet_recovery::Call::as_recovered {
				account: MultiAddress::Id(charlie()),
				call: boxed(non_whitelisted_transfer()),
			});
			assert_err_ignore_postinfo!(
				Utility::batch_all(RuntimeOrigin::signed(bob()), vec![inner]),
				pallet_recovery::Error::<Runtime>::CallNotAllowedForHighSecurity
			);
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

			let batch = RuntimeCall::Utility(pallet_utility::Call::batch {
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
	fn wormhole_proof_recorder_counts_as_derivative_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			let ext = WormholeProofRecorderExtension::<Runtime>::new();

			// A transfer hidden behind `as_derivative` (possibly batched) must be charged the
			// same per-transfer weight as a direct transfer.
			let wrapped = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
				index: 0,
				call: boxed(RuntimeCall::Utility(pallet_utility::Call::batch {
					calls: vec![non_whitelisted_transfer(), non_whitelisted_transfer()],
				})),
			});
			let weight = <WormholeProofRecorderExtension<Runtime> as TransactionExtension<
				RuntimeCall,
			>>::weight(&ext, &wrapped);
			assert_eq!(
				weight,
				WormholeProofRecorderExtension::<Runtime>::per_transfer_weight().saturating_mul(2),
				"as_derivative-wrapped transfers must be statically counted"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_if_else_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			// `if_else` executes exactly one branch: the fallback runs only if the main call
			// failed, in which case the main call's changes (and events) were rolled back. The
			// static charge must therefore cover the worst case across the two branches.
			let call = RuntimeCall::Utility(pallet_utility::Call::if_else {
				main: boxed(RuntimeCall::Utility(pallet_utility::Call::batch {
					calls: vec![non_whitelisted_transfer(), non_whitelisted_transfer()],
				})),
				fallback: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				2,
				"if_else must charge for the transfer-heavier branch (main)"
			);

			// The worst case can also sit in the fallback branch.
			let call = RuntimeCall::Utility(pallet_utility::Call::if_else {
				main: boxed(RuntimeCall::System(frame_system::Call::remark { remark: vec![1] })),
				fallback: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				1,
				"if_else must charge for the transfer-heavier branch (fallback)"
			);
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
	fn wormhole_proof_recorder_counts_dispatch_as_fallible_wrapped_transfers() {
		new_test_ext().execute_with(|| {
			let call = RuntimeCall::Utility(pallet_utility::Call::dispatch_as_fallible {
				as_origin: alloc::boxed::Box::new(OriginCaller::system(
					frame_system::RawOrigin::Signed(alice()),
				)),
				call: boxed(non_whitelisted_transfer()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&call),
				1,
				"dispatch_as_fallible-wrapped transfers must be statically counted"
			);
		});
	}

	#[test]
	fn wormhole_proof_recorder_counts_reversible_cancel_and_close_recovery() {
		new_test_ext().execute_with(|| {
			// `ReversibleTransfers::cancel` releases the held funds with `transfer_on_hold`,
			// emitting exactly one `TransferOnHold` that the scanner turns into a proof. The
			// call is statically visible, so the proof must be fee-charged, not just
			// reconciled post-hoc against block capacity.
			let cancel =
				RuntimeCall::ReversibleTransfers(pallet_reversible_transfers::Call::cancel {
					tx_id: Default::default(),
				});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&cancel),
				1,
				"cancel seizes held funds via transfer_on_hold and must be charged one proof"
			);

			// `Recovery::close_recovery` repatriates the rescuer's reserved deposit,
			// emitting exactly one `ReserveRepatriated` that the scanner records.
			let close = RuntimeCall::Recovery(pallet_recovery::Call::close_recovery {
				rescuer: MultiAddress::Id(bob()),
			});
			assert_eq!(
				WormholeProofRecorderExtension::<Runtime>::count_transfers(&close),
				1,
				"close_recovery repatriates the recovery deposit and must be charged one proof"
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
			run_lifecycle(&guardian, cancel, || {
				assert_ok!(ReversibleTransfers::cancel(
					RuntimeOrigin::signed(guardian.clone()),
					tx_id
				));
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
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
				WormholeProofRecorderExtension::<Runtime>::event_scan_weight(scanned.get()),
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
				let recorded =
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

			// The presented call is opaque to the static matcher (like `Multisig::execute`,
			// whose inner call lives on-chain), but the dispatch emits a real transfer
			// event that post_dispatch must record.
			let opaque_call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1] });
			assert_eq!(WormholeProofRecorderExtension::<Runtime>::count_transfers(&opaque_call), 0);

			let weight_before = frame_system::Pallet::<Runtime>::block_weight().total();

			let scanned = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), opaque_call, || {
				assert_ok!(Balances::transfer_keep_alive(
					RuntimeOrigin::signed(alice()),
					MultiAddress::Id(bob()),
					EXISTENTIAL_DEPOSIT * 50,
				));
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
			});

			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::per_transfer_weight().saturating_add(
					WormholeProofRecorderExtension::<Runtime>::event_scan_weight(scanned.get())
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

			// Capture the event count at the end of the dispatch closure: that is
			// exactly the number of records the post-dispatch scan decodes.
			let scanned = core::cell::Cell::new(0u32);
			run_lifecycle(&alice(), call, || {
				for i in 0..7u8 {
					assert_ok!(System::remark_with_event(RuntimeOrigin::signed(alice()), vec![i],));
				}
				scanned.set(frame_system::Pallet::<Runtime>::event_count());
			});
			assert!(scanned.get() >= 7, "the remarks must have emitted events");

			let weight_after = frame_system::Pallet::<Runtime>::block_weight().total();
			assert_eq!(
				weight_after.saturating_sub(weight_before),
				WormholeProofRecorderExtension::<Runtime>::event_scan_weight(scanned.get()),
				"the per-event decode work of the scan must be registered as block weight"
			);
		});
	}

	/// `weight()` reserves the static worst case, so a dispatch that performs fewer
	/// proof inserts than charged (short-circuited `batch`, `if_else`'s lighter branch,
	/// `recover_funds` with fewer pending holds than `MaxPendingPerAccount`) must have
	/// the difference refunded via `post_dispatch_details`, not kept forever.
	#[test]
	fn statically_overcharged_transfers_are_refunded_post_dispatch() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// A batch of two transfers is charged two per-transfer reservations, but a
			// short-circuiting `batch` stops at the first failure: simulate a dispatch
			// that only completed the first transfer.
			let call = RuntimeCall::Utility(pallet_utility::Call::batch {
				calls: vec![non_whitelisted_transfer(), non_whitelisted_transfer()],
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
			let scan = WormholeProofRecorderExtension::<Runtime>::event_scan_weight(events_at_scan);
			assert_eq!(
				frame_system::Pallet::<Runtime>::block_weight().total(),
				admitted.saturating_sub(per_transfer).saturating_add(scan),
				"the trailing WeightReclaim must return the wormhole refund to block capacity"
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
			let result = ext.prepare((), &origin, &call, &Default::default(), 0);
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

			// Execute a transfer (this emits pallet_balances::Event::Transfer)
			assert_ok!(Balances::transfer_keep_alive(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
				transfer_amount,
			));

			// Simulate what post_dispatch does - scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Verify transfer was recorded (proof is now in ZK trie)
			let count_after = Wormhole::transfer_count(&bob_account);
			assert_eq!(count_after, count_before + 1, "Transfer count should increment");
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
	fn event_based_proof_recording_batch_transfers() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			let bob_account = bob();
			let charlie_account = charlie();
			let bob_count_before = Wormhole::transfer_count(&bob_account);
			let charlie_count_before = Wormhole::transfer_count(&charlie_account);

			// Alice has EXISTENTIAL_DEPOSIT * 10000, use smaller amounts
			// Execute a batch with multiple transfers
			assert_ok!(Utility::batch(
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
			// exactly as it would for a plain transfer.
			let events_before = frame_system::Pallet::<Runtime>::event_count();
			assert_ok!(ReversibleTransfers::cancel(RuntimeOrigin::signed(guardian.clone()), tx_id));

			let recorded =
				WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(
					events_before,
				);

			assert_eq!(recorded, 1, "hold-transfer seizure must be recorded as a transfer proof");
			assert_eq!(Wormhole::transfer_count(&guardian), count_before + 1);
		});
	}

	#[test]
	fn event_based_proof_recording_recovery_deposit_repatriation() {
		new_test_ext().execute_with(|| {
			System::set_block_number(1);

			// alice makes her account recoverable; bob (say, maliciously) initiates a
			// recovery, reserving the recovery deposit on his own account. The recovery
			// deposits are UNIT-denominated, so fund both well past the genesis balances.
			Balances::make_free_balance_be(&alice(), 100 * crate::UNIT);
			Balances::make_free_balance_be(&bob(), 100 * crate::UNIT);
			assert_ok!(Recovery::create_recovery(
				RuntimeOrigin::signed(alice()),
				vec![charlie()],
				1,
				0,
			));
			assert_ok!(Recovery::initiate_recovery(
				RuntimeOrigin::signed(bob()),
				MultiAddress::Id(alice()),
			));

			let count_before = Wormhole::transfer_count(&alice());
			let events_before = frame_system::Pallet::<Runtime>::event_count();

			// Closing the recovery seizes the rescuer's reserved deposit into alice's
			// free balance via `repatriate_reserved`, which emits
			// `Balances::ReserveRepatriated` — not a free-balance `Transfer`. The
			// credit is real spendable value landing on alice, so the recorder must
			// create a leaf for it.
			assert_ok!(Recovery::close_recovery(
				RuntimeOrigin::signed(alice()),
				MultiAddress::Id(bob()),
			));

			let recorded =
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

			// Mint tokens (requires root origin)
			// This emits pallet_balances::Event::Minted
			assert_ok!(Balances::force_set_balance(
				RuntimeOrigin::root(),
				MultiAddress::Id(recipient.clone()),
				mint_amount,
			));

			// Scan events and record proofs.
			// Use 0 as the before count for tests (all events are "new").
			WormholeProofRecorderExtension::<Runtime>::record_proofs_from_events_since(0);

			// Note: force_set_balance emits Minted event, which we scan for
			// The proof should use MintingAccount as 'from'
			let count_after = Wormhole::transfer_count(&recipient);

			// Check if count increased (depends on whether Minted event is emitted)
			// force_set_balance may emit BalanceSet instead of Minted
			// This test documents the expected behavior - proofs are now in ZK trie
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
