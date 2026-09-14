//! Authentication HTTP route paths shared by the web shell (client) and
//! the serve-web daemon (server).
//!
//! The wasm bundle ships no identity code of its own — it drives these routes
//! and lets the daemon decide who it is talking to. Keeping the paths in one
//! wasm-clean crate both sides already depend on means the client and server
//! can never drift apart.
//!
//! Two generations of routes live here. [`LOGIN`], [`LOGOUT`], [`STATUS`] and
//! [`INVITE_ACCEPT`] are the product's own accounts: a name and a password, a
//! session cookie, and an invitation. The `LOGIN_BEGIN` / `LOGIN_STATUS` /
//! `LOGIN_CANCEL` family is the device-login pairing the daemon used to proxy
//! to a third-party identity service — the daemon no longer serves it, and the
//! constants stay only until the browser shell that calls them is replaced.

/// Sign-in popup interstitial page (auth-exempt static HTML that shows
/// a spinner until the popup is navigated to the verification URI).
pub const LOADING_PAGE: &str = "/auth/loading";

/// Prefix shared by every JSON auth API route below (used by the
/// server's sensitive-POST / CORS gating).
pub const API_PREFIX: &str = "/api/auth/";

/// `GET` — session/status snapshot (seeds `account_ui_available`).
///
/// Answers `200` for an anonymous caller too: "nobody is signed in" is an
/// answer, and a client that only applies successful answers would otherwise
/// never learn it.
pub const STATUS: &str = "/api/auth/status";

/// `POST` — sign in with a name and a password. `{"username","password"}`.
///
/// The one auth route that is meant to be reachable with no credential at all.
pub const LOGIN: &str = "/api/auth/login";

/// `POST` — accept an invitation and become an account.
/// `{"token","username","password"[,"display_name"]}`.
pub const INVITE_ACCEPT: &str = "/api/auth/invite/accept";

/// `POST` — bounded current-account avatar bytes from the same-origin daemon.
///
/// JSON POST is intentional: unlike a GET, a cross-site image element cannot
/// trigger the daemon's upstream fetch without passing the existing
/// same-origin and JSON content-type gates.
pub const AVATAR: &str = "/api/auth/avatar";

/// `POST` — begin a device-login pairing. The daemon holds the request
/// until the pairing's verification URI is known.
pub const LOGIN_BEGIN: &str = "/api/auth/login/begin";

/// `GET` — poll the in-flight login pairing for approval progress.
pub const LOGIN_STATUS: &str = "/api/auth/login/status";

/// `POST` — cancel the in-flight login pairing.
pub const LOGIN_CANCEL: &str = "/api/auth/login/cancel";

/// `POST` — sign out of the current session. `{"all":true}` ends every session
/// of the account instead of this one.
pub const LOGOUT: &str = "/api/auth/logout";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_routes_share_the_gating_prefix() {
        for route in [
            STATUS,
            LOGIN,
            INVITE_ACCEPT,
            AVATAR,
            LOGIN_BEGIN,
            LOGIN_STATUS,
            LOGIN_CANCEL,
            LOGOUT,
        ] {
            assert!(
                route.starts_with(API_PREFIX),
                "{route} outside {API_PREFIX}"
            );
        }
        // The interstitial is deliberately outside the API prefix — it is
        // an auth-exempt static page, not a JSON route.
        assert!(!LOADING_PAGE.starts_with(API_PREFIX));
    }

    #[test]
    fn no_two_routes_are_the_same_path() {
        // Both generations live in this file for now, so the one thing that
        // must not happen is one spelling answering for another. Note that
        // `/api/auth/login` IS a prefix of `/api/auth/login/begin`, and that
        // is fine: every route table in this product matches exact paths, so a
        // prefix is not a collision — but a table that ever switched to
        // prefix matching would send a pairing poll to the password form, and
        // this is where that is written down.
        let all = [
            STATUS,
            LOGIN,
            LOGOUT,
            INVITE_ACCEPT,
            AVATAR,
            LOADING_PAGE,
            LOGIN_BEGIN,
            LOGIN_STATUS,
            LOGIN_CANCEL,
        ];
        for (index, route) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(route, other, "two routes share one path");
            }
        }
    }
}
