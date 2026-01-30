//! High-Security Account Primitives
//!
//! This crate provides the core trait for High-Security account inspection and validation
//! in the Quantus blockchain. High-Security accounts are designed for institutional users
//! and high-value accounts that require additional security controls:
//!
//! - **Call whitelisting**: Only approved operations can be executed
//! - **Guardian/interceptor role**: Designated account can cancel malicious transactions
//! - **Delayed execution**: Time window for intervention before irreversible actions
//!
//! ## Architecture
//!
//! The `HighSecurityInspector` trait is implemented at the runtime level and consumed by:
//! - `pallet-multisig`: Validates calls in High-Security multisigs
//! - `pallet-reversible-transfers`: Provides the storage and core HS account management
//! - Transaction extensions: Validates calls for High-Security EOAs
//!
//! This primitives crate breaks the circular dependency between pallets by providing
//! a shared abstraction that all consumers can depend on.

#![cfg_attr(not(feature = "std"), no_std)]

/// High-Security account inspector trait
///
/// Provides methods to check if an account is designated as High-Security,
/// validate whitelisted calls, and retrieve guardian information.
///
/// # Type Parameters
///
/// - `AccountId`: The account identifier type
/// - `RuntimeCall`: The runtime-level call enum (required for whitelist validation)
///
/// # Implementation Notes
///
/// This trait is typically implemented at the runtime level in a configuration struct
/// that bridges multiple pallets. The runtime implementation delegates to the actual
/// storage pallet (e.g., `pallet-reversible-transfers`) for account status checks
/// and defines the runtime-specific whitelist logic.
///
/// # Example
///
/// ```ignore
/// // In runtime/src/configs/mod.rs
/// pub struct HighSecurityConfig;
///
/// impl qp_high_security::HighSecurityInspector<AccountId, RuntimeCall>
///     for HighSecurityConfig
/// {
///     fn is_high_security(who: &AccountId) -> bool {
///         pallet_reversible_transfers::Pallet::<Runtime>::is_high_security_account(who)
///     }
///
///     fn is_whitelisted(call: &RuntimeCall) -> bool {
///         matches!(
///             call,
///             RuntimeCall::ReversibleTransfers(
///                 pallet_reversible_transfers::Call::schedule_transfer { .. }
///             )
///         )
///     }
///
///     fn guardian(who: &AccountId) -> Option<AccountId> {
///         pallet_reversible_transfers::Pallet::<Runtime>::get_guardian(who)
///     }
/// }
/// ```
pub trait HighSecurityInspector<AccountId, RuntimeCall> {
	/// Check if an account is designated as High-Security
	///
	/// High-Security accounts are restricted to executing only whitelisted calls
	/// and have a guardian that can intercept malicious transactions.
	///
	/// # Parameters
	///
	/// - `who`: The account to check
	///
	/// # Returns
	///
	/// `true` if the account is High-Security, `false` otherwise
	fn is_high_security(who: &AccountId) -> bool;

	/// Check if a runtime call is whitelisted for High-Security accounts
	///
	/// The whitelist is typically defined at the runtime level and includes only
	/// operations that are reversible or delayed (e.g., scheduled transfers).
	///
	/// # Parameters
	///
	/// - `call`: The runtime call to validate
	///
	/// # Returns
	///
	/// `true` if the call is whitelisted, `false` otherwise
	///
	/// # Implementation Notes
	///
	/// The runtime-level implementation typically uses pattern matching on `RuntimeCall`:
	///
	/// ```ignore
	/// matches!(
	///     call,
	///     RuntimeCall::ReversibleTransfers(
	///         pallet_reversible_transfers::Call::schedule_transfer { .. }
	///     ) | RuntimeCall::ReversibleTransfers(
	///         pallet_reversible_transfers::Call::cancel { .. }
	///     )
	/// )
	/// ```
	fn is_whitelisted(call: &RuntimeCall) -> bool;

	/// Get the guardian/interceptor account for a High-Security account
	///
	/// The guardian has special privileges to cancel pending transactions
	/// initiated by the High-Security account.
	///
	/// # Parameters
	///
	/// - `who`: The High-Security account
	///
	/// # Returns
	///
	/// `Some(guardian_account)` if the account has a guardian, `None` otherwise
	fn guardian(who: &AccountId) -> Option<AccountId>;

	// NOTE: No benchmarking-specific methods in the trait!
	// Production API should not be polluted by test/benchmark requirements.
	// Use pallet-specific helpers instead (e.g.,
	// pallet_reversible_transfers::Pallet::add_high_security_benchmark_account)
}

/// Default implementation for `HighSecurityInspector`
///
/// This implementation disables all High-Security checks, allowing any call to execute.
/// It's useful for:
/// - Test environments that don't need HS enforcement
/// - Pallets that want optional HS support via `type HighSecurity = ();`
/// - Gradual feature rollout
///
/// # Behavior
///
/// - `is_high_security()`: Always returns `false`
/// - `is_whitelisted()`: Always returns `true` (allow everything)
/// - `guardian()`: Always returns `None`
impl<AccountId, RuntimeCall> HighSecurityInspector<AccountId, RuntimeCall> for () {
	fn is_high_security(_who: &AccountId) -> bool {
		false
	}

	fn is_whitelisted(_call: &RuntimeCall) -> bool {
		true // Allow everything if no High-Security enforcement
	}

	fn guardian(_who: &AccountId) -> Option<AccountId> {
		None
	}
}
