//! The session cookie: what it is called, what it says about itself, and when
//! it may be marked `Secure`.
//!
//! ## The attributes, and why each one
//!
//! * `HttpOnly` — the shell never reads this cookie (the browser attaches it),
//!   so nothing it can run needs to see it. Without this, one injected script
//!   is a stolen session.
//! * `SameSite=Lax` — the cookie does not ride along on a cross-site POST, so
//!   the classic CSRF shape (a form on another site posting to us) arrives
//!   without a credential at all. It is Lax and not Strict because Strict also
//!   withholds the cookie on an ordinary top-level navigation from a chat
//!   message, which is exactly how an invitation link is opened.
//! * `Path=/` — one deployment, one cookie. Scoping it narrower would mean the
//!   editor and the API disagreed about who is signed in.
//! * `Max-Age` — equal to the session row's own lifetime
//!   ([`crate::accounts::SESSION_TTL_SECS`]). A browser-session cookie (no
//!   `Max-Age`) would sign people out every time they closed a tab even though
//!   the row was still live, and a longer `Max-Age` would leave the browser
//!   holding a credential the server had already stopped accepting.
//!
//! ## `Secure`, and the localhost problem
//!
//! `Secure` is right whenever the cookie crosses a network: it is the attribute
//! that stops a plaintext request from carrying a session. It is also the
//! attribute that makes a local daemon unusable in a browser — a browser
//! refuses to store a `Secure` cookie set over `http://`, so a deployment on
//! the operator's own machine could never sign anybody in.
//!
//! The project already has one answer to "is this request's own origin one we
//! trust as local?", and this file reuses it rather than inventing a second:
//! [`super::origin_guard::is_loopback_web_host`] is what
//! [`super::origin_guard::sensitive_origin_allowed`] consults before it lets a
//! sensitive browser POST through, so loopback is already the trusted case for
//! the settings and credentials routes. Here it means: the cookie is
//! marked `Secure` unless this very request arrived at a loopback `Host` AND
//! (when the browser sent one) a loopback `Origin` — that is, unless the
//! browser is standing on this machine, which is the only situation where an
//! insecure cookie still has a user.
//!
//! Requiring BOTH is what keeps the exception from leaking into a public
//! deployment. A reverse proxy that forwards `Host: localhost:8080` to a
//! public address — an unusual but real misconfiguration — cannot strip
//! `Secure` on its own, because a browser POST to the public address sends
//! `Origin: https://that-address` and the second half of the test fails.
//!
//! The alternative considered and rejected: an environment flag
//! (`NORKA_INSECURE_COOKIES=1`). A flag is a second way to say what the Origin
//! already says, it has to be documented, remembered and got wrong — and the
//! failure of getting it wrong is a session cookie travelling in the clear.

use crate::accounts::SESSION_TTL_SECS;
use crate::mcp_serve::HttpRequest;

/// The cookie a signed-in browser holds.
///
/// Named for this product, with no relation to the hub's `op_hub_session`: the
/// hub is gone, its sessions are not readable here, and reusing its name would
/// leave a deployment looking for an identity it can no longer resolve.
pub const SESSION_COOKIE_NAME: &str = "norka_session";

/// The `Secure` value the attributes below are built with.
///
/// A newtype rather than a bare `bool` at each call site: the two `Set-Cookie`
/// values a route emits for one request — the session and its removal — must
/// agree, and two independent booleans is how they stop agreeing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CookieSecurity {
    secure: bool,
}

impl CookieSecurity {
    /// The security this request's cookie may be set with. See the module docs.
    pub fn for_request(request: &HttpRequest) -> Self {
        Self {
            secure: !request_stands_on_loopback(request),
        }
    }

    /// The `Secure` attribute, or nothing.
    fn attribute(self) -> &'static str {
        if self.secure {
            "; Secure"
        } else {
            ""
        }
    }
}

/// Whether the request was made by a browser standing on this machine.
///
/// `Host` is required and checked first: a request that does not name a
/// loopback host is not local whatever its `Origin` claims. `Origin` is
/// optional — a non-browser client sends none — and, when present, must also
/// be loopback, so that a request arriving through a proxy that rewrites
/// `Host` cannot talk this decision out of `Secure`.
fn request_stands_on_loopback(request: &HttpRequest) -> bool {
    let Some(host) = request.host.as_deref() else {
        return false;
    };
    if !super::origin_guard::is_loopback_web_host(host_without_port(host)) {
        return false;
    }
    match request.origin.as_deref() {
        None => true,
        Some(origin) => super::origin_guard::parse_http_origin(origin)
            .and_then(|url| url.host_str().map(str::to_string))
            .is_some_and(|host| super::origin_guard::is_loopback_web_host(&host)),
    }
}

/// A `Host` header without its port: `localhost:3100` → `localhost`,
/// `[::1]:3100` → `[::1]`.
///
/// The brackets stay on: [`super::origin_guard::is_loopback_web_host`] is the
/// predicate that knows how to read an IPv6 literal, and stripping them here
/// would leave `::1` — which every port-stripper downstream would mistake for
/// a host and a port.
fn host_without_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((head, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => head,
        _ => host,
    }
}

/// The `Set-Cookie` value that starts a session.
pub fn session_cookie(token: &str, security: CookieSecurity) -> String {
    format!(
        "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_TTL_SECS}{}",
        security.attribute()
    )
}

/// The `Set-Cookie` value that ends one.
///
/// The same attributes as [`session_cookie`], with `Max-Age=0` and an empty
/// value. A browser replaces a cookie by name, path and domain, so a clearing
/// header that named a different path would leave the original in place and
/// the sign-out would look like it had failed.
pub fn cleared_session_cookie(security: CookieSecurity) -> String {
    format!(
        "{SESSION_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}",
        security.attribute()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(host: Option<&str>, origin: Option<&str>) -> HttpRequest {
        HttpRequest {
            method: "POST".into(),
            path: "/api/auth/login".into(),
            body: String::new(),
            host: host.map(str::to_string),
            origin: origin.map(str::to_string),
            token: None,
            content_type: Some("application/json".into()),
            authorization: None,
            cookie: None,
            user_agent: None,
            query: None,
        }
    }

    #[test]
    fn a_public_request_always_gets_a_secure_cookie() {
        for (host, origin) in [
            (Some("norka.example"), Some("https://norka.example")),
            (
                Some("norka.example:8443"),
                Some("https://norka.example:8443"),
            ),
            (Some("127.0.0.1"), Some("https://norka.example")),
            (Some("localhost"), Some("https://norka.example")),
            // No Origin at all: a non-browser client still gets `Secure`, since
            // nothing said this was a local browser.
            (Some("norka.example"), None),
            // And a request that names no host at all is not local either.
            (None, Some("http://localhost:3100")),
        ] {
            let security = CookieSecurity::for_request(&request(host, origin));
            let cookie = session_cookie("tok", security);
            assert!(
                cookie.contains("; Secure"),
                "{host:?} / {origin:?} should not weaken the cookie: {cookie}"
            );
            assert!(cleared_session_cookie(security).contains("; Secure"));
        }
    }

    #[test]
    fn a_browser_on_this_machine_gets_a_cookie_it_can_actually_store() {
        // Over plain http a browser refuses a `Secure` cookie, so a local
        // daemon would be unusable — this is the whole reason the exception
        // exists.
        for (host, origin) in [
            (Some("localhost:3100"), Some("http://localhost:3100")),
            (Some("127.0.0.1:3100"), Some("http://127.0.0.1:3100")),
            (Some("[::1]:3100"), Some("http://[::1]:3100")),
            (Some("norka.localhost"), Some("http://norka.localhost:3100")),
            // A native client on this machine sends no Origin.
            (Some("127.0.0.1:3100"), None),
        ] {
            let security = CookieSecurity::for_request(&request(host, origin));
            let cookie = session_cookie("tok", security);
            assert!(
                !cookie.contains("Secure"),
                "{host:?} / {origin:?} is a local browser: {cookie}"
            );
            // Everything else is unweakened: the exception is one attribute.
            assert!(cookie.contains("HttpOnly"));
            assert!(cookie.contains("SameSite=Lax"));
            assert!(cookie.contains("Path=/"));
        }
    }

    #[test]
    fn a_proxy_that_rewrites_host_to_localhost_cannot_strip_secure() {
        // The deployment is public, the browser is public, but something in
        // front of the daemon rewrote the authority. The Origin is the half of
        // the test that survives that, and it fails closed.
        let security = CookieSecurity::for_request(&request(
            Some("localhost:8080"),
            Some("https://norka.example"),
        ));
        assert!(session_cookie("tok", security).contains("; Secure"));
    }

    #[test]
    fn the_cookie_attributes_are_the_ones_the_sign_out_must_match() {
        // A public origin, so the attributes are the strict ones.
        let security = CookieSecurity::for_request(&request(Some("norka.example"), None));
        let set = session_cookie("tok", security);
        let cleared = cleared_session_cookie(security);
        assert!(set.starts_with(&format!("{SESSION_COOKIE_NAME}=tok;")));
        assert!(set.contains("HttpOnly; SameSite=Lax; Path=/"));
        assert!(set.contains(&format!("Max-Age={SESSION_TTL_SECS}")));
        // Clearing differs in exactly two ways: no value, and no lifetime.
        assert!(cleared.starts_with(&format!("{SESSION_COOKIE_NAME}=;")));
        assert!(cleared.contains("Max-Age=0"));
        for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
            assert_eq!(
                set.contains(attribute),
                cleared.contains(attribute),
                "{attribute}"
            );
        }
    }

    #[test]
    fn the_cookie_is_named_for_this_product_and_not_for_the_hub() {
        // Old hub sessions are not readable here, and a shared name would make
        // the deployment present a stale cookie to a verifier that cannot
        // resolve it.
        assert_eq!(SESSION_COOKIE_NAME, "norka_session");
        assert_ne!(SESSION_COOKIE_NAME, "op_hub_session");
    }

    #[test]
    fn a_host_header_is_read_with_or_without_a_port() {
        assert_eq!(host_without_port("localhost"), "localhost");
        assert_eq!(host_without_port("localhost:3100"), "localhost");
        assert_eq!(host_without_port("[::1]:3100"), "[::1]");
        // An IPv6 literal with no port keeps both its brackets and its colons.
        assert_eq!(host_without_port("[::1]"), "[::1]");
        // A trailing colon is not a port, so it is not treated as one.
        assert_eq!(host_without_port("norka.example:"), "norka.example:");
    }
}
