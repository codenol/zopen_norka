//! How long a credential lasts.
//!
//! These are the numbers a deployment gets when it does not choose, and they
//! live here — in the store, next to the tables they bound — because the
//! alternative is what this module was added to fix: every call site picking a
//! number, and a reset link that lasts a year because one route copy-pasted a
//! session's lifetime.
//!
//! Every operation that takes a lifetime still takes it as an argument. A
//! deployment with an opinion passes its own; these are the defaults a caller
//! names rather than invents, and naming one puts the choice in a diff somebody
//! can review.

use super::accounts_model::OneTimePurpose;

/// How long a browser stays signed in.
///
/// Thirty days is the length a session can be and still be forgotten by the
/// person using it — which is the point of a session: a tool that asks for a
/// password every morning is one whose password ends up in a note. The window
/// is bounded because a session cookie is a bearer credential sitting in a
/// browser profile, and [`crate::accounts::AccountsDb::revoke_user_sessions`]
/// exists for the day one of them is not the person's own.
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// How long a password-reset link works.
///
/// An hour, and short on purpose: a reset link is a password with extra steps,
/// and it sits in an inbox, a chat log or a browser history where it outlives
/// its usefulness. Long enough for somebody to notice the mail and act on it;
/// short enough that a link found later is dead.
pub const PASSWORD_RESET_TTL_SECS: i64 = 60 * 60;

/// How long an address-verification link works.
///
/// A day, longer than a reset because the cost of the two failing is different:
/// an expired reset link delays somebody who is already trying to get in, and
/// an expired verification link leaves an account unable to prove its address
/// until a new one is sent. Nothing is at risk from the longer window — the
/// link proves an address, it does not hand over an account.
pub const EMAIL_VERIFY_TTL_SECS: i64 = 24 * 60 * 60;

/// How long an invitation link works.
///
/// A week: an invite is handed to a person who has to be told about it, decide,
/// and find a moment — and the operator who has to re-issue one that expired
/// over a weekend will re-issue it with a longer life, which is how a link ends
/// up lasting forever. A week is long enough for a holiday and short enough to
/// be forgotten on purpose.
pub const INVITE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// How many failed sign-ins one NAME may spend before it is refused.
///
/// Five, because the number has to survive two different readers. A person who
/// cannot remember which of their passwords this account uses types it three or
/// four times, and a budget that locked them out on the second would make a
/// working account look broken. A guesser gets five tries per window against a
/// name they have to already know, which over a day is a few hundred attempts
/// against a password space that makes that hopeless — and the budget is spent
/// per name, so guessing a thousand names costs a thousand budgets, which is
/// what the per-source ceiling below is for.
pub const SIGN_IN_MAX_FAILURES_PER_ACCOUNT: u32 = 5;

/// How many failed sign-ins one ADDRESS may spend before it is refused.
///
/// Four times the per-name budget, and the difference is the point: an address
/// is not a person. An office, a university, a mobile carrier and every
/// deployment behind a reverse proxy arrive as ONE address, so this ceiling is
/// sized to swallow a crowd's ordinary typos and still stop a machine. It is
/// also the only bound on how many Argon2id verifications a single caller can
/// make this process compute, which is the denial-of-service half of issue #77:
/// without it, a caller spraying names that do not exist pays no price at all.
pub const SIGN_IN_MAX_FAILURES_PER_SOURCE: u32 = 20;

/// How far back failures are counted — and therefore how long a refusal lasts.
///
/// Fifteen minutes, one number for both jobs, because a second number would be
/// a second thing to explain: the budget is spent until this long after the
/// LAST failure, so a burst keeps its own lock alive and a person who stops
/// trying is free again in a quarter of an hour. Long enough to make online
/// guessing pointless, short enough that a locked-out person waits rather than
/// telephones the operator.
pub const SIGN_IN_FAILURE_WINDOW_SECS: i64 = 15 * 60;

/// The budget a credential gets before the store stops checking it.
///
/// The two dimensions are separate numbers rather than one because they protect
/// different things — see [`SIGN_IN_MAX_FAILURES_PER_ACCOUNT`] and
/// [`SIGN_IN_MAX_FAILURES_PER_SOURCE`] — and because an operator behind a
/// reverse proxy has to be able to widen exactly one of them.
///
/// A deployment with an opinion passes its own; [`Default`] is what it gets
/// when it does not. The web tier reads the environment once and hands the
/// result in ([`crate::accounts::SignInLimits`] is read by
/// `web_canvas_server::account_routes`), which is the same split every other
/// lifetime in this module follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignInLimits {
    /// Failures one name may spend within the window.
    pub max_failures_per_account: u32,
    /// Failures one source address may spend within the window, across every
    /// name it tries. Zero in the field would mean "no address is bounded",
    /// and no configuration can produce it: see the environment reader, which
    /// keeps the default for anything that is not a positive number.
    pub max_failures_per_source: u32,
    /// How far back failures are counted, and how long a refusal lasts.
    pub window_secs: i64,
}

impl Default for SignInLimits {
    fn default() -> Self {
        Self {
            max_failures_per_account: SIGN_IN_MAX_FAILURES_PER_ACCOUNT,
            max_failures_per_source: SIGN_IN_MAX_FAILURES_PER_SOURCE,
            window_secs: SIGN_IN_FAILURE_WINDOW_SECS,
        }
    }
}

impl OneTimePurpose {
    /// The lifetime this purpose gets when the caller names no other.
    ///
    /// On the purpose rather than chosen per call site, because the two differ
    /// for a reason (see [`EMAIL_VERIFY_TTL_SECS`]) and a route that had to
    /// pick would eventually pick the same number for both.
    pub const fn default_ttl_secs(self) -> i64 {
        match self {
            Self::PasswordReset => PASSWORD_RESET_TTL_SECS,
            Self::EmailVerify => EMAIL_VERIFY_TTL_SECS,
        }
    }
}
