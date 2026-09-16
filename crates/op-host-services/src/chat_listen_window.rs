//! How long the OpenCode chat server's child process has to announce itself.
//!
//! Issue #152: this window used to be a pair of constants mirrored from the
//! retired TypeScript client (5 s, 15 s on Windows), and the mirroring was
//! never re-derived from this product's own machine. Instrumenting the #133
//! fix measured single `spawn → exec → reap` cycles of **3812 / 6385 / 6444 /
//! 7420 / 7514 ms** on an 8-core machine running the crate's own tests, with
//! one cycle still running when a 10 s bound expired (~18 loadavg on 8 cores),
//! and 10 000 ms under heavier load. A five-second window therefore sits
//! *inside* the noise of the machine it runs on: the child is reported as
//! failed while it is still starting, and no caller can widen it, because the
//! product returns [`OpenCodeError::ListenTimeout`] first.
//!
//! The window is two different things at once, and both decide this file:
//!
//! 1. **A patience budget for one machine.** How long a healthy child needs
//!    depends on the host, not on the protocol, so it is a property of the
//!    DEPLOYMENT — the same conclusion the connection and sign-in ceilings
//!    reached. An operator on a slow or heavily loaded host can widen it
//!    without patching the product, and the number is printed in the daemon's
//!    start-up output instead of being discoverable only from a failure.
//! 2. **A liveness bound that must not disappear.** Waiting forever for a
//!    child that will never announce would wedge the chat turn with nothing
//!    but Stop; the timeout is what turns that into a diagnosable error. So
//!    this reader follows the rule the other ceilings follow: a value that is
//!    absent, unparsable, zero, or below [`MIN_LISTEN_WINDOW_SECS`] keeps the
//!    default, and a typo can never remove the bound.
//!
//! The default is [`DEFAULT_LISTEN_WINDOW_SECS`] — roughly four times the
//! worst measured cycle above, which is the margin the measurement supports.
//! It is one number for every platform: the old Windows split existed because
//! the TS client's Windows process spawn was slower, and a single window with
//! that headroom covers both rather than leaving a second number to keep
//! right.
//!
//! [`OpenCodeError::ListenTimeout`]: crate::chat_http_server::OpenCodeError::ListenTimeout

use std::time::Duration;

/// The variable an operator writes to change the window, in seconds.
pub const CHAT_LISTEN_WINDOW_ENV: &str = "OPENPENCIL_CHAT_LISTEN_WINDOW_SECS";

/// The window this product ships with, in seconds.
///
/// Thirty and not five: see the module docs for the measurement. Too long a
/// window costs a slower error on a child that really is broken; too short a
/// one costs a working deployment, which is what #152 was.
pub const DEFAULT_LISTEN_WINDOW_SECS: u64 = 30;

/// The smallest value this reader will take seriously, in seconds.
///
/// One, so that the setting can express "fail fast" — the point of the knob is
/// that the operator knows their host better than this file does. Zero is
/// refused with the rest because a window that has expired before the child is
/// spawned is not a setting, it is a broken deployment.
pub const MIN_LISTEN_WINDOW_SECS: u64 = 1;

/// The window this deployment runs with.
pub fn listen_window_from_env() -> Duration {
    listen_window_from(std::env::var(CHAT_LISTEN_WINDOW_ENV).ok().as_deref())
}

/// [`listen_window_from_env`] with the environment already read.
///
/// Split out so the rule above is assertable without mutating a process-wide
/// variable — the same shape `account_signin_limits` and
/// `online_identity_tier` use for theirs.
pub fn listen_window_from(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs >= MIN_LISTEN_WINDOW_SECS)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_LISTEN_WINDOW_SECS))
}

#[cfg(test)]
#[path = "chat_listen_window_tests.rs"]
mod tests;
