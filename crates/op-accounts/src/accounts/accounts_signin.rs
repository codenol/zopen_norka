//! Signing in: the one place that decides whether a name and a password open an
//! account.
//!
//! ## Why this is in the store and not in the route
//!
//! Because it is the operation with a security consequence, and the route is
//! the wrong place for a rule that must hold for every route. Four separate
//! decisions live in it, and each of them is one a route would get subtly wrong
//! on its own:
//!
//!   * how often the same name may be tried at all, and how often one address
//!     may try — checked BEFORE any hash is verified, so a request that is going
//!     to be refused costs two indexed reads instead of an Argon2id
//!     verification ([`super::accounts_signin_throttle`]);
//!   * the password is checked BEFORE the account's status is looked at, so the
//!     only person who learns that an account is disabled is one who already
//!     holds its password;
//!   * a name that matches nothing costs the same as a password that is wrong,
//!     so the answer time does not say whether an account exists;
//!   * the answer to "the password is wrong" and "there is no such account" is
//!     ONE answer, so a login form built on it cannot be talked into telling
//!     the difference. The refusal above is one answer too, and for the same
//!     reason: it is keyed on the name the caller typed, so a name that exists
//!     and one that does not are refused identically.
//!
//! ## What it does not decide
//!
//! Whether a signed-in account may do anything afterwards. That is
//! `op_host_services::web_canvas_server::request_access`'s question, answered
//! from the roles and the document, and keeping it there is what stops this
//! module growing an authorization policy nobody would look for here.

use std::net::IpAddr;
use std::sync::OnceLock;

use rusqlite::params;

use super::accounts_error::AccountsError;
use super::accounts_model::{User, UserStatus};
use super::accounts_password::{hash_password, verify_password};
use super::accounts_policy::SignInLimits;
use super::accounts_secret::issue_token;
use super::accounts_signin_throttle::{account_key, source_key};
use super::AccountsDb;

/// One attempt to sign in, as the caller knows it.
///
/// A struct rather than four arguments because the parts are added to over time
/// by callers that do not all know all of them — the source address exists only
/// where there is a socket — and because a bare `None` in third position at
/// every call site says nothing about what is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignInAttempt<'a> {
    /// The name as it was typed. Matched case-insensitively; the budget it
    /// spends is folded the same way.
    pub username: &'a str,
    /// The password as it was typed. Never stored, never logged, never part of
    /// an error.
    pub password: &'a str,
    /// The address the request arrived from, when the transport knows one.
    ///
    /// The source ceiling is the only bound on how many Argon2id verifications
    /// one caller can make this process compute, and it cannot exist without
    /// this value: the accept loop supplies it, and a caller that does not know
    /// the address is limited by the name alone. Nothing from the request
    /// itself (`Host`, `Origin`, a forwarding header) may be used instead —
    /// every one of those is written by the caller, and a budget keyed on a
    /// value the attacker chooses is not a budget.
    pub source: Option<IpAddr>,
}

impl<'a> SignInAttempt<'a> {
    /// An attempt from an unknown address.
    pub const fn new(username: &'a str, password: &'a str) -> Self {
        Self {
            username,
            password,
            source: None,
        }
    }

    /// The same attempt, from the address it arrived on.
    pub const fn with_source(mut self, source: IpAddr) -> Self {
        self.source = Some(source);
        self
    }
}

/// What came of a name and a password.
///
/// `clippy::large_enum_variant` is wrong here for the same reason it is on
/// [`crate::accounts::FirstAdmin`]: `User` is ~224 bytes because an account is
/// nine owned strings, and this value is produced once per sign-in attempt and
/// matched immediately. Boxing `SignedIn` would allocate on the SUCCESS path of
/// every sign-in to shrink a value that lives on one stack frame, and would
/// change a `pub` variant on the surface `op-host-services` and `op-cli`
/// re-export.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum SignInOutcome {
    /// The credentials are right and the account may be used.
    SignedIn(User),
    /// The credentials do not open an account.
    ///
    /// One variant for every way that can happen — no such name, the wrong
    /// password, an account that has no password yet — because the caller must
    /// not be able to tell them apart, and the surest way to keep it that way
    /// is to have nothing to tell them apart WITH.
    Rejected,
    /// The credentials are right and the account still may not sign in.
    ///
    /// Separate from [`Self::Rejected`] because it reaches only a caller that
    /// already proved it holds this account's password: it is not an oracle,
    /// and "your account is disabled" is the one answer a blocked person can
    /// act on.
    Blocked(UserStatus),
    /// Nothing was checked: this name, or this address, has spent its budget of
    /// failures too recently.
    ///
    /// Separate from [`Self::Rejected`] because the two are different facts —
    /// "your credentials are wrong" would be a lie about a correct password —
    /// and one variant covers both dimensions on purpose. Which ceiling fired
    /// is not the caller's business, and splitting it here would hand back
    /// exactly the distinction the name-keyed counter exists to hide.
    Throttled {
        /// Seconds until the budget starts moving again. The route puts it in
        /// `Retry-After`, which is where a client can act on it.
        retry_after_secs: i64,
    },
}

impl AccountsDb {
    /// Check a name and a password, and say what they open — within the budget
    /// this deployment allows.
    ///
    /// The order is the security property, so it is stated once and kept here:
    ///
    /// 1. the budget of the NAME TYPED and of the ADDRESS is read, and an
    ///    exhausted one answers [`SignInOutcome::Throttled`] immediately. Before
    ///    the lookup and before the KDF: an online attack is limited by what the
    ///    server will compute, so a refusal that still costs an Argon2id
    ///    verification is not a limit;
    /// 2. only then is the account looked up and the password verified;
    /// 3. the outcome is recorded: a failure spends the budget, a password that
    ///    verifies gives that name's budget back.
    ///
    /// `now` is written to `users.last_seen_at` on success and nowhere else: a
    /// failed attempt is not the account being seen, and the failure counter
    /// this module now keeps is deliberately a different table — a stranger's
    /// guesses must never read as activity on the account.
    /// `updated_at` is left alone — signing in does not change the account, and
    /// a column that moved here could no longer answer when the RECORD last
    /// changed.
    ///
    /// A login that accepts an ADDRESS as well as a name is composed by the
    /// caller — [`Self::find_user_by_email`] and then this — rather than built
    /// in here, because whether a product lets somebody type either one is a
    /// decision about its form, not about its accounts. A caller that does that
    /// spends the budget of the typed text, which is the conservative direction:
    /// an address that is also somebody's name shares one budget rather than
    /// getting two.
    pub fn authenticate(
        &self,
        attempt: &SignInAttempt<'_>,
        limits: &SignInLimits,
        now: i64,
    ) -> Result<SignInOutcome, AccountsError> {
        let account_key = account_key(attempt.username);
        let source_key = attempt.source.map(source_key);
        if let Some(retry_after_secs) =
            self.sign_in_retry_after(&account_key, source_key.as_deref(), limits, now)?
        {
            return Ok(SignInOutcome::Throttled { retry_after_secs });
        }
        let outcome = self.check_credentials(attempt.username, attempt.password, now)?;
        match &outcome {
            SignInOutcome::Rejected => {
                self.record_sign_in_failure(&account_key, source_key.as_deref(), limits, now)?
            }
            // A password that verified — whether or not the account may then use
            // it — is proof the caller holds the credential, so this name's
            // failures are forgotten. The address's are not: see the throttle
            // module for why a working account must not refund an attacker's
            // budget.
            SignInOutcome::SignedIn(_) | SignInOutcome::Blocked(_) => {
                self.clear_sign_in_failures(&account_key)?
            }
            // Returned by the guard above, which has already returned; listed so
            // that a variant added later has to be decided here rather than
            // falling through a wildcard.
            SignInOutcome::Throttled { .. } => {}
        }
        Ok(outcome)
    }

    /// The credential check itself: what the store asked before it counted
    /// anything.
    ///
    /// Only [`UserStatus::Active`] accounts sign in. An invited account has no
    /// password to check, and disabled or orphan ones are refused after their
    /// password has been verified — which is the part that matters: an
    /// implementation that checked the status first would answer "disabled"
    /// to anybody who guessed a name.
    fn check_credentials(
        &self,
        username: &str,
        password: &str,
        now: i64,
    ) -> Result<SignInOutcome, AccountsError> {
        let Some(user) = self.find_user_by_username(username)? else {
            spend_the_same_time(password);
            return Ok(SignInOutcome::Rejected);
        };
        let Some(stored) = user.password_hash.as_deref() else {
            // An account created from an invite: it has no password, so there
            // is nothing to verify. The work is spent anyway, or "this account
            // exists but has no password yet" would answer faster than a wrong
            // password and be just as readable.
            spend_the_same_time(password);
            return Ok(SignInOutcome::Rejected);
        };
        if !verify_password(password, stored)? {
            return Ok(SignInOutcome::Rejected);
        }
        if !user.status.may_sign_in() {
            return Ok(SignInOutcome::Blocked(user.status));
        }

        self.conn().execute(
            "UPDATE users SET last_seen_at = ?2 WHERE id = ?1",
            params![user.id, now],
        )?;
        let mut user = user;
        // The returned value is the row as it now stands, so a caller that logs
        // the sign-in does not log the previous time the account was seen.
        user.last_seen_at = Some(now);
        Ok(SignInOutcome::SignedIn(user))
    }
}

/// Do the work a password check costs, without a password to check it against.
///
/// Argon2id is deliberately slow, so an account that does not exist would
/// otherwise answer in microseconds where a wrong password takes tens of
/// milliseconds — a difference any client can measure, and one that turns a
/// login form into a way to ask "is this person a customer of yours".
///
/// The hash is a real Argon2id hash of a value nobody can present, computed
/// once per process: the verification runs the whole KDF against the candidate,
/// which is exactly what the comparison a real account would have done.
fn spend_the_same_time(password: &str) {
    if let Some(dummy) = dummy_hash() {
        // The result is deliberately discarded: this hash belongs to no
        // account, so nothing can match it.
        let _ = verify_password(password, dummy);
    }
}

/// The throwaway hash [`spend_the_same_time`] verifies against.
///
/// Built from a random token rather than a literal so there is no password in
/// the source that a reader could think is meaningful, and cached because
/// hashing it costs what a login costs — paying that once is the point, paying
/// it per attempt would be a denial-of-service of our own making.
fn dummy_hash() -> Option<&'static str> {
    static DUMMY: OnceLock<Option<String>> = OnceLock::new();
    DUMMY
        .get_or_init(|| hash_password(&issue_token().ok()?.0).ok())
        .as_deref()
}
