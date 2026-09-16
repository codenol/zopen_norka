//! How often a credential may be guessed, and what is remembered while it is.
//!
//! Issue #77: the sign-in route had no ceiling at all. Argon2id makes a wrong
//! guess expensive for whoever holds the hash — which, in an online attack, is
//! this server — so an unlimited login route is a guessing lever and a
//! denial-of-service lever at the same time. This module is the ceiling, and the
//! decisions in it are the ones worth arguing about.
//!
//! ## Why the counter is on the name TYPED, not on the account
//!
//! A counter kept per existing account answers differently for a name that
//! exists and one that does not: five tries at `alice` would be refused where
//! five tries at `nobody` would not. That is an account-existence oracle, and
//! the whole of [`authenticate`](super::AccountsDb::authenticate) is built to
//! refuse being one. So the counter is keyed by the folded name the caller sent,
//! resolved or not — which is why `alice` and `ALICE` share a budget (the
//! column's own `COLLATE NOCASE` folding, done in [`account_key`]) and why a
//! name nobody has ever registered can still be locked.
//!
//! ## Why two dimensions, and why they are not the same size
//!
//! A per-name budget alone leaves the cheap attack open: a caller that tries
//! ten thousand DIFFERENT names spends ten thousand budgets, and this process
//! pays ten thousand Argon2id verifications. A per-source budget alone lets one
//! caller lock out everybody behind its address. Together they bound each other:
//! the number of accounts one address can lock within a window is at most that
//! address's budget.
//!
//! The address dimension is OPTIONAL, and only the transport can supply it — see
//! [`super::accounts_signin::SignInAttempt::source`]. A caller that does not know
//! the address gets the name-keyed ceiling only, which is the honest behaviour:
//! the alternative is keying on something the request can lie about.
//!
//! ## The cost this accepts, said out loud
//!
//! A per-name budget lets a stranger deny one account for one window: they can
//! spend somebody else's budget with guesses and the owner then waits. That is
//! the price of a lockout, and it is bounded by the address ceiling — one caller
//! can lock at most that many accounts per window, which is a handful, not the
//! whole deployment. The alternative was leaving the guessing unlimited, and
//! with Argon2id the guessing is also CPU this process pays for.
//!
//! The refusal is also FAST — microseconds, where a checked attempt costs the
//! KDF's tens of milliseconds — so a caller can measure that a name is locked.
//! That is not the fact the 401 refuses to give: locked is a state anyone can
//! put any name into, existing or not, and a caller who spent a budget already
//! knows it did.
//!
//! ## Why the rows are in SQLite
//!
//! A restart is not a security event. An in-memory counter is cleared by a
//! crash, a deploy, or an OOM — the events an attacker is most able to cause —
//! and it is invisible to the operator's own `sqlite3` shell, which is where
//! "why is this account locked" has to be answerable. The cost is one small
//! write per failed attempt, on a path that has just spent tens of milliseconds
//! in the KDF, and the shape that keeps the table from growing without bound is
//! the sweep in [`AccountsDb::record_sign_in_failure`].
//!
//! ## What a success does, and what it deliberately does not
//!
//! A password that verifies clears the failure streak for THAT NAME — a person
//! who mistyped once and then got it right is not punished for their earlier
//! typo. It clears nothing for the address: a caller holding one working account
//! would otherwise be able to wash away its own budget whenever it liked, and
//! the address ceiling is exactly the one an attacker with a valid account would
//! want to reset.
//!
//! ## What an operator does with it
//!
//! The rows ARE the lockout list, readable in the deployment's own data
//! directory:
//!
//! ```text
//! sqlite3 "$OPENPENCIL_ONLINE_DATA_DIR/accounts.db" \
//!   "SELECT scope, subject, failures, datetime(last_failure_at,'unixepoch') \
//!    FROM sign_in_attempts ORDER BY last_failure_at DESC"
//! ```
//!
//! `scope = 'account'` names the account somebody is guessing at, `scope =
//! 'source'` the caller doing it: a row in the first with no row in the second
//! is a person mistyping their own password, and both together is somebody
//! working through a list. An operator who has to let a person in before the
//! window expires removes the row (`DELETE FROM sign_in_attempts WHERE scope =
//! 'account' AND subject = 'alice'`) and has, by then, already seen what
//! happened — which is the reason the counters are durable and named rather
//! than an anonymous number in memory.

use std::net::{IpAddr, Ipv6Addr};

use rusqlite::{params, OptionalExtension};

use super::accounts_error::AccountsError;
use super::accounts_policy::SignInLimits;
use super::AccountsDb;

/// The longest a counter key may be, in characters.
///
/// A name is attacker-chosen text and this table is written before anybody has
/// authenticated, so the key is capped rather than stored as sent. Truncating
/// can only MERGE two long names into one budget, which is the direction that
/// over-counts — a caller cannot dodge a budget by padding its name.
const MAX_SUBJECT_CHARS: usize = 128;

impl AccountsDb {
    /// How long the caller must wait, when its budget is already spent.
    ///
    /// Read BEFORE any password is looked at — [`super::AccountsDb::authenticate`]
    /// runs this first and returns on `Some` — because a request that is going to
    /// be refused must not cost an Argon2id verification. That order is the
    /// denial-of-service half of the answer, not an optimization.
    ///
    /// `None` means the name, the address, or both have budget left. A streak
    /// that has aged out past the window is not a lockout: the row is still
    /// there until the next failure sweeps it, and this reads the same
    /// `last_failure_at` the recorder resets on.
    pub(super) fn sign_in_retry_after(
        &self,
        account_key: &str,
        source_key: Option<&str>,
        limits: &SignInLimits,
        now: i64,
    ) -> Result<Option<i64>, AccountsError> {
        let conn = self.conn();
        let mut statement = conn.prepare_cached(
            "SELECT failures, last_failure_at FROM sign_in_attempts
             WHERE scope = ?1 AND subject = ?2",
        )?;
        // The name first: it is the budget a targeted guesser spends, and the
        // one whose wait is worth reporting when both are spent.
        let spent = [("account", account_key, limits.max_failures_per_account)]
            .into_iter()
            .chain(source_key.map(|key| ("source", key, limits.max_failures_per_source)));
        for (scope, subject, budget) in spent {
            let recorded = statement
                .query_row(params![scope, subject], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })
                .optional()?;
            let Some((failures, last_failure_at)) = recorded else {
                continue;
            };
            if failures < i64::from(budget) {
                continue;
            }
            if let Some(retry_after_secs) =
                seconds_until_free(last_failure_at, limits.window_secs, now)
            {
                return Ok(Some(retry_after_secs));
            }
        }
        Ok(None)
    }

    /// Record that a password did not open an account.
    ///
    /// Called for [`SignInOutcome::Rejected`](super::SignInOutcome::Rejected)
    /// only. A refused account that DID verify its password is not a guess —
    /// see the module docs on what a success clears — and a store that cannot
    /// read the row it was checking does not count either: a fault on this side
    /// must not spend the account holder's budget.
    ///
    /// The sweep is here, on the write path, rather than on a timer: it is one
    /// indexed range delete, it needs no machinery the daemon would have to
    /// start, and it runs exactly as often as the table grows. A name a stranger
    /// invented is a row that disappears one window later, so the table is
    /// bounded by the failures that are still being counted plus the ones that
    /// just aged out — not by the number of names ever tried.
    pub(super) fn record_sign_in_failure(
        &self,
        account_key: &str,
        source_key: Option<&str>,
        limits: &SignInLimits,
        now: i64,
    ) -> Result<(), AccountsError> {
        let conn = self.conn();
        let expired_before = now - limits.window_secs;
        conn.execute(
            "DELETE FROM sign_in_attempts WHERE last_failure_at <= ?1",
            params![expired_before],
        )?;
        for (scope, subject) in [("account", account_key)]
            .into_iter()
            .chain(source_key.map(|key| ("source", key)))
        {
            // One statement for 'start a streak' and 'continue one', because the
            // two are one fact about the caller. The CASEs are what make the
            // window sliding: a row that has aged out restarts at one failure
            // rather than carrying a count of failures nobody is counting any
            // more. (The sweep above has usually deleted such a row already; a
            // row that survived it is one whose timestamp is exactly on the
            // boundary, and the CASE is what makes the two paths agree.)
            conn.execute(
                "INSERT INTO sign_in_attempts
                     (scope, subject, failures, first_failure_at, last_failure_at)
                 VALUES (?1, ?2, 1, ?3, ?3)
                 ON CONFLICT (scope, subject) DO UPDATE SET
                     failures = CASE WHEN sign_in_attempts.last_failure_at <= ?4
                                     THEN 1 ELSE sign_in_attempts.failures + 1 END,
                     first_failure_at = CASE WHEN sign_in_attempts.last_failure_at <= ?4
                                             THEN ?3
                                             ELSE sign_in_attempts.first_failure_at END,
                     last_failure_at = ?3",
                params![scope, subject, now, expired_before],
            )?;
        }
        Ok(())
    }

    /// Forget this name's failures, because its password was just proved.
    ///
    /// Only the `account` row goes. See the module docs for why the address row
    /// is untouched: a successful sign-in must not be a way for whoever holds
    /// one working account to refund a budget spent attacking everybody else.
    pub(super) fn clear_sign_in_failures(&self, account_key: &str) -> Result<(), AccountsError> {
        self.conn().execute(
            "DELETE FROM sign_in_attempts WHERE scope = 'account' AND subject = ?1",
            params![account_key],
        )?;
        Ok(())
    }
}

/// The counter key for a name somebody typed.
///
/// Folded to ASCII lower case, which is what the `users.username` column's
/// `COLLATE NOCASE` does — not Rust's full Unicode lowercasing, which would fold
/// pairs of characters SQLite treats as different names. The direction matters:
/// under-folding would let `Alice` and `ALICE` spend two budgets, which is the
/// bypass this exists to close.
///
/// Trimmed for the same reason the lookup trims: `"  Alice  "` opens Alice's
/// account, so it has to spend Alice's budget.
pub(super) fn account_key(username: &str) -> String {
    let mut key: String = username.trim().chars().take(MAX_SUBJECT_CHARS).collect();
    key.make_ascii_lowercase();
    key
}

/// The counter key for the address a request arrived from.
///
/// IPv6 is counted by its `/64` prefix. An IPv6 host is routinely handed a whole
/// /64 — 2^64 addresses it may source from — so keying on the full address would
/// let one caller spend a fresh budget per request by picking a new interface
/// identifier, with no cooperation from its network. The prefix is the block a
/// provider actually assigns to one subscriber, so that is the unit the budget
/// is spent in.
///
/// The text is what an operator reads in the table, which is why both halves are
/// rendered the way the address is written rather than as a hash: `203.0.113.7`
/// and `2001:db8:1:2::/64` name the caller to whoever is looking at the lockout.
pub(super) fn source_key(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let network = u128::from_be_bytes(v6.octets()) & !(u128::MAX >> 64);
            format!("{}/64", Ipv6Addr::from(network))
        }
    }
}

/// Seconds until a streak recorded at `last_failure_at` stops refusing, or
/// `None` when it already has.
///
/// The window is measured from the LAST failure and not from the first, so a
/// burst keeps its own lock alive for as long as it continues: an attacker who
/// stops guessing is free again one window after giving up, and one who keeps
/// guessing at the allowed rate never gets back in.
fn seconds_until_free(last_failure_at: i64, window_secs: i64, now: i64) -> Option<i64> {
    let free_at = last_failure_at.saturating_add(window_secs);
    // At least one second: the guard answers with a `Retry-After`, and "try
    // again in 0 seconds" is an invitation to an immediate retry.
    (free_at > now).then(|| (free_at - now).max(1))
}
