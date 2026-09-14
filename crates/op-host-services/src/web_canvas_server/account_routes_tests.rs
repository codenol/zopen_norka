//! What the account routes promise, driven through `AccountAuth::handle`.
//!
//! The assertions are about the three things a sign-in surface can get wrong
//! in a way nobody notices: whether the cookie is actually set, whether a
//! refusal is the SAME refusal for every cause, and whether a failed attempt
//! left something behind. So the tests read the `Set-Cookie` header, compare
//! refusal bodies byte for byte, and then ask the store what exists.

use std::sync::Arc;

use super::*;
use crate::accounts::{NewInvite, NewUser, Session, UserStatus, INVITE_TTL_SECS, SESSION_TTL_SECS};
use crate::document_test_dir::TempDir;

const PASSWORD: &str = "correct-horse-battery-staple-norka-7";
/// A deployment that answers for a public origin: the cookie is strict, and a
/// browser POST has to come from the deployment's own page.
const ORIGIN: &str = "https://canvas.example";

/// The deployment's own origin list, as the accept loop holds it. Passed in
/// rather than read from the environment: a test that mutated a process-wide
/// variable every other test in this binary shares would be testing something
/// other than the route.
fn allowed_origins() -> Vec<String> {
    vec![ORIGIN.to_string()]
}

/// Drive one route, with the origin list the accept loop would pass.
trait HandleAsDeployment {
    fn handle_here(&self, request: &HttpRequest) -> Option<AccountReply>;
}

impl HandleAsDeployment for AccountAuth {
    fn handle_here(&self, request: &HttpRequest) -> Option<AccountReply> {
        self.handle(request, &allowed_origins())
    }
}

fn deployment() -> (TempDir, AccountAuth) {
    let dir = TempDir::new("account-routes");
    let db = AccountsDb::open(dir.path()).expect("open the account store");
    (dir, AccountAuth::new(Arc::new(db)))
}

/// One request, in the shape the connection thread builds.
fn request(method: &str, path: &str, body: &str) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        path: path.into(),
        body: body.into(),
        host: Some("canvas.example".into()),
        // A browser POST always carries an Origin, and this deployment's own
        // origin is what the sensitive-POST gate wants to see.
        origin: Some(ORIGIN.into()),
        token: None,
        content_type: Some("application/json".into()),
        authorization: None,
        cookie: None,
        query: None,
    }
}

fn with_cookie(mut request: HttpRequest, token: &str) -> HttpRequest {
    request.cookie = Some(format!(
        "{}={token}",
        super::super::account_cookie::SESSION_COOKIE_NAME
    ));
    request
}

fn login_body(username: &str, password: &str) -> String {
    serde_json::json!({ "username": username, "password": password }).to_string()
}

/// The token a `Set-Cookie` header hands the browser.
fn cookie_token(reply: &AccountReply) -> String {
    let cookie = reply.cookies.first().expect("a Set-Cookie header");
    cookie
        .split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, value)| value.to_string())
        .expect("a cookie value")
}

fn json(reply: &AccountReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).expect("the reply body is JSON")
}

/// An account that can sign in, with the roles it was made with.
fn account(auth: &AccountAuth, username: &str, roles: &[&str]) -> crate::accounts::User {
    auth.db()
        .create_user(
            &NewUser {
                id: None,
                username,
                display_name: "Person",
                email: None,
                password: Some(PASSWORD),
                roles,
            },
            crate::accounts::now_secs(),
        )
        .expect("create an account")
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn status_answers_an_anonymous_caller_instead_of_refusing() {
    let (_dir, auth) = deployment();
    let reply = auth
        .handle_here(&request("GET", auth_routes::STATUS, ""))
        .expect("the status route is ours");
    // 200, not 401: the shell only applies an answer that succeeded, and the
    // whole point of polling this route is to find out nobody is signed in.
    assert_eq!(reply.status, "200 OK");
    assert_eq!(json(&reply)["signed_in"], false);
    assert_eq!(json(&reply)["available"], true);
    // And it says the deployment has no admin yet, which is the one thing an
    // operator can act on.
    assert_eq!(json(&reply)["needs_first_admin"], true);
}

#[test]
fn status_reports_the_account_a_session_belongs_to() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &["ux_ui", "qa"]);
    let session = auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&user.id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        )
        .expect("start a session");

    let reply = auth
        .handle_here(&with_cookie(
            request("GET", auth_routes::STATUS, ""),
            &session.token,
        ))
        .expect("the status route is ours");
    assert_eq!(reply.status, "200 OK");
    let payload = json(&reply);
    assert_eq!(payload["signed_in"], true);
    // `subject` is the stable account key the shell partitions its storage by.
    assert_eq!(payload["subject"], user.id);
    assert_eq!(payload["username"], "designer");
    assert_eq!(payload["roles"][0], "ux_ui");
    assert_eq!(payload["roles"][1], "qa");
}

#[test]
fn status_is_read_only_and_sets_no_cookie() {
    let (_dir, auth) = deployment();
    let reply = auth
        .handle_here(&request("GET", auth_routes::STATUS, ""))
        .expect("the status route is ours");
    assert!(reply.cookies.is_empty());
}

// ---------------------------------------------------------------------------
// Sign in
// ---------------------------------------------------------------------------

#[test]
fn a_name_and_password_open_a_session_the_next_request_can_use() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &["ux_ui", "qa"]);

    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        ))
        .expect("the login route is ours");
    assert_eq!(reply.status, "200 OK");
    assert_eq!(json(&reply)["ok"], true);
    assert_eq!(json(&reply)["subject"], user.id);

    // The cookie is the credential, and it is the one the verifier reads.
    let cookie = reply.cookies.first().expect("a Set-Cookie header");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(cookie.contains("Path=/"), "{cookie}");
    assert!(cookie.contains("Secure"), "a public origin: {cookie}");
    assert!(
        cookie.contains(&format!("Max-Age={SESSION_TTL_SECS}")),
        "{cookie}"
    );

    let identity = auth
        .verifier()
        .resolve(&PresentedCredentials::from_request(&with_cookie(
            request("GET", "/api/mcp/document", ""),
            &cookie_token(&reply),
        )))
        .expect("the session the sign-in just made resolves");
    assert_eq!(identity.user_id, user.id);
}

#[test]
fn a_wrong_password_and_an_unknown_name_are_the_same_answer() {
    let (_dir, auth) = deployment();
    account(&auth, "designer", &[]);

    let wrong = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("designer", "not-the-password-at-all"),
        ))
        .expect("the login route is ours");
    let unknown = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("nobody", "not-the-password-at-all"),
        ))
        .expect("the login route is ours");

    // Byte for byte: a form built on this cannot be talked into telling the
    // difference between the two, which is how a login page becomes a way to
    // ask whether somebody has an account here.
    assert_eq!(wrong.status, "401 Unauthorized");
    assert_eq!(wrong.body, unknown.body);
    assert!(wrong.cookies.is_empty());
    assert!(unknown.cookies.is_empty());
}

#[test]
fn a_disabled_account_cannot_sign_in_even_with_the_right_password() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &[]);
    auth.db()
        .set_status(&user.id, UserStatus::Disabled, crate::accounts::now_secs())
        .expect("disable the account");

    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        ))
        .expect("the login route is ours");
    // Refused AFTER the password was verified, so this answer reaches only
    // somebody who already holds the password — and it is the one answer a
    // blocked person can act on.
    assert_eq!(reply.status, "403 Forbidden");
    assert_eq!(json(&reply)["error"], "account-disabled");
    assert!(reply.cookies.is_empty());
}

#[test]
fn a_deployment_with_no_accounts_says_so_instead_of_refusing_the_password() {
    let (_dir, auth) = deployment();
    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("operator", PASSWORD),
        ))
        .expect("the login route is ours");
    // Not 401: there is no password to be wrong. The message names both ways
    // to fix it, because the person reading it is the operator.
    assert_eq!(reply.status, "503 Service Unavailable");
    assert_eq!(json(&reply)["error"], "accounts-unprovisioned");
    let message = json(&reply)["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(message.contains("op admin create"), "{message}");
    assert!(message.contains("NORKA_ADMIN_USERNAME"), "{message}");
}

#[test]
fn a_sign_in_without_a_name_or_a_password_is_refused_before_the_store_is_touched() {
    let (_dir, auth) = deployment();
    account(&auth, "designer", &[]);
    for body in [
        "{}",
        r#"{"username":"designer"}"#,
        r#"{"password":"something-long-enough"}"#,
        r#"{"username":"","password":""}"#,
        "not json at all",
    ] {
        let reply = auth
            .handle_here(&request("POST", auth_routes::LOGIN, body))
            .expect("the login route is ours");
        assert_eq!(reply.status, "400 Bad Request", "{body}");
        assert!(reply.cookies.is_empty(), "{body}");
    }
}

#[test]
fn a_sign_in_from_another_origin_is_refused_before_any_session_exists() {
    let (_dir, auth) = deployment();
    account(&auth, "designer", &[]);

    let hostile = HttpRequest {
        origin: Some("https://evil.example".into()),
        ..request(
            "POST",
            auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        )
    };
    let reply = auth.handle_here(&hostile).expect("the login route is ours");
    assert_eq!(reply.status, "403 Forbidden");
    assert!(reply.cookies.is_empty());
    // Nothing was created: the refusal happens before the route reads a name.
    assert_eq!(auth.db().list_users(10, 0).expect("list").len(), 1);
    assert!(auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new("nobody", SESSION_TTL_SECS),
            crate::accounts::now_secs()
        )
        .is_err());
}

#[test]
fn a_sign_in_that_is_not_json_is_refused() {
    let (_dir, auth) = deployment();
    let mut plain = request(
        "POST",
        auth_routes::LOGIN,
        &login_body("designer", PASSWORD),
    );
    // `text/plain` is a CORS "simple request": a drive-by page can send it
    // without a preflight, so the content type is part of the boundary.
    plain.content_type = Some("text/plain".into());
    let reply = auth.handle_here(&plain).expect("the login route is ours");
    assert_eq!(reply.status, "415 Unsupported Media Type");
}

// ---------------------------------------------------------------------------
// Sign out
// ---------------------------------------------------------------------------

#[test]
fn signing_out_ends_the_session_and_clears_the_cookie() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &[]);
    let session = auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&user.id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        )
        .expect("start a session");

    let reply = auth
        .handle_here(&with_cookie(
            request("POST", auth_routes::LOGOUT, "{}"),
            &session.token,
        ))
        .expect("the logout route is ours");
    assert_eq!(reply.status, "200 OK");
    assert_eq!(json(&reply)["signed_in"], false);
    let cookie = reply.cookies.first().expect("a Set-Cookie header");
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
    // The same attributes it was set with, or the browser would keep the
    // original and the sign-out would look like it had failed.
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    assert!(auth
        .db()
        .resolve_session(&session.token, crate::accounts::now_secs())
        .expect("resolve")
        .is_none());
}

#[test]
fn signing_out_everywhere_ends_the_accounts_other_sessions_too() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &[]);
    let now = crate::accounts::now_secs();
    let laptop = auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&user.id, SESSION_TTL_SECS),
            now,
        )
        .expect("a session")
        .token;
    let phone = auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&user.id, SESSION_TTL_SECS),
            now,
        )
        .expect("another session")
        .token;

    let reply = auth
        .handle_here(&with_cookie(
            request("POST", auth_routes::LOGOUT, r#"{"all":true}"#),
            &laptop,
        ))
        .expect("the logout route is ours");
    assert_eq!(reply.status, "200 OK");
    assert!(auth
        .db()
        .resolve_session(&phone, now)
        .expect("resolve")
        .is_none());
    assert!(auth
        .db()
        .resolve_session(&laptop, now)
        .expect("resolve")
        .is_none());
    // Another account's sessions are not the caller's to end.
    let other = account(&auth, "colleague", &[]);
    let theirs = auth
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&other.id, SESSION_TTL_SECS),
            now,
        )
        .expect("their session")
        .token;
    assert!(auth
        .handle_here(&with_cookie(
            request("POST", auth_routes::LOGOUT, r#"{"all":true}"#),
            &laptop
        ))
        .is_some());
    assert!(auth
        .db()
        .resolve_session(&theirs, now)
        .expect("resolve")
        .is_some());
}

#[test]
fn signing_out_without_a_session_still_clears_the_cookie() {
    let (_dir, auth) = deployment();
    let reply = auth
        .handle_here(&request("POST", auth_routes::LOGOUT, ""))
        .expect("the logout route is ours");
    // Idempotent: a sign-out that failed because there was nothing to sign out
    // of would leave the browser holding a cookie the server had forgotten.
    assert_eq!(reply.status, "200 OK");
    assert!(reply
        .cookies
        .first()
        .is_some_and(|cookie| cookie.contains("Max-Age=0")));
}

#[test]
fn a_sign_out_with_a_body_that_is_not_json_is_refused() {
    let (_dir, auth) = deployment();
    for body in ["not json", "[]", "42"] {
        let reply = auth
            .handle_here(&request("POST", auth_routes::LOGOUT, body))
            .expect("the logout route is ours");
        assert_eq!(reply.status, "400 Bad Request", "{body}");
    }
}

// ---------------------------------------------------------------------------
// Accepting an invitation
// ---------------------------------------------------------------------------

/// Issue an invitation for `roles`, valid for `ttl_secs` from now.
fn invite(auth: &AccountAuth, roles: &[&str], ttl_secs: i64) -> String {
    auth.db()
        .create_invite(
            &NewInvite::new(roles, None, ttl_secs),
            crate::accounts::now_secs(),
        )
        .expect("issue an invitation")
        .token
}

fn acceptance_body(token: &str, username: &str, password: &str) -> String {
    serde_json::json!({ "token": token, "username": username, "password": password }).to_string()
}

#[test]
fn an_invitation_becomes_an_active_account_with_its_roles_and_a_session() {
    let (_dir, auth) = deployment();
    let token = invite(&auth, &["ux_ui", "qa"], INVITE_TTL_SECS);

    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "newcomer", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    let cookie = reply.cookies.first().expect("a Set-Cookie header");
    assert!(cookie.contains("HttpOnly"), "{cookie}");

    let user = auth
        .db()
        .find_user_by_username("newcomer")
        .expect("look up")
        .expect("the account exists");
    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(user.roles, vec!["ux_ui".to_string(), "qa".to_string()]);
    // The roles the invitation carried reach the identity, which is what makes
    // them worth carrying.
    let identity = auth
        .verifier()
        .resolve(&PresentedCredentials::from_request(&with_cookie(
            request("GET", "/api/mcp/document", ""),
            &cookie_token(&reply),
        )))
        .expect("the session the acceptance just made resolves");
    assert_eq!(identity.user_id, user.id);
    assert!(identity
        .roles
        .contains(op_editor_core::access::ProductRole::UxUi));

    // The link is spent.
    let invite = auth
        .db()
        .find_invite(&token)
        .expect("find")
        .expect("the invitation row");
    assert_eq!(invite.accepted_by.as_deref(), Some(user.id.as_str()));
}

#[test]
fn an_invitation_cannot_be_accepted_twice() {
    let (_dir, auth) = deployment();
    let token = invite(&auth, &[], INVITE_TTL_SECS);
    let first = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "newcomer", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(first.status, "200 OK", "{}", first.body);

    let second = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "someone-else", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(second.status, "409 Conflict");
    assert_eq!(json(&second)["error"], "invite-already-accepted");
    assert!(second.cookies.is_empty());
    // And no account was left behind for the second attempt: the whole point
    // of one link, one account.
    assert_eq!(auth.db().count_users().expect("count"), 1);
    assert!(auth
        .db()
        .find_user_by_username("someone-else")
        .expect("look up")
        .is_none());
}

#[test]
fn an_unknown_or_expired_invitation_is_refused_with_its_own_reason() {
    let (_dir, auth) = deployment();
    let unknown = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body("no-such-token", "newcomer", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(unknown.status, "404 Not Found");
    assert_eq!(json(&unknown)["error"], "invite-not-found");

    // Issued already expired: the acceptance path is the only place a person
    // holding a stale link finds out.
    let token = invite(&auth, &[], -1);
    let expired = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "newcomer", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(expired.status, "410 Gone");
    assert_eq!(json(&expired)["error"], "invite-expired");
    assert_eq!(auth.db().count_users().expect("count"), 0);
}

#[test]
fn a_weak_password_is_refused_before_any_account_exists() {
    let (_dir, auth) = deployment();
    let token = invite(&auth, &[], INVITE_TTL_SECS);
    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "newcomer", "short"),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(reply.status, "400 Bad Request");
    assert_eq!(json(&reply)["error"], "password-too-short");
    // No half-made account, and the link is still anybody's to accept.
    assert_eq!(auth.db().count_users().expect("count"), 0);
    assert!(auth
        .db()
        .find_invite(&token)
        .expect("find")
        .expect("the row")
        .is_redeemable_at(crate::accounts::now_secs()));
}

#[test]
fn a_name_that_is_already_taken_is_refused() {
    let (_dir, auth) = deployment();
    account(&auth, "designer", &[]);
    let token = invite(&auth, &[], INVITE_TTL_SECS);
    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "DESIGNER", PASSWORD),
        ))
        .expect("the acceptance route is ours");
    assert_eq!(reply.status, "409 Conflict");
    assert_eq!(json(&reply)["error"], "username-taken");
    assert_eq!(auth.db().count_users().expect("count"), 1);
}

#[test]
fn an_acceptance_without_its_three_fields_is_refused() {
    let (_dir, auth) = deployment();
    for body in [
        "{}",
        r#"{"token":"t"}"#,
        r#"{"token":"t","username":"newcomer"}"#,
        "not json",
    ] {
        let reply = auth
            .handle_here(&request("POST", auth_routes::INVITE_ACCEPT, body))
            .expect("the acceptance route is ours");
        assert_eq!(reply.status, "400 Bad Request", "{body}");
    }
}

#[test]
fn an_invitation_accepted_from_another_origin_is_refused() {
    let (_dir, auth) = deployment();
    let token = invite(&auth, &[], INVITE_TTL_SECS);
    // The invite token is the credential here, so a cross-site page that has
    // one (a link in a chat log) must not be able to spend it from a context
    // the invitee is not standing in.
    let hostile = HttpRequest {
        origin: Some("https://evil.example".into()),
        ..request(
            "POST",
            auth_routes::INVITE_ACCEPT,
            &acceptance_body(&token, "newcomer", PASSWORD),
        )
    };
    let reply = auth
        .handle_here(&hostile)
        .expect("the acceptance route is ours");
    assert_eq!(reply.status, "403 Forbidden");
    assert_eq!(auth.db().count_users().expect("count"), 0);
}

// ---------------------------------------------------------------------------
// Not our routes, and a store that cannot answer
// ---------------------------------------------------------------------------

#[test]
fn a_request_that_is_not_an_account_route_is_left_alone() {
    let (_dir, auth) = deployment();
    for (method, path) in [
        ("GET", "/api/mcp/document"),
        ("POST", "/api/mcp/document"),
        ("POST", "/api/auth/login/begin"),
        ("GET", "/api/auth/login/status"),
        ("POST", "/api/auth/avatar"),
        ("GET", "/auth/loading"),
        // The right path with the wrong method: the caller falls through to
        // the route table, which answers 404 rather than pretending.
        ("GET", auth_routes::LOGIN),
        ("POST", auth_routes::STATUS),
    ] {
        assert!(
            auth.handle_here(&request(method, path, "{}")).is_none(),
            "{method} {path} should not be answered by the account tier"
        );
    }
}

#[test]
fn an_unreadable_store_is_a_503_and_never_a_wrong_password() {
    let (dir, auth) = deployment();
    account(&auth, "designer", &[]);
    let other = rusqlite::Connection::open(dir.path().join("accounts.db"))
        .expect("a second connection to the same file");
    other
        .execute_batch("DROP TABLE users; DROP TABLE invites;")
        .expect("drop the tables under the store");

    for path in [auth_routes::LOGIN, auth_routes::INVITE_ACCEPT] {
        let body = if path == auth_routes::LOGIN {
            login_body("designer", PASSWORD)
        } else {
            acceptance_body("whatever", "newcomer", PASSWORD)
        };
        let reply = auth
            .handle_here(&request("POST", path, &body))
            .expect("the route is ours");
        assert_eq!(reply.status, "503 Service Unavailable", "{path}");
        assert_eq!(json(&reply)["error"], "accounts-unavailable", "{path}");
        assert!(reply.cookies.is_empty(), "{path}");
    }

    // And the status route still answers, without claiming anybody is signed
    // in: a shell that got a 5xx here would keep painting the previous
    // account.
    let status = auth
        .handle_here(&request("GET", auth_routes::STATUS, ""))
        .expect("the status route is ours");
    assert_eq!(status.status, "200 OK");
    assert_eq!(json(&status)["signed_in"], false);
    // The store cannot be counted, so the deployment is NOT reported as fresh
    // — telling an operator to create an admin they may already have is worse
    // than saying nothing.
    assert_eq!(json(&status)["needs_first_admin"], false);
}

#[test]
fn the_session_rows_a_sign_in_creates_are_the_accounts_own() {
    let (_dir, auth) = deployment();
    let user = account(&auth, "designer", &[]);
    let reply = auth
        .handle_here(&request(
            "POST",
            auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        ))
        .expect("the login route is ours");
    let session: Option<Session> = auth
        .db()
        .resolve_session(&cookie_token(&reply), crate::accounts::now_secs())
        .expect("resolve the new session");
    assert_eq!(
        session.map(|session| session.user_id),
        Some(user.id),
        "the session belongs to the account that signed in"
    );
}
