//! Mainnet genesis allocation — the audit surface for the 27% TGE mint.
//!
//! How to audit this file:
//! 1. [`GENESIS_ALLOCATION`] is 27% of [`MAX_SUPPLY`] (5_670_000). Every coin of it is either a row
//!    of [`VESTING`] or a [`SEED`] endowment: `sum(VESTING) + SEEDED_ACCOUNTS * SEED ==
//!    GENESIS_ALLOCATION` is asserted at compile time.
//! 2. [`VESTING`] is the allocation spreadsheet, one row per schedule: `(account, amount, unlock
//!    start day, unlock end day)`. Days count from TGE, the first non-zero block timestamp (the
//!    genesis block's `Now` is 0 and is not TGE). Every row vests linearly with `cliff == start`,
//!    so nothing unlocks as a lump. Spreadsheet grants lock for [`GRANT_UNLOCK_DELAY_DAYS`] (1
//!    year) and then vest over [`GRANT_UNLOCK_PERIOD_DAYS`] (3 years) to [`GRANT_END_DAYS`]; the
//!    intents grant ([`INTENTS_AMOUNT`]) vests from TGE over [`INTENTS_UNLOCK_PERIOD_DAYS`].
//! 3. [`TREASURY`] rows resolve to the [`TREASURY_THRESHOLD`]-of-10 multisig of [`TREASURERS`]:
//!    [`INITIAL_LIQUIDITY_AMOUNT`] (1% of max supply) vesting over [`INITIAL_LIQUIDITY_VEST_DAYS`],
//!    and [`TREASURY_VESTING_AMOUNT`] — whatever of the 27% is left — on the grant clock.
//! 4. Spreadsheet grants sum to [`TOTAL_GRANT_AMOUNT`].
//! 5. Each of [`TREASURERS`] and [`TECH_COLLECTIVE`] (distinct sets) is endowed with [`SEED`] as
//!    free balance so they can pay fees and deposits from block 1; a treasurer cannot claim a
//!    schedule before the multisig can act. The vesting pot additionally receives its existential
//!    deposit from `genesis_template`, the only issuance outside the 27%.
//! 6. Accounts are SS58 addresses only — no personal names.
//!
//! Flip [`FINALIZED`] only after every `REPLACE_WITH_` placeholder is filled. Until then the
//! `mainnet` preset refuses to build.

use super::{account_from_ss58, days_ms, Multisig, VestingScheduleTuple};
use crate::{AccountId, MAX_SUPPLY, UNIT};
use alloc::vec::Vec;

/// 27% of [`MAX_SUPPLY`] minted at genesis.
pub const GENESIS_ALLOCATION: u128 = 27 * MAX_SUPPLY / 100;
/// 1% of [`MAX_SUPPLY`] to the treasury, vested linearly over [`INITIAL_LIQUIDITY_VEST_DAYS`] with
/// no lockup.
pub const INITIAL_LIQUIDITY_AMOUNT: u128 = MAX_SUPPLY / 100;
/// Linear vest for the treasury liquidity, in days from TGE.
pub const INITIAL_LIQUIDITY_VEST_DAYS: u64 = 16;
/// Delay before unlock starts, in days from TGE. Shared by every spreadsheet grant except the
/// intents grant, and by the company vesting.
pub const GRANT_UNLOCK_DELAY_DAYS: u64 = 365;
/// Linear unlock after the delay, in days.
pub const GRANT_UNLOCK_PERIOD_DAYS: u64 = 3 * 365;
/// Shared finish of every delayed schedule, in days from TGE.
pub const GRANT_END_DAYS: u64 = GRANT_UNLOCK_DELAY_DAYS + GRANT_UNLOCK_PERIOD_DAYS;
/// Intents grant (ADDRESS_45): unlock starts at TGE, linear over [`INTENTS_UNLOCK_PERIOD_DAYS`].
pub const INTENTS_AMOUNT: u128 = 42_000 * UNIT;
pub const INTENTS_UNLOCK_PERIOD_DAYS: u64 = 365;
/// Sum of the spreadsheet grants: every [`VESTING`] row that is not a [`TREASURY`] row.
pub const TOTAL_GRANT_AMOUNT: u128 = 4_957_502 * UNIT;
/// Free balance endowed to each treasurer and tech collective member at genesis.
pub const SEED: u128 = 3 * UNIT;
/// Number of [`SEED`] endowments: [`TREASURERS`] plus [`TECH_COLLECTIVE`].
pub const SEEDED_ACCOUNTS: u128 = (TREASURERS.len() + TECH_COLLECTIVE.len()) as u128;
/// Approvals required on the treasury multisig.
pub const TREASURY_THRESHOLD: u32 = 6;
const TREASURY_NONCE: u64 = 0;
/// Leftover of the 27% after grants, liquidity and seeds — vests to the treasury on the grant
/// clock.
pub const TREASURY_VESTING_AMOUNT: u128 =
	GENESIS_ALLOCATION - TOTAL_GRANT_AMOUNT - INITIAL_LIQUIDITY_AMOUNT - SEEDED_ACCOUNTS * SEED;

/// Flip to `true` only when every placeholder below is a launch address.
pub const FINALIZED: bool = true;

/// Treasury multisig signers.
pub const TREASURERS: [&str; 10] = [
	"qzkmmtHL1XZ94LnDc43hTuUUW6o2jkjQBSgwSYF7Dm2JErapB",
	"qzmFVMW5f48c1cBNXTzNjAn9YbLhCfohXqEgofeCn3UhJUaMw",
	"qzkBP7kRB9wh1dKozJK83Y4e6GqSFgWEhf69WEggay7wt5Ls7",
	"qzmtKfCXKHvhg5agvp1XKgx8ymuShQzNhJ8ZrpKCqy4Y5z6HW",
	"qzk42WAmPUAbirA9SdAfpu9qr5UdTwU2PnLoifRFtX7GJLf5P",
	"qznYp4bk4YdA77tXqfokbwLcrzbDMLuKdNKTQs4s9EjLbx2rq",
	"qzjeW1DVYmPUqYCiosKnr3Ssh8DVTjmXmYV6JHibiVQKiBveD",
	"qzm2TcoDAyqycAmMuS91bXhPwWqEgaKcYGh5nQ3wqBqCzc9d5",
	"qzjuYZSefPGu3DFgBfXDRgGXJ3YvdFwXBNTSbE8Fa7dk3aV8u",
	"qzmHuteJKcKmyNdLrgS9WivKrsSv6AvwDC56ETf21YhastHXm",
];

/// Tech collective members, distinct from [`TREASURERS`].
pub const TECH_COLLECTIVE: [&str; 10] = [
	"qzpfF7tvw4nhTzhhAjifFGPRKqKTBfCk68dgvM5DwnDCSyXYJ",
	"qzjpLEi51md3q9FpECBTxRuLQsP31N5NmT9CF3ovVXY2RVKWE",
	"qzomCRTgMZHtdDWBBAqm5WLG8FwLKuYCJHgrAMznkKEhWG8qJ",
	"qzmQoa5gvngjmTafLJqCBnmmmNKFCuaVvZiZcdairrGWVCBjA",
	"qzosrX14BSUfTsGYB3NmakBWBvJgLGwqboJCQAvL2V1Tm64UA",
	"qznivf7i8HDSqqb4uCSzeAypCPpKAHZASC2QPQa4D1zzYGsZu",
	"qzmyhjrbKk9Nhtcq3gnNpX5p9CR8HnM92SUGBC2cVW6FpAzd2",
	"qzneDSjVt2Lbf2nq3UZFGG3cutFfkk5ktffaeJRamAb134obQ",
	"qzmdJBPzYdtBGLaJYjtT7FutNd47q79rjr63qhJTeQNzvBb9c",
	"qzoijuSKGJAgAbPChRc1LxxxjeZRpWGLBooHDUYzhPJ6ooTvw",
];

/// Row account that resolves to the treasury multisig derived from [`TREASURERS`].
pub const TREASURY: &str = "TREASURY";

/// One schedule: `(account, amount, unlock start day, unlock end day)`; `cliff == start`.
type Row = (&'static str, u128, u64, u64);

#[rustfmt::skip]
const VESTING: &[Row] = &[
	//  account                                                               amount  start                    end
	("qzo9ddk2km1qqAmXNiwo55vs5LNTU851mSQc8ufBwkQygHRBv",            840_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzq9inB2ja6jGuaBbWktKeBycHNA7eRDHVFnaxAVqDuEzhsdc",            840_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzjiozZCkaJnvm6h54aaNTCvcDPAbG2q2XLWTiB6pxW35UyEe",            525_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoHvg4L13wUfCzXHEFFggxsXh2x2hbygBq2K79tcirCsfDN9",            262_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmqKknQfjrvvXvBciPL2XppaDiJi7wg26GzRP266rVsmwwcN",             70_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkDeyY438Pqodmq6pJHF1m8E9xwb2VSJ5wB7pUM2Q8mNg65P",            472_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoBzAq4F4Cpf82Jj3yDTNrocqWDqnxB47VHuQH5KFEUB2yPj",             52_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmpFp2GCj23rSuskbEM7u7sezbKAhEvvY52DaZ37vLjX8ukp",             26_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzox7xGgNk2MpkUsFMUpdHnbJbfiUu3nDskaFPLfs4ZZRDid2",             52_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkbv3tDr1NBK2iey2z9NGNfyALnYcbvXJmuy8HTN67qsnRPP",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qznoufvc6dhwie2TvuXWrWMCRbA925WTofereiTnZxGzjNTsP",              5_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkbVzeUi5zYmMaewnnkpRMhBHV7y7pMo4SvTNRZCACMPaKWU",              2_100 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzogZBABSmhpUBWEaKw4fs78igH9PyHCJ1NxSpEjn2sgjjrFi",              5_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkNXaZ6Up8nWQ1wn5m4LACnw6PwP2orNd79omAdbvhxYXJ6X",              5_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzjmD32vqMhwy4GU2ma8VANS4V4TtHcQo35Ynsa3xioMjRFye",             42_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzjkDjpWP8vvfNmebeH5AVonbVDFNKDggh9XT1KbTQPPrj6Fm",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzouqJ4eyfxH3qH3MeyoaNhossKvqAQxs9YP6DK4Mo8J6FJ3H",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkiLP9tPxy4ECdy8MQKkpETEQzYzrLZ2DZMQazCEHfDuYtqX",              7_350 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoRSP6Yj1YbTGa9mb5QaUb3yHcAdNhPdP2N7DCWD8TeAGoYU",              2_100 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzodzJGufu5U1XC1Xi39Q7cTbzJs3JK92Vc3xpLBPXXNFhy11",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qznv4Xb41HMaS4AqzTVMvFHcPLtN4PtS2m468bkUu1mTWemTH",              5_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmqPJuc5RMbk2RnZwm1TBg4bkqx87pBxS7UuQNgxb2vHnFU5",              5_250 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzpj8mzijcLbcvT16bUPKhYkRsSHWzuMqb1uqqvJpjizKvesu",            420_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoh6MQtRojFVAjRbWKJa3FscHEfzMJ3ybZecsCSx3miVFVJA",            210_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qznRPT2raGy3spYuU3o8ViA6MLUu8NGSKyNJA6TfxFjF3miHd",            136_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzpZ9YQen3oRuFZzFJf3SWyCn9b2eXHmbJe6nCxFQSUSabNpg",             10_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzjhNC1Gcp1zHpJ43fEcqGEyTDKd2jipi9dhm3UjFNJFvUNEb",            105_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzk9eYREUbogTy9oKBrZAWP7ZcqtPJi2RPZoLfqHW2pS51qFS",             52_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoHyF1vuKVv7icssc8BsHBgV2PqGyyyDY8i3usJpGe6GpweY",            210_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qznhY8p1pfFCSuNYvb45EfXaRgaWEbK54p1kcqrFzCiEoe77X",             10_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmQ5JFmchP1K76LUQ5sAuFWWJzWSmMkuXfJSJF6td7oBKZyf",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzk1GBELnXPLK45ZbWeYikfnjcWgPHZVSFJDR1fzsHLYXTQte",            105_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoSXaPFb6hibxHkKeZeWGErsoYyeFE9dNzM6gTaAKd4urunc",            210_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzpZfwZBgtBKtGjWwTdE9tMBgfPT3wc7hRWnPVjV8z8KgGcHv",             63_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzo48Sn5vZu7BhT1ojyT6budi9pbF5rA1tiGUtKbrRFNfJJxf",              1_050 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzpNMj2jLhAauhf6FBNJEYUoYx9dpAPh51vPjuF3rKJ5SREDU",             10_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzn3iwmQ3dPaiJuQ9EqUiZXaty8AyuZrfFfPjUfU4dS1tCRy5",             21_000 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzpUkSoWSwXLmcZ7qnda6CNL9RDLM7gJAaw4XGVdvryqsFX1k",                830 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzodtvut7NErNcRMX8r2Z6X9x5ybXeq1yHBVTcbfRZUEoUGy6",              1_312 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzkHddrW8eR1NpNqx7XWBzAk2zMSQ8BCAWcJFgarBqw6QPSvq",              4_410 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmHoVdpPzADYwkg7Kof1mQ7DeDC7swyACsKMBaPC21HLzCRs",              2_100 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzn4nPBrz9w2KoJTKZSbBu3Aaghw9aWk9hxBNUkyRm3VqtiwU",              2_100 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qznksEvwu1cy1Gqk75E8FecqKQpmEKJ5SgDaUz44mMSgJCSGZ",              2_100 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzmhuMENeJ44MNStX9jxu57JGsECUCP8S81438DpCqnAcmBk1",              1_050 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzoQxyk2HkyDMScbhSDYM5Fv9yNKDvkPMFvburmbMCagYiTqN",             10_500 * UNIT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
	("qzjX3MScnFw6dxFjR32KAgrtF8S9YvpbioyNAGPZ3rCefFtvE",            INTENTS_AMOUNT,  0,                       INTENTS_UNLOCK_PERIOD_DAYS),
	(TREASURY,                                             INITIAL_LIQUIDITY_AMOUNT,  0,                       INITIAL_LIQUIDITY_VEST_DAYS),
	(TREASURY,                                              TREASURY_VESTING_AMOUNT,  GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS),
];

const fn vesting_total() -> u128 {
	let mut sum = 0u128;
	let mut i = 0;
	while i < VESTING.len() {
		sum += VESTING[i].1;
		i += 1;
	}
	sum
}

const _: () = assert!(GENESIS_ALLOCATION == 5_670_000 * UNIT);
const _: () = assert!(INITIAL_LIQUIDITY_AMOUNT == 210_000 * UNIT);
const _: () = assert!(TOTAL_GRANT_AMOUNT == 4_957_502 * UNIT);
const _: () = assert!(SEEDED_ACCOUNTS * SEED == 60 * UNIT);
const _: () = assert!(TREASURY_VESTING_AMOUNT == 502_438 * UNIT);
const _: () = assert!(GRANT_UNLOCK_PERIOD_DAYS == 3 * 365);
const _: () = assert!(GRANT_END_DAYS == 4 * 365);
const _: () = assert!(GRANT_UNLOCK_DELAY_DAYS < GRANT_END_DAYS);
const _: () = assert!(INTENTS_UNLOCK_PERIOD_DAYS > 0 && INITIAL_LIQUIDITY_VEST_DAYS > 0);
const _: () = assert!(VESTING.len() == 48);
const _: () = assert!(VESTING.len() <= pallet_vesting::MAX_GENESIS_SCHEDULES as usize);
const _: () = assert!(
	vesting_total() == TOTAL_GRANT_AMOUNT + INITIAL_LIQUIDITY_AMOUNT + TREASURY_VESTING_AMOUNT
);
const _: () = assert!(vesting_total() + SEEDED_ACCOUNTS * SEED == GENESIS_ALLOCATION);

fn require_finalized() {
	if !FINALIZED {
		panic!(
			"mainnet allocation is not finalized — fill every REPLACE_WITH_ placeholder in \
			 genesis_config_presets/mainnet_vesting.rs and flip FINALIZED before building this \
			 chain spec"
		);
	}
}

fn accounts(ss58s: &[&str], what: &str) -> Vec<AccountId> {
	let out: Vec<AccountId> = ss58s.iter().map(|s| account_from_ss58(s)).collect();
	let mut deduped = out.clone();
	deduped.sort();
	deduped.dedup();
	assert_eq!(deduped.len(), out.len(), "mainnet {what} must be distinct");
	out
}

/// Treasury multisig signers.
pub fn treasurers() -> Vec<AccountId> {
	require_finalized();
	accounts(&TREASURERS, "treasurers")
}

/// Tech collective members; must not overlap the treasurers.
pub fn tech_collective() -> Vec<AccountId> {
	let treasurers = treasurers();
	let members = accounts(&TECH_COLLECTIVE, "tech collective members");
	assert!(
		members.iter().all(|m| !treasurers.contains(m)),
		"mainnet tech collective members must differ from the treasurers"
	);
	members
}

/// The [`TREASURY_THRESHOLD`]-of-10 multisig of [`TREASURERS`].
pub fn treasury_account() -> AccountId {
	Multisig::<crate::Runtime>::derive_multisig_address(
		&treasurers(),
		TREASURY_THRESHOLD,
		TREASURY_NONCE,
	)
}

/// [`SEED`] free balance for every treasurer and tech collective member.
pub fn seed_balances() -> Vec<(AccountId, u128)> {
	treasurers()
		.into_iter()
		.chain(tech_collective())
		.map(|who| (who, SEED))
		.collect()
}

/// [`VESTING`] in row order (ids from 0). Times are offsets from the first non-zero timestamp.
pub fn schedules() -> Vec<VestingScheduleTuple> {
	let treasury = treasury_account();
	VESTING
		.iter()
		.map(|(who, amount, start, end)| {
			let who = if *who == TREASURY { treasury.clone() } else { account_from_ss58(who) };
			(who, days_ms(*start), days_ms(*start), days_ms(*end), *amount)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::configs::{VestingMinimumPayout, VestingPayoutQuantum};

	fn grant_rows() -> Vec<&'static Row> {
		VESTING.iter().filter(|(who, ..)| *who != TREASURY).collect()
	}

	#[test]
	fn every_coin_of_the_27_percent_is_accounted_for() {
		assert_eq!(GENESIS_ALLOCATION, 27 * MAX_SUPPLY / 100);
		assert_eq!(GENESIS_ALLOCATION, 5_670_000 * UNIT);
		assert_eq!(SEED, 3 * UNIT);
		assert_eq!(TREASURERS.len(), 10);
		assert_eq!(TECH_COLLECTIVE.len(), 10);
		assert_eq!(TREASURY_VESTING_AMOUNT, 502_438 * UNIT);
		let vested: u128 = VESTING.iter().map(|(_, amount, _, _)| *amount).sum();
		assert_eq!(vested + SEEDED_ACCOUNTS * SEED, GENESIS_ALLOCATION);
	}

	#[test]
	fn grants_match_the_allocation_sheet() {
		let grants = grant_rows();
		assert_eq!(grants.len(), 46);
		assert_eq!(
			grants.iter().map(|(_, amount, _, _)| *amount).sum::<u128>(),
			TOTAL_GRANT_AMOUNT
		);
		let (intents, locked): (Vec<&&Row>, Vec<&&Row>) =
			grants.iter().partition(|(_, _, start, _)| *start == 0);
		assert_eq!(locked.len(), 45);
		assert!(locked
			.iter()
			.all(|(_, _, start, end)| (*start, *end) == (GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS)));
		assert_eq!(intents.len(), 1);
		assert_eq!(
			(intents[0].1, intents[0].2, intents[0].3),
			(INTENTS_AMOUNT, 0, INTENTS_UNLOCK_PERIOD_DAYS)
		);
		assert_eq!(GRANT_UNLOCK_DELAY_DAYS, 365);
		assert_eq!(GRANT_UNLOCK_PERIOD_DAYS, 3 * 365);
		assert_eq!(INTENTS_UNLOCK_PERIOD_DAYS, 365);
	}

	#[test]
	fn treasury_gets_liquidity_and_the_company_remainder() {
		let rows: Vec<_> = VESTING.iter().filter(|(who, ..)| *who == TREASURY).collect();
		assert_eq!(rows.len(), 2);
		assert_eq!(rows[0], &(TREASURY, INITIAL_LIQUIDITY_AMOUNT, 0, INITIAL_LIQUIDITY_VEST_DAYS));
		assert_eq!(
			rows[1],
			&(TREASURY, TREASURY_VESTING_AMOUNT, GRANT_UNLOCK_DELAY_DAYS, GRANT_END_DAYS)
		);
		assert_eq!(INITIAL_LIQUIDITY_AMOUNT, MAX_SUPPLY / 100);
		assert_eq!(INITIAL_LIQUIDITY_VEST_DAYS, 16);
	}

	#[test]
	fn rows_are_valid_distinct_schedules() {
		for (who, amount, start, end) in VESTING {
			assert!(start < end, "{who}");
			assert!(*amount >= VestingMinimumPayout::get(), "{who}");
			assert_eq!(amount % VestingPayoutQuantum::get(), 0, "{who}");
		}
		let mut accounts: Vec<&str> = grant_rows().iter().map(|(who, ..)| *who).collect();
		accounts.extend(TREASURERS);
		accounts.extend(TECH_COLLECTIVE);
		let total = accounts.len();
		accounts.sort_unstable();
		accounts.dedup();
		assert_eq!(
			accounts.len(),
			total,
			"grant, treasurer and tech collective addresses must be distinct"
		);
	}

	#[test]
	fn seeds_cover_bootstrap_costs() {
		use super::super::{tech_referendum_cost, treasury_signer_seed};
		assert!(treasury_signer_seed(TREASURERS.len() as u32) <= SEED);
		assert!(tech_referendum_cost() <= SEED);
	}

	#[test]
	fn refuses_to_build_until_finalized() {
		if FINALIZED {
			let placeholders = VESTING
				.iter()
				.map(|(who, ..)| *who)
				.chain(TREASURERS)
				.chain(TECH_COLLECTIVE)
				.filter(|s| s.starts_with("REPLACE_WITH_"))
				.count();
			assert_eq!(placeholders, 0);
			let treasury = treasury_account();
			let rows = schedules();
			assert_eq!(rows.len(), VESTING.len());
			assert_eq!(rows.iter().filter(|(who, ..)| *who == treasury).count(), 2);
			assert!(rows.iter().all(|(_, start, cliff, _, _)| start == cliff));
			let seeds = seed_balances();
			assert_eq!(seeds.len(), SEEDED_ACCOUNTS as usize);
			assert!(seeds.iter().all(|(who, amount)| {
				*amount == SEED && *who != treasury && rows.iter().all(|(b, ..)| b != who)
			}));
		} else {
			assert!(std::panic::catch_unwind(treasurers).is_err());
			assert!(std::panic::catch_unwind(tech_collective).is_err());
			assert!(std::panic::catch_unwind(treasury_account).is_err());
			assert!(std::panic::catch_unwind(seed_balances).is_err());
			assert!(std::panic::catch_unwind(schedules).is_err());
		}
	}
}
