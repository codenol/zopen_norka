//! Signing in, signing out, and accepting an invitation.
//!
//! ## Why these are the only routes served BEFORE the identity check
//!
//! Every other online route is dispatched against a verified identity. These
//! cannot be: the request that asks for a session is by definition the request
//! that does not have one. So they are dispatched ahead of the verifier, in
//! the same anonymous prefix that serves the page and its bundle — and being
//! anonymous, each of them carries its own gate:
//!
//! * the sensitive-POST Origin and content-type check
//!   ([`super::origin_guard::sensitive_post_refusal`]), the same one the
//!   credentialed routes get;
//! * a credential where one is needed (sign-out), and the invite token as the
//!   credential where the point is that nobody is signed in yet (acceptance).
//!
//! ## What the routes do NOT decide
//!
//! Nothing about whether a name and password open an account: that is
//! [`AccountsDb::authenticate`](crate::accounts::AccountsDb::authenticate),
//! and this module maps its three outcomes onto three answers. Likewise the
//! account store refuses a `disabled` account, refuses a username that is
//! taken, and refuses an invitation that is spent or expired — each as a typed
//! error this tier translates. A route that re-decided any of it would be a
//! second place to look when the first one is wrong.
//!
//! ## Why the answers are shaped like `/api/auth/status`
//!
//! A successful sign-in and a successful acceptance both return the caller's
//! own identity in the same projection the status route uses
//! ([`ResolvedIdentity::auth_status_json`]). The browser shell can then paint
//! the account from whichever answer it received without a second round trip,
//! and there is one shape to keep right instead of three.

use std::sync::Arc;

use crate::accounts::{AccountsDb, AccountsError, NewSession, NewUser, SESSION_TTL_SECS};
use op_editor_core::auth_routes;

use super::account_cookie::{cleared_session_cookie, session_cookie, CookieSecurity};
use super::account_verifier::AccountVerifier;
use super::tenant_auth::{anonymous_auth_status_json, IdentityVerifier, PresentedCredentials};
use crate::mcp_serve::HttpRequest;

/// This deployment's account tier: the store, the verifier built on it, and the
/// routes that start and end sessions.
///
/// One struct because the three are one thing — the same store answers all of
/// them, and a deployment either has accounts (all three) or does not (none).
/// `None` is a real deployment state: a daemon started without a data
/// directory serves documents and cannot sign anybody in, which its status
/// route says out loud.
#[derive(Clone)]
pub struct AccountAuth {
    db: Arc<AccountsDb>,
}

/// One answer from the account tier, `Set-Cookie` included.
///
/// Not [`super::WebReply`] because that type has no room for headers and the
/// whole point of half these routes is the cookie. Keeping it separate also
/// keeps the cookie out of the reply type every other route shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountReply {
    pub status: &'static str,
    pub body: String,
    /// `Set-Cookie` values in the order they must be sent. Empty for a route
    /// that sets none.
    pub cookies: Vec<String>,
}

impl AccountReply {
    /// A JSON answer with no cookie.
    fn json(status: &'static str, body: String) -> Self {
        Self {
            status,
            body,
            cookies: Vec::new(),
        }
    }

    /// A coded refusal, in the shape every other daemon route refuses in.
    fn refusal(status: &'static str, code: &str, message: &str) -> Self {
        Self::json(
            status,
            serde_json::json!({
                "ok": false,
                "error": code,
                "message": message,
            })
            .to_string(),
        )
    }

    /// A sign-in answer: the caller's own identity, plus the cookie that makes
    /// it true for the next request.
    fn signed_in(identity: &super::tenant_auth::ResolvedIdentity, cookie: String) -> Self {
        let mut body: serde_json::Value =
            serde_json::from_str(&identity.auth_status_json()).expect("the projection is JSON");
        body["ok"] = serde_json::Value::Bool(true);
        Self {
            status: "200 OK",
            body: body.to_string(),
            cookies: vec![cookie],
        }
    }
}

impl AccountAuth {
    pub fn new(db: Arc<AccountsDb>) -> Self {
        Self { db }
    }

    /// The deployment's account tier, when it has a data directory.
    ///
    /// `Ok(None)` is a deployment without accounts — see the struct docs. An
    /// `Err` is a directory that is configured and unusable, which the caller
    /// must not paper over: the difference between "no accounts" and "accounts
    /// I cannot open" is the difference between a deployment that works and
    /// one that must say so.
    pub fn open_from_env() -> Result<Option<Self>, AccountsError> {
        Ok(AccountsDb::open_from_env()?.map(|db| Self::new(Arc::new(db))))
    }

    pub fn db(&self) -> &Arc<AccountsDb> {
        &self.db
    }

    /// The verifier this deployment's identities come from.
    ///
    /// Built on demand: it is an `Arc` clone of the same store, and a verifier
    /// that held a second copy of the store would be a second store.
    pub fn verifier(&self) -> AccountVerifier {
        AccountVerifier::new(Arc::clone(&self.db))
    }

    /// Answer one request, or `None` when it is not an account route.
    ///
    /// Returning `None` rather than a 404 is what lets the caller run this tier
    /// ahead of everything else without it swallowing the rest of the route
    /// table.
    ///
    /// `allowed_origins` is the deployment's own origin list, exactly as the
    /// accept loop holds it: the same list decides whether a cookie-carrying
    /// write may proceed, and a sign-in is a write against the caller's
    /// account.
    pub fn handle(
        &self,
        request: &HttpRequest,
        allowed_origins: &[String],
    ) -> Option<AccountReply> {
        let route = AccountRoute::of(request)?;
        if route.is_write() {
            // A write against the caller's OWN account, so it carries the same
            // gate every other sensitive POST does before it is allowed to
            // read its body. Applied here because this tier runs above the
            // tier that normally applies it — and applied to the whole family
            // rather than per route, so a route added later cannot miss it.
            if let Some((status, message)) =
                super::origin_guard::sensitive_post_refusal(request, allowed_origins)
            {
                return Some(AccountReply::refusal(status, "origin-refused", message));
            }
        }
        Some(match route {
            AccountRoute::Status => self.status(request),
            AccountRoute::Login => self.login(request),
            AccountRoute::Logout => self.logout(request),
            AccountRoute::AcceptInvite => self.accept_invite(request),
        })
    }

    /// `GET /api/auth/status` — who, if anyone, this request belongs to.
    ///
    /// Always `200`, including for an anonymous caller: see
    /// [`anonymous_auth_status_json`]. A credential that does not resolve is
    /// not an error here — the answer to "who am I" is "nobody", which is
    /// exactly what the shell needs to hear to show the sign-in form.
    fn status(&self, request: &HttpRequest) -> AccountReply {
        let presented = PresentedCredentials::from_request(request);
        if let Ok(identity) = self.verifier().resolve(&presented) {
            return AccountReply::json("200 OK", identity.auth_status_json());
        }
        AccountReply::json(
            "200 OK",
            anonymous_auth_status_json(true, self.needs_first_admin()),
        )
    }

    /// `POST /api/auth/login` — a name and a password for a session.
    fn login(&self, request: &HttpRequest) -> AccountReply {
        let Some(fields) = LoginRequest::parse(&request.body) else {
            return AccountReply::refusal(
                "400 Bad Request",
                "malformed-body",
                "expected a JSON object with `username` and `password`",
            );
        };
        // A deployment nobody has provisioned cannot sign anybody in, and
        // "wrong password" would be a lie: there is no password. Said out loud
        // — including how to fix it — because the operator sees this answer
        // and the alternative is a login form that refuses everything.
        if self.needs_first_admin() {
            return AccountReply::refusal(
                "503 Service Unavailable",
                "accounts-unprovisioned",
                "this deployment has no accounts yet: run `op admin create`, or set \
                 NORKA_ADMIN_USERNAME and NORKA_ADMIN_PASSWORD and restart",
            );
        }
        match self.db.authenticate(
            &fields.username,
            &fields.password,
            crate::accounts::now_secs(),
        ) {
            Err(_) => AccountReply::refusal(
                "503 Service Unavailable",
                "accounts-unavailable",
                "the account store cannot be read right now",
            ),
            // One answer for "no such account" and "wrong password", because
            // that is the whole point of the store returning one outcome.
            Ok(crate::accounts::SignInOutcome::Rejected) => AccountReply::refusal(
                "401 Unauthorized",
                "unauthorized",
                "the name and password do not open an account",
            ),
            // Reached only by a caller that already proved it holds this
            // account's password, so it is not an oracle: it is the one answer
            // a blocked person can act on.
            Ok(crate::accounts::SignInOutcome::Blocked(status)) => AccountReply::refusal(
                "403 Forbidden",
                "account-disabled",
                &format!("this account is {} and may not sign in", status.as_str()),
            ),
            Ok(crate::accounts::SignInOutcome::SignedIn(user)) => {
                self.start_session(&user.id, request)
            }
        }
    }

    /// `POST /api/auth/logout` — end this session, or every session of the
    /// account with `{"all":true}`.
    ///
    /// Idempotent: signing out without a session, or twice, is still `200` and
    /// still clears the cookie. A sign-out that failed because there was
    /// nothing to sign out of would leave the browser holding a cookie the
    /// server had already forgotten, which is the state sign-out exists to
    /// leave behind.
    fn logout(&self, request: &HttpRequest) -> AccountReply {
        let Some(all) = LogoutRequest::parse(&request.body) else {
            return AccountReply::refusal(
                "400 Bad Request",
                "malformed-body",
                "expected a JSON object, optionally with `all`",
            );
        };
        let security = CookieSecurity::for_request(request);
        let presented = PresentedCredentials::from_request(request);
        let credential = presented
            .bearer
            .as_deref()
            .or(presented.session_cookie.as_deref());
        if let Some(credential) = credential {
            // Whether this is "everywhere" is a decision about the ACCOUNT, so
            // the session has to be resolved first to learn whose it is. A
            // credential that resolves to nothing is not an error: the answer
            // to logout is the same either way, and the cookie goes.
            let session = self
                .db
                .resolve_session(credential, crate::accounts::now_secs())
                .ok()
                .flatten();
            let ended = match (&session, all) {
                (Some(session), true) => self.db.revoke_user_sessions(&session.user_id),
                _ => self.db.revoke_session(credential).map(u64::from),
            };
            if ended.is_err() {
                return AccountReply::refusal(
                    "503 Service Unavailable",
                    "accounts-unavailable",
                    "the account store cannot be written right now",
                );
            }
            // A sign-out that failed to reach the store must NOT clear the
            // cookie: the cookie is the only thing that still names the
            // session somebody may have to revoke by hand.
        }
        let cookie = cleared_session_cookie(security);
        let body = serde_json::json!({ "ok": true, "signed_in": false }).to_string();
        AccountReply {
            status: "200 OK",
            body,
            cookies: vec![cookie],
        }
    }

    /// `POST /api/auth/invite/accept` — an invitation becomes an account.
    ///
    /// The sequence, and why it is this one:
    ///
    /// 1. read the invitation (its roles are needed to create the account, and
    ///    a link that names nothing is answered before anything is written);
    /// 2. refuse a password this product will not accept — before any row
    ///    exists, so a refused password cannot leave a half-made account;
    /// 3. create the account as `invited`: it exists, it has no password, and
    ///    the store's own rules make it unable to sign in. This is the point
    ///    of the order — **a usable password never exists for a link nobody
    ///    has claimed yet**;
    /// 4. claim the invitation (one guarded `UPDATE` in the store: one link,
    ///    one account, whatever two simultaneous requests do);
    /// 5. only now set the password and make the account active.
    ///
    /// If step 4 loses the race, the account made in step 3 is deleted again —
    /// it never held a password, so the worst a failed compensation can leave
    /// behind is an inert row an operator can see and remove. The reverse
    /// order cannot promise that: an account created with its password first
    /// is a second usable account for one invitation.
    fn accept_invite(&self, request: &HttpRequest) -> AccountReply {
        let Some(fields) = InviteAcceptance::parse(&request.body) else {
            return AccountReply::refusal(
                "400 Bad Request",
                "malformed-body",
                "expected a JSON object with `token`, `username` and `password`",
            );
        };
        let now = crate::accounts::now_secs();
        let invite = match self.db.find_invite(&fields.token) {
            Err(_) => return self.store_unavailable(),
            Ok(None) => {
                return AccountReply::refusal(
                    "404 Not Found",
                    "invite-not-found",
                    "this invitation does not exist",
                )
            }
            Ok(Some(invite)) => invite,
        };
        // Answered from the store's own predicate, and answered EARLY only so
        // that a dead link does not create a row first. The authority is the
        // guarded update in step 4; this must never be the check that decides.
        if !invite.is_redeemable_at(now) {
            // Which of the two it is comes from the row, because the two are
            // different answers for the person holding the link: "already used"
            // and "too late" are the only things they can act on.
            return if invite.accepted_at.is_some() {
                AccountReply::refusal(
                    "409 Conflict",
                    "invite-already-accepted",
                    &AccountsError::InviteAlreadyAccepted.to_string(),
                )
            } else {
                AccountReply::refusal(
                    "410 Gone",
                    "invite-expired",
                    &AccountsError::InviteExpired.to_string(),
                )
            };
        }
        if let Err(weak) =
            crate::accounts::check_password_strength(&fields.password, &fields.username)
        {
            return AccountReply::refusal("400 Bad Request", weak.code(), &weak.to_string());
        }
        let display_name = fields
            .display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&fields.username);
        let roles: Vec<&str> = invite.roles.iter().map(String::as_str).collect();
        let mut new_user = NewUser::invited(&fields.username, display_name);
        new_user.roles = &roles;
        let user = match self.db.create_user(&new_user, now) {
            Ok(user) => user,
            Err(error) => return self.account_error_reply(error),
        };
        // The one step that decides whether this invitation is still anybody's
        // to accept.
        if let Err(error) = self.db.redeem_invite(&fields.token, &user.id, now) {
            // Compensation. The account was created moments ago, it has no
            // password, and `delete_user` also takes the sessions it never had
            // — a failure here leaves an inert row, never a usable account.
            let _ = self.db.delete_user(&user.id);
            return match error {
                AccountsError::InviteAlreadyAccepted => AccountReply::refusal(
                    "409 Conflict",
                    "invite-already-accepted",
                    &error.to_string(),
                ),
                AccountsError::InviteExpired => {
                    AccountReply::refusal("410 Gone", "invite-expired", &error.to_string())
                }
                AccountsError::InviteNotFound => {
                    AccountReply::refusal("404 Not Found", "invite-not-found", &error.to_string())
                }
                other => self.account_error_reply(other),
            };
        }
        // From here the invitation is spent: the account exists and belongs to
        // whoever holds this password. A store fault now leaves an invited
        // account with no password — inert, visible in the table, and fixable
        // by an operator — rather than a spent link with a working account.
        if let Err(error) = self
            .db
            .set_password(&user.id, &fields.password, now)
            .and_then(|()| {
                self.db
                    .set_status(&user.id, crate::accounts::UserStatus::Active, now)
            })
        {
            return self.account_error_reply(error);
        }
        self.start_session(&user.id, request)
    }

    /// Create the session and hand back the answer that carries its cookie.
    fn start_session(&self, user_id: &str, request: &HttpRequest) -> AccountReply {
        // The store's default lifetime, not a number invented here: the
        // cookie's `Max-Age` is derived from the same constant, so the browser
        // stops presenting a credential at the moment the server stops
        // accepting it.
        let issued = match self.db.create_session(
            &NewSession::new(user_id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        ) {
            Ok(issued) => issued,
            Err(_) => return self.store_unavailable(),
        };
        let identity = match self.db.find_user_by_id(user_id).ok().flatten() {
            Some(user) => super::account_verifier::identity_from_user(
                user,
                super::tenant_auth::IdentityVia::SessionCookie,
            ),
            None => return self.store_unavailable(),
        };
        AccountReply::signed_in(
            &identity,
            session_cookie(&issued.token, CookieSecurity::for_request(request)),
        )
    }

    /// Whether this deployment has no accounts at all.
    ///
    /// A store that cannot be counted reads as "not empty": claiming a fresh
    /// deployment during a database fault would tell an operator to create an
    /// admin they already have.
    fn needs_first_admin(&self) -> bool {
        self.db
            .count_users()
            .map(|count| count == 0)
            .unwrap_or(false)
    }

    /// The answer a store failure earns.
    ///
    /// Never "your credentials are wrong": a store that cannot be read is a
    /// deployment that cannot answer, and telling a browser otherwise signs
    /// everyone out for the duration of a disk fault.
    fn store_unavailable(&self) -> AccountReply {
        AccountReply::refusal(
            "503 Service Unavailable",
            "accounts-unavailable",
            "the account store cannot be read right now",
        )
    }

    /// Map a store error onto the answer its cause deserves.
    fn account_error_reply(&self, error: AccountsError) -> AccountReply {
        match error {
            // The caller chose a name somebody already has. Not a fault, and
            // not a secret: the invite named the account, so there is nothing
            // here an enumeration would learn.
            AccountsError::UsernameTaken { .. } => {
                AccountReply::refusal("409 Conflict", "username-taken", &error.to_string())
            }
            // A name, address or role the store will not accept as written.
            AccountsError::InvalidText { .. } | AccountsError::EmptyPassword => {
                AccountReply::refusal("400 Bad Request", "invalid-account", &error.to_string())
            }
            _ => self.store_unavailable(),
        }
    }
}

/// The account tier's route table.
///
/// One enum because the tier has to answer "is this mine?" before it reads
/// anything, and because the sensitive-POST gate belongs to a CLASS of these
/// routes (the writes) rather than to each of them by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountRoute {
    /// `GET /api/auth/status` — a read, and the one route here that must answer
    /// a caller who has no credential at all.
    Status,
    /// `POST /api/auth/login`.
    Login,
    /// `POST /api/auth/logout`.
    Logout,
    /// `POST /api/auth/invite/accept`.
    AcceptInvite,
}

impl AccountRoute {
    /// The route this request is, or `None` when it belongs to someone else.
    ///
    /// Exact paths and exact methods: an account route that also matched a
    /// prefix would answer for whatever else starts the same way, and
    /// `/api/auth/login/begin` does start the same way.
    fn of(request: &HttpRequest) -> Option<Self> {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", auth_routes::STATUS) => Some(Self::Status),
            ("POST", auth_routes::LOGIN) => Some(Self::Login),
            ("POST", auth_routes::LOGOUT) => Some(Self::Logout),
            ("POST", auth_routes::INVITE_ACCEPT) => Some(Self::AcceptInvite),
            _ => None,
        }
    }

    /// Whether this route changes the caller's account, and so must pass the
    /// cross-origin and content-type gate first.
    const fn is_write(self) -> bool {
        !matches!(self, Self::Status)
    }
}

/// `POST /api/auth/login` body.
struct LoginRequest {
    username: String,
    password: String,
}

impl LoginRequest {
    fn parse(body: &str) -> Option<Self> {
        let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
        Some(Self {
            username: required_text(&parsed, "username")?,
            password: required_text(&parsed, "password")?,
        })
    }
}

/// `POST /api/auth/invite/accept` body.
struct InviteAcceptance {
    token: String,
    username: String,
    password: String,
    /// Optional: the invitee's own name for themselves. Falls back to the
    /// username, which is what the first admin gets too.
    display_name: Option<String>,
}

impl InviteAcceptance {
    fn parse(body: &str) -> Option<Self> {
        let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
        Some(Self {
            token: required_text(&parsed, "token")?,
            username: required_text(&parsed, "username")?,
            password: required_text(&parsed, "password")?,
            display_name: parsed["display_name"].as_str().map(str::to_string),
        })
    }
}

/// `POST /api/auth/logout` body: optional, and absent means "this session".
struct LogoutRequest;

impl LogoutRequest {
    /// `Some(true)` for every session, `Some(false)` for this one, `None` for a
    /// body that is not JSON at all.
    ///
    /// An empty body is `false` rather than `None`: a POST with nothing in it
    /// is a sign-out of the current session, which is what every client that
    /// predates the `all` flag sends.
    fn parse(body: &str) -> Option<bool> {
        if body.trim().is_empty() {
            return Some(false);
        }
        let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
        if !parsed.is_object() {
            return None;
        }
        Some(parsed["all"].as_bool().unwrap_or(false))
    }
}

/// A required non-empty string field.
fn required_text(value: &serde_json::Value, field: &str) -> Option<String> {
    value[field]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[path = "account_routes_tests.rs"]
mod tests;
