//! Signing in: the one place that decides whether a name and a password open an
//! account.
//!
//! ## Why this is in the store and not in the route
//!
//! Because it is the operation with a security consequence, and the route is
//! the wrong place for a rule that must hold for every route. Three separate
//! decisions live in it, and each of them is one a route would get subtly
//! wrong on its own:
//!
//!   * the password is checked BEFORE the account's status is looked at, so the
//!     only person who learns that an account is disabled is one who already
//!     holds its password;
//!   * a name that matches nothing costs the same as a password that is wrong,
//!     so the answer time does not say whether an account exists;
//!   * the answer to "the password is wrong" and "there is no such account" is
//!     ONE answer, so a login form built on it cannot be talked into telling
//!     the difference.
//!
//! ## What it does not decide
//!
//! Whether a signed-in account may do anything afterwards. That is
//! [`super::super::web_canvas_server::request_access`]'s question, answered from
//! the roles and the document, and keeping it there is what stops this module
//! growing an authorization policy nobody would look for here.

use std::sync::OnceLock;

use rusqlite::params;

use super::accounts_error::AccountsError;
use super::accounts_model::{User, UserStatus};
use super::accounts_password::{hash_password, verify_password};
use super::accounts_secret::issue_token;
use super::AccountsDb;

/// What came of a name and a password.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

impl AccountsDb {
    /// Check a name and a password, and say what they open.
    ///
    /// `now` is written to `users.last_seen_at` on success and nowhere else: a
    /// failed attempt is not the account being seen, and a lockout counter built
    /// on this column later must not count a stranger's guesses as activity.
    /// `updated_at` is left alone — signing in does not change the account, and
    /// a column that moved here could no longer answer when the RECORD last
    /// changed.
    ///
    /// Only [`UserStatus::Active`] accounts sign in. An invited account has no
    /// password to check, and disabled or orphan ones are refused after their
    /// password has been verified — which is the part that matters: an
    /// implementation that checked the status first would answer "disabled"
    /// to anybody who guessed a name.
    ///
    /// A login that accepts an ADDRESS as well as a name is composed by the
    /// caller — [`Self::find_user_by_email`] and then this — rather than built
    /// in here, because whether a product lets somebody type either one is a
    /// decision about its form, not about its accounts.
    pub fn authenticate(
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
