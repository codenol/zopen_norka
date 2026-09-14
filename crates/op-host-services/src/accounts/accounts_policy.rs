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
