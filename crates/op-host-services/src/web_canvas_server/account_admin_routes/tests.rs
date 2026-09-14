//! What the administration routes promise, driven through `AccountAuth::handle`
//! exactly as the online accept loop drives them.
//!
//! The assertions are about the four things this family can get wrong in a way
//! nobody notices: whether an invitation really is the only place its token
//! exists, whether the roles an invitation carries reach the account it makes,
//! whether an ordinary signed-in colleague is refused, and whether a listing
//! tells the truth about a link that has expired or been used.

use std::sync::Arc;

use super::*;
use crate::accounts::{AccountsDb, NewInvite, NewSession, NewUser, UserStatus, SESSION_TTL_SECS};
use crate::document_test_dir::TempDir;
use op_editor_core::route;

const PASSWORD: &str = "correct-horse-battery-staple-norka-7";
/// A deployment that answers for a public origin: the sensitive-POST gate
/// applies to every write below.
const ORIGIN: &str = "https://canvas.example";

fn allowed_origins() -> Vec<String> {
    vec![ORIGIN.to_string()]
}

fn handle(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    auth.handle(request, &allowed_origins()).unwrap_or_else(|| {
        panic!(
            "{} {} is not this tier's route",
            request.method, request.path
        )
    })
}

fn deployment() -> (TempDir, AccountAuth) {
    let dir = TempDir::new("account-admin-routes");
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
        origin: Some(ORIGIN.into()),
        token: None,
        content_type: Some("application/json".into()),
        authorization: None,
        cookie: None,
        query: None,
    }
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

/// A signed-in account, and the request builder that speaks as it.
struct SignedIn {
    id: String,
    token: String,
}

/// Create `username` with `roles` and start a session for them.
fn sign_in(auth: &AccountAuth, username: &str, roles: &[&str]) -> SignedIn {
    let user = account(auth, username, roles);
    let session = auth
        .db()
        .create_session(
            &NewSession::new(&user.id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        )
        .expect("start a session");
    SignedIn {
        id: user.id,
        token: session.token,
    }
}

/// A request carrying `who`'s session cookie.
fn as_account(who: &SignedIn, method: &str, path: &str, body: &str) -> HttpRequest {
    let mut request = request(method, path, body);
    request.cookie = Some(format!(
        "{}={}",
        super::super::account_cookie::SESSION_COOKIE_NAME,
        who.token
    ));
    request
}

fn issue_body(roles: &[&str]) -> String {
    serde_json::json!({ "roles": roles }).to_string()
}

/// The token out of an issuance answer, as the operator would receive it.
fn issued_token(reply: &AccountReply) -> String {
    json(reply)["token"]
        .as_str()
        .expect("the answer carries the token once")
        .to_string()
}

#[path = "tests_access.rs"]
mod access;
#[path = "tests_accounts.rs"]
mod accounts;
#[path = "tests_gates.rs"]
mod gates;
#[path = "tests_issuing.rs"]
mod issuing;
#[path = "tests_listings.rs"]
mod listings;
