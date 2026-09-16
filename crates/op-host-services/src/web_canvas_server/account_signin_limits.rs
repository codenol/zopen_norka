//! What a deployment can tune about sign-in, and where it says so.
//!
//! Issue #77 gave the sign-in route a budget; this is the file an operator
//! reads to change it, and the only one that names the variables. It is a
//! sibling of the routes rather than part of them because it is CONFIGURATION:
//! a reader looking for "how do I widen this" should not have to read the route
//! table, and the route table should not grow a knob every time a deployment
//! wants one.
//!
//! The numbers themselves are the store's ([`SignInLimits`]), and the reason
//! each is the size it is lives with them in `accounts::accounts_policy`. This
//! module only turns three environment variables into that struct — and applies
//! to them the rule every other daemon ceiling applies to its own: **a value
//! that is absent, unparsable or nonsensical keeps the default.**
//!
//! That rule is the whole safety argument. An operator who mistypes a variable
//! name, or writes `...=0` meaning "no limit", must not be able to REMOVE a
//! security bound by accident: there is no zero, no "off" and no negative here,
//! and the widest a deployment can ask for is a large number that still stops a
//! machine. Anything below two is treated the same way, because a budget of one
//! failure refuses every account after a single typo and a window of one second
//! has expired before the caller can read the answer — neither is a setting
//! somebody meant.
//!
//! The three are printed in the daemon's startup banner alongside the
//! connection ceilings, so an operator sees the numbers this deployment is
//! actually running with rather than the ones they think they set.

use crate::accounts::SignInLimits;

/// How many failed sign-ins one NAME may spend before it is refused.
///
/// The name of the variable, not the number: the default is the store's
/// ([`SignInLimits::default`]), and this is what an operator writes to change
/// it.
pub const SIGN_IN_MAX_FAILURES_ENV: &str = "OPENPENCIL_ONLINE_SIGNIN_MAX_FAILURES";

/// The same, for one source ADDRESS.
///
/// This is the one a deployment behind a reverse proxy has to raise: every
/// caller then arrives from the proxy's own address, so a single budget covers
/// everybody behind it. The daemon does not trust `X-Forwarded-For` — nothing
/// here can tell a proxy's header from a client's — so the number is the only
/// lever there is, and a proxy that mitigates abuse itself may reasonably want
/// it much larger than the default.
pub const SIGN_IN_MAX_FAILURES_PER_SOURCE_ENV: &str =
    "OPENPENCIL_ONLINE_SIGNIN_MAX_FAILURES_PER_SOURCE";

/// How far back failures are counted — and so how long a refusal lasts.
pub const SIGN_IN_LOCKOUT_SECS_ENV: &str = "OPENPENCIL_ONLINE_SIGNIN_LOCKOUT_SECS";

/// The sign-in budgets this deployment configured, or the store's defaults.
///
/// Anything absent, unparsable, or below two keeps the default, exactly as the
/// connection ceilings do ([`super::tenant::TenantLimits::from_env`]). So no
/// setting here turns the throttle off: a deployment can make the budget
/// generous, and cannot make it absent.
///
/// A window of one second is refused with the rest of them — it would expire
/// between two requests and read as "off" to whoever set it, which is the
/// mistake this reader exists to catch.
pub(super) fn sign_in_limits_from_env() -> SignInLimits {
    let defaults = SignInLimits::default();
    SignInLimits {
        max_failures_per_account: env_at_least_two(
            SIGN_IN_MAX_FAILURES_ENV,
            defaults.max_failures_per_account,
        ),
        max_failures_per_source: env_at_least_two(
            SIGN_IN_MAX_FAILURES_PER_SOURCE_ENV,
            defaults.max_failures_per_source,
        ),
        window_secs: env_at_least_two(SIGN_IN_LOCKOUT_SECS_ENV, defaults.window_secs),
    }
}

/// One environment value, or the default when it is absent, unparsable, or
/// below two.
///
/// Two and not one because both numbers count things that only mean something
/// from the second one on — see the module docs. The bound is applied to the
/// PARSED value, so `-1` and `0` are refused alongside a typo rather than
/// reaching the store, where a budget of zero would refuse every name the
/// moment it failed once.
fn env_at_least_two<T>(name: &str, fallback: T) -> T
where
    T: std::str::FromStr + PartialOrd + From<u8>,
{
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<T>().ok())
        .filter(|value| *value >= T::from(2u8))
        .unwrap_or(fallback)
}
