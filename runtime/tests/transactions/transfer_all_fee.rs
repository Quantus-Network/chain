//! A top-level `transfer_all { keep_alive: false }` is the wallet's "send max".
//! `transaction-payment` skips the post-dispatch refund when the transaction
//! reaps the payer, so whatever `WormholeProofRecorderExtension` reserves for
//! this call is paid in full. It must therefore reserve a single leaf, not the
//! supply bound a wrapped `transfer_all` carries.

use crate::transactions::high_security_tip::{funded_ext, pair, signed_call};
use frame_support::traits::Currency;
use quantus_runtime::{
	Balances, Executive, RuntimeCall, RuntimeEvent, System, EXISTENTIAL_DEPOSIT, UNIT,
};
use sp_core::Pair;
use sp_runtime::{traits::IdentifyAccount, AccountId32, MultiAddress};

fn recipient() -> AccountId32 {
	crate::common::TestCommons::account_id(7)
}

fn transfer_all(keep_alive: bool) -> RuntimeCall {
	RuntimeCall::Balances(pallet_balances::Call::transfer_all {
		dest: MultiAddress::Id(recipient()),
		keep_alive,
	})
}

fn actual_fee(who: &AccountId32) -> u128 {
	System::events()
		.into_iter()
		.find_map(|record| match record.event {
			RuntimeEvent::TransactionPayment(
				pallet_transaction_payment::Event::TransactionFeePaid {
					who: payer,
					actual_fee,
					..
				},
			) if payer == *who => Some(actual_fee),
			_ => None,
		})
		.expect("fee event")
}

/// Fee actually paid: everything that left the sender and did not reach the
/// recipient.
fn run_send_max(keep_alive: bool) -> (u128, u128) {
	let pair = pair();
	let account = pair.public().into_account();
	funded_ext(&account, false).execute_with(|| {
		Balances::make_free_balance_be(&recipient(), EXISTENTIAL_DEPOSIT);
		let sender_before = Balances::total_balance(&account);
		let recipient_before = Balances::total_balance(&recipient());

		let xt = signed_call(&pair, account.clone(), transfer_all(keep_alive), 0, 0);
		Executive::apply_extrinsic(xt).expect("valid").expect("dispatch ok");

		let moved = Balances::total_balance(&recipient()) - recipient_before;
		let paid = sender_before - Balances::total_balance(&account) - moved;
		(paid, actual_fee(&account))
	})
}

#[test]
fn send_max_pays_the_single_leaf_fee_whether_or_not_it_reaps_the_sender() {
	let (paid_keep_alive, reported_keep_alive) = run_send_max(true);
	let (paid_reaped, reported_reaped) = run_send_max(false);

	assert_eq!(
		paid_keep_alive, reported_keep_alive,
		"with the sender alive the refund lands and the event is exact"
	);
	// Reaping the sender skips every post-dispatch refund, including the call's own
	// weight correction, so a small residual over the reported fee is inherent. The
	// wormhole reservation must not add to it: with the supply bound reserved here
	// this paid 4.4x the kept-alive fee.
	assert!(paid_reaped >= reported_reaped);
	assert!(
		paid_reaped <= paid_keep_alive + paid_keep_alive / 20,
		"send max must pay the single-leaf fee, not an unrefunded supply-bound reservation: \
		 paid {paid_reaped}, kept-alive {paid_keep_alive}"
	);
	assert!(paid_reaped < UNIT / 100, "a single-leaf transfer_all fee is well under 0.01 UNIT");
}
