//! Who is asking — the online daemon's identity boundary.
//!
//! Every online request is served against a tenant, and the tenant key is
//! derived from a **verified** identity and nothing else. That is the whole
//! contract of this module: a client can present a credential, but it can
//! never name the account it wants. A request body, a query string, or a
//! header the browser controls are all equally untrusted here.
//!
//! [`IdentityVerifier`] is the seam. [`AccountVerifier`] is the one
//! implementation a deployment runs — it resolves a session against this
//! deployment's own account store — and [`StaticVerifier`] is the
//! operator-written token table tests and local development use, which no
//! deployment may reach (see the online run loop's start-up check).
//!
//! [`AccountVerifier`]: super::account_verifier::AccountVerifier

use std::collections::HashMap;

use op_editor_core::access::RoleSet;

use crate::mcp_serve::tool_profile::McpScopes;
use crate::mcp_serve::HttpRequest;

/// Development-only token table: `token1=user1,token2=user2`.
///
/// See [`StaticVerifier`] for why this is not a production credential path.
pub const STATIC_IDENTITIES_ENV: &str = "OPENPENCIL_ONLINE_STATIC_IDENTITIES";

/// The cookie a signed-in browser holds.
///
/// Defined with the rest of the cookie's rules in
/// [`super::account_cookie`], and re-exported here because this is the module
/// that READS it: the request parser below is the only thing that needs the
/// name on the way in, and keeping one constant means the name a route sets
/// and the name a verifier looks for cannot drift apart.
pub use super::account_cookie::SESSION_COOKIE_NAME;

/// Longest credential this layer will even look at. Both forms are short
/// opaque tokens; a longer one is a client bug or an attempt to make the
/// verifier allocate.
pub(super) const MAX_CREDENTIAL_CHARS: usize = 4096;

/// How an identity was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityVia {
    /// A browser session cookie on the shared origin.
    SessionCookie,
    /// An `Authorization: Bearer <token>` API token.
    ApiToken,
}

/// A verified account. **The only source of a tenant key.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    /// Stable account id. This — and never anything from the request body —
    /// is what [`super::tenant::TenantRegistry`] keys on.
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    /// The product roles this account holds (#10).
    ///
    /// Carried here so the decision points downstream — the REST routes, and
    /// later the chrome — read the verified identity instead of asking again
    /// or trusting the request. They come from the account row, parsed through
    /// [`op_editor_core::access::ProductRole::from_wire`], which is what makes
    /// a role change in the store reach the route checks.
    ///
    /// Empty is normal and means "no roles", never "no access": see
    /// [`op_editor_core::access::RoleSet::rights`], which floors at view-only
    /// for its own workspace.
    pub roles: RoleSet,
    pub via: IdentityVia,
    /// What this credential may drive over MCP.
    ///
    /// A browser session is the account itself and carries full authority; a
    /// narrower credential — an API token scoped to part of the product —
    /// carries whatever the issuer gave it. Enforced in the MCP dispatch,
    /// where the tool being called is known — see
    /// `crate::mcp_serve::tool_profile`.
    pub scopes: McpScopes,
}

/// Why a request could not be attributed to an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineAuthError {
    /// No credential was presented at all.
    MissingCredential,
    /// A credential was presented but is not a well-formed one.
    MalformedCredential,
    /// A well-formed credential that no account matches.
    UnknownCredential,
    /// The daemon has no way to verify credentials (misconfiguration).
    VerifierUnavailable,
}

impl OnlineAuthError {
    /// Stable machine-readable code for a REST body.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingCredential => "unauthorized",
            Self::MalformedCredential => "malformed-credential",
            Self::UnknownCredential => "unauthorized",
            Self::VerifierUnavailable => "verifier-unavailable",
        }
    }

    /// HTTP status this failure maps to.
    ///
    /// A missing or unrecognised credential is deliberately the SAME answer:
    /// telling a caller that a token exists but is not this account's is a
    /// credential-probing oracle.
    pub const fn http_status(self) -> &'static str {
        match self {
            Self::MissingCredential | Self::UnknownCredential | Self::MalformedCredential => {
                "401 Unauthorized"
            }
            Self::VerifierUnavailable => "503 Service Unavailable",
        }
    }
}

impl std::fmt::Display for OnlineAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredential | Self::UnknownCredential => f.write_str("unauthorized"),
            Self::MalformedCredential => f.write_str("malformed credential"),
            Self::VerifierUnavailable => {
                f.write_str("this deployment cannot verify credentials right now")
            }
        }
    }
}

impl std::error::Error for OnlineAuthError {}

/// What a request carried, after the header parse and before verification.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentedCredentials {
    /// The token from `Authorization: Bearer <token>`, scheme stripped.
    pub bearer: Option<String>,
    /// The value of the session cookie, if the `Cookie` header had one.
    pub session_cookie: Option<String>,
}

impl PresentedCredentials {
    /// Extract both credential forms from a parsed request.
    ///
    /// Anything malformed is simply absent — this is a parser, not a policy.
    /// The verifier decides what a request with no usable credential means.
    pub fn from_request(request: &HttpRequest) -> Self {
        Self {
            bearer: request.authorization.as_deref().and_then(parse_bearer),
            session_cookie: request
                .cookie
                .as_deref()
                .and_then(|header| cookie_value(header, SESSION_COOKIE_NAME)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bearer.is_none() && self.session_cookie.is_none()
    }
}

impl ResolvedIdentity {
    /// The `/api/auth/status` projection for a signed-in caller.
    ///
    /// The shell's account layer polls this route to learn WHO it is showing;
    /// online used to 404 it wholesale, which left the identity epoch — and
    /// therefore the account-switch reset — permanently dormant.
    ///
    /// It is a strictly READ-ONLY projection of the caller's own identity.
    /// Signing in and out are [`super::account_routes`]' business, and they are
    /// anonymous routes: a request that carries no session is exactly the
    /// request that has to be able to ask for one.
    pub fn auth_status_json(&self) -> String {
        serde_json::json!({
            // The account UI is meaningful: this deployment has accounts.
            "available": true,
            "signed_in": true,
            // The STABLE account key — the same value the tenant registry
            // keys on, and the value the account row is identified by. The
            // shell partitions its storage by this, not by `username`: a
            // username is a handle an operator may change, and a rename would
            // silently move the tab to a new partition (losing its settings)
            // or, worse, collide with another account that later takes the old
            // handle.
            "subject": self.user_id,
            // Display only.
            "username": self.username,
            "display_name": self.display_name,
            // The roles the account holds, in the store's own spelling — see
            // [`role_wire_names`]. Carried because the shell has to draw the
            // chrome an account's rights imply, and because a role this build
            // does not recognise must be visible to whoever debugs the
            // mismatch rather than dropped on the way out.
            "roles": role_wire_names(&self.roles),
            // Not projected: whether the account has an address is not
            // something the shell needs, and the store's avatar support is a
            // later step. Absent is honest rather than fabricated.
            "primary_email": serde_json::Value::Null,
            "avatar_revision": serde_json::Value::Null,
        })
        .to_string()
    }
}

/// The `/api/auth/status` projection for a caller with no identity.
///
/// Answered with `200` rather than `401` because the shell only applies a
/// status answer that succeeded (`web_auth_sync::StatusRequestGate`), and the
/// whole point of polling this route is to find out that nobody is signed in.
/// A `401` here would leave the account UI in whatever state it was already
/// in — including the previous account's.
///
/// `available` says this deployment HAS an account store and can sign people
/// in; `needs_first_admin` says it has one and it is empty, which is the state
/// a fresh deployment is in until its operator runs `op admin create` or sets
/// the `NORKA_ADMIN_*` pair. The two are different answers to different
/// questions, and a sign-in form that could not tell them apart would offer
/// credentials it cannot check.
pub fn anonymous_auth_status_json(available: bool, needs_first_admin: bool) -> String {
    serde_json::json!({
        "available": available,
        "signed_in": false,
        "needs_first_admin": needs_first_admin,
    })
    .to_string()
}

/// A role set as the role strings the store wrote.
///
/// Recognised roles come back in their canonical spelling and unrecognised
/// ones are passed through as they were found: [`RoleSet`] keeps both, and a
/// projection that printed only the first would hide the second — which is the
/// failure #10 was about, one layer out.
fn role_wire_names(roles: &RoleSet) -> Vec<&str> {
    roles
        .roles()
        .iter()
        .map(|role| role.as_wire())
        .chain(roles.unrecognized().iter().map(String::as_str))
        .collect()
}

/// Turns a presented credential into a verified account.
///
/// Implementations must be safe to call from many connection threads at
/// once; the online accept loop shares one verifier across all of them.
pub trait IdentityVerifier: Send + Sync {
    fn resolve(
        &self,
        presented: &PresentedCredentials,
    ) -> Result<ResolvedIdentity, OnlineAuthError>;
}

/// An env-injected token→account table.
///
/// **Development and tests only.** The tokens are compared in plain text,
/// live in the process environment, and carry no expiry, no revocation and
/// no scopes. It exists so the multi-tenant plumbing — and the ~200 route
/// tests written against this seam — can be exercised without an account
/// store. It is OUR code and it stays; what must never happen is a deployment
/// reaching it by accident, so the online run loop only falls back to it when
/// no account store is configured, says so on stderr, and its start-up check
/// is asserted by tests.
pub struct StaticVerifier {
    /// token → (user id, scopes).
    entries: HashMap<String, (String, McpScopes)>,
}

impl StaticVerifier {
    /// Read the table from [`STATIC_IDENTITIES_ENV`].
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var(STATIC_IDENTITIES_ENV)
                .unwrap_or_default()
                .as_str(),
        )
    }

    /// Parse a `token=user[,token2=user2:read][,token3=user3:none]` table.
    ///
    /// The optional suffix narrows the credential so the scope paths can be
    /// exercised without an account store: `:read` mints a read-only token and
    /// `:none` a scopeless one (what a credential that names no `mcp:*` scope
    /// resolves to). A bare `user` is full authority — the operator wrote this
    /// table by hand, so that is their explicit intent. Any other suffix is
    /// rejected rather than silently granting write: a typo must not widen
    /// authority.
    ///
    /// Malformed pairs are skipped rather than failing the whole table: the
    /// failure mode of a dropped entry is "that token does not authenticate",
    /// which is the safe direction.
    pub fn parse(raw: &str) -> Self {
        let entries = raw
            .split(',')
            .filter_map(|pair| {
                let (token, account) = pair.split_once('=')?;
                let token = token.trim();
                let (user, scopes) = match account.trim().rsplit_once(':') {
                    Some((user, "read")) => (user.trim(), McpScopes::READ_ONLY),
                    Some((user, "none")) => (user.trim(), McpScopes::NONE),
                    Some((_, _)) => return None,
                    None => (account.trim(), McpScopes::FULL),
                };
                (!token.is_empty()
                    && !user.is_empty()
                    && token.chars().count() <= MAX_CREDENTIAL_CHARS)
                    .then(|| (token.to_string(), (user.to_string(), scopes)))
            })
            .collect();
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl IdentityVerifier for StaticVerifier {
    fn resolve(
        &self,
        presented: &PresentedCredentials,
    ) -> Result<ResolvedIdentity, OnlineAuthError> {
        if self.entries.is_empty() {
            return Err(OnlineAuthError::VerifierUnavailable);
        }
        if presented.is_empty() {
            return Err(OnlineAuthError::MissingCredential);
        }
        // Bearer first: an API client that also happens to carry a stale
        // browser cookie means the token, and an ambiguous answer here would
        // be a way to make one credential silently stand in for the other.
        let (credential, via) = match (&presented.bearer, &presented.session_cookie) {
            (Some(token), _) => (token, IdentityVia::ApiToken),
            (None, Some(cookie)) => (cookie, IdentityVia::SessionCookie),
            (None, None) => return Err(OnlineAuthError::MissingCredential),
        };
        if credential.chars().count() > MAX_CREDENTIAL_CHARS {
            return Err(OnlineAuthError::MalformedCredential);
        }
        let (user_id, scopes) = self
            .entries
            .get(credential.as_str())
            .ok_or(OnlineAuthError::UnknownCredential)?;
        Ok(ResolvedIdentity {
            user_id: user_id.clone(),
            username: user_id.clone(),
            display_name: user_id.clone(),
            // This table has no roles to give: it is an operator-written
            // `token=user` list for development, with no account store behind
            // it. An empty set is the honest answer and it must not narrow
            // anything — `RoleSet::rights` floors at view-only, so a static
            // deployment keeps working exactly as it did before roles existed.
            // The credential's real narrowing axis here stays `scopes`.
            roles: RoleSet::empty(),
            via,
            // A browser session is the account itself, so it carries full
            // authority however the token table classified the same string.
            scopes: match via {
                IdentityVia::SessionCookie => McpScopes::FULL,
                IdentityVia::ApiToken => *scopes,
            },
        })
    }
}

/// `Bearer <token>` → `<token>`. Scheme match is case-insensitive per
/// RFC 7235; the token itself is not touched beyond trimming whitespace.
fn parse_bearer(header: &str) -> Option<String> {
    let (scheme, token) = header.trim().split_once(char::is_whitespace)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty() && token.chars().count() <= MAX_CREDENTIAL_CHARS).then(|| token.to_string())
}

/// Pull one cookie out of a `Cookie:` header value.
fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if !key.trim().eq(name) {
            return None;
        }
        let value = value.trim();
        (!value.is_empty() && value.chars().count() <= MAX_CREDENTIAL_CHARS)
            .then(|| value.to_string())
    })
}

#[cfg(test)]
#[path = "tenant_auth_tests.rs"]
mod tests;
