# Vesting Module

Quantus fork of FRAME `pallet-vesting`: schedules unlock against wall-clock time
(`Config::Moment` / `Config::TimeProvider`, milliseconds since the unix epoch from
`pallet_timestamp`) instead of block numbers. Schedule fields are `locked`, `per_ms`, `start`,
and an optional `repurchaser`.

Each schedule may carry an optional `repurchaser` account (set at genesis or on `vested_transfer`).
The repurchaser — in addition to Root — may:

- `force_remove_vesting_schedule` (repurchase): the funds still unvested at that time are
  transferred to the repurchaser; the already-vested portion stays with the holder. Root removal
  instead leaves all funds with the holder (unlocked).
- `transfer_vesting_schedule`: move the schedule as-is to another account (e.g. lost-wallet
  recovery or switching to a multisig), along with the still-unvested funds. Schedule terms
  (`locked`, `per_ms`, `start`, `repurchaser`) are preserved verbatim.

Schedules without a repurchaser can only be removed or transferred by Root. Two schedules can
only be merged if their repurchasers match, and the merged schedule keeps that repurchaser.

## Overview

A simple module providing a means of placing a linear curve on an account's locked balance. This
module ensures that there is a lock in place preventing the balance to drop below the *unvested*
amount for reason other than the ones specified in `UnvestedFundsAllowedWithdrawReasons`
configuration value.

As the amount vested increases over time, the amount unvested reduces. However, locks remain in
place and explicit action is needed on behalf of the user to ensure that the amount locked is
equivalent to the amount remaining to be vested. This is done through a dispatchable function,
either `vest` (in typical case where the sender is calling on their own behalf) or `vest_other`
in case the sender is calling on another account's behalf.

## Interface

This module implements the `VestingSchedule` trait.

### Dispatchable Functions

- `vest` - Update the lock, reducing it in line with the amount "vested" so far.
- `vest_other` - Update the lock of another account, reducing it in line with the amount
  "vested" so far.

[`Call`]: ./enum.Call.html
[`Config`]: ./trait.Config.html

License: Apache-2.0
