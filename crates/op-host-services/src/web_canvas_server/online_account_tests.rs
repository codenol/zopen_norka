//! The account tier, driven through the accept loop.
//!
//! The unit tests in `account_routes_tests` prove what each route answers. What
//! only these can prove is WHERE the tier runs: ahead of the identity check, in
//! the anonymous prefix — because a request that is refused for having no
//! session before it is routed is a request nobody can ever sign in with.
//!
//! So each test here starts from a real request on the wire, with no credential
//! at all, and finishes by using what the answer handed back.

use std::sync::Arc;

use super::*;
use crate::accounts::{AccountsDb, NewUser, SESSION_TTL_SECS};
use crate::document_test_dir::TempDir;
use crate::web_canvas_server::account_routes::AccountAuth;
use crate::web_canvas_server::online_identity_tier::identity_tier;

const PASSWORD: &str = "correct-horse-battery-staple-norka-7";

/// A deployment with its own accounts, and the verifier built on them.
fn deployment() -> (TempDir, AccountAuth, AccountVerifier) {
    let dir = TempDir::new("online-accounts");
    let db = AccountsDb::open(dir.path()).expect("open the account store");
    let accounts = AccountAuth::new(Arc::new(db));
    let verifier = accounts.verifier();
    (dir, accounts, verifier)
}

fn account(accounts: &AccountAuth, username: &str, roles: &[&str]) -> crate::accounts::User {
    accounts
        .db()
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

fn login_body(username: &str, password: &str) -> String {
    serde_json::json!({ "username": username, "password": password }).to_string()
}

/// The cookie value a response handed the browser, if it set one.
fn set_cookie(response: &str) -> Option<String> {
    response
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .and_then(|line| line.split_once('='))
        .map(|(_, rest)| {
            rest.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
}

#[test]
fn a_sign_in_on_the_anonymous_prefix_authenticates_the_next_request() {
    let (_dir, accounts, verifier) = deployment();
    let user = account(&accounts, "designer", &["ux_ui"]);
    let registry = registry();

    // No credential at all: this is the request the old loop answered with 401
    // before it ever looked at the route table.
    let signed_in = serve_as(
        &registry,
        // A verifier that resolves nothing: the account tier must not depend on
        // it, and the loop's own gate must not be what admits this request.
        &StaticVerifier::parse(""),
        Some(&accounts),
        Request::json(
            "POST",
            op_editor_core::auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        )
        .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&signed_in), "HTTP/1.1 200 OK", "{signed_in}");
    let token = set_cookie(&signed_in).expect("the sign-in set a session cookie");
    assert!(body(&signed_in)["signed_in"].as_bool().unwrap_or(false));

    // The session the answer handed back is the one this deployment's verifier
    // resolves, and it names this account's tenant.
    let read = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::new("GET", "/api/mcp/document").with_session(&token),
    );
    assert_eq!(status_line(&read), "HTTP/1.1 200 OK", "{read}");
    assert_eq!(
        registry
            .lease_for(
                &verifier
                    .resolve(&PresentedCredentials {
                        bearer: None,
                        session_cookie: Some(token),
                    })
                    .expect("the session resolves")
            )
            .expect("a lease")
            .owner_id(),
        user.id
    );
}

#[test]
fn a_deployment_with_accounts_answers_the_status_route_anonymously() {
    let (_dir, accounts, verifier) = deployment();
    let response = serve_as(
        &registry(),
        &verifier,
        Some(&accounts),
        Request::new("GET", op_editor_core::auth_routes::STATUS),
    );
    // The loop used to refuse this with 401 before dispatch, which left the
    // shell unable to tell "signed out" from "the daemon is unhappy".
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
    assert_eq!(body(&response)["signed_in"], false);
    assert_eq!(body(&response)["available"], true);
    assert_eq!(body(&response)["needs_first_admin"], true);
}

#[test]
fn signing_out_through_the_loop_ends_the_session_the_cookie_opened() {
    let (_dir, accounts, verifier) = deployment();
    account(&accounts, "designer", &[]);
    let registry = registry();
    let signed_in = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json(
            "POST",
            op_editor_core::auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        )
        .with_origin(PUBLIC_ORIGIN),
    );
    let token = set_cookie(&signed_in).expect("a session cookie");

    let signed_out = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json("POST", op_editor_core::auth_routes::LOGOUT, "{}")
            .with_session(&token)
            .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&signed_out), "HTTP/1.1 200 OK", "{signed_out}");
    assert!(
        set_cookie(&signed_out).is_some_and(|cookie| cookie.is_empty()),
        "the cookie is cleared: {signed_out}"
    );

    // And the token that cookie carried opens nothing any more.
    let after = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::new("GET", "/api/mcp/document").with_session(&token),
    );
    assert_eq!(status_line(&after), "HTTP/1.1 401 Unauthorized", "{after}");
}

#[test]
fn two_accounts_that_sign_in_get_two_tenants() {
    let (_dir, accounts, verifier) = deployment();
    account(&accounts, "designer", &[]);
    account(&accounts, "colleague", &[]);
    let registry = registry();

    let mut tokens = Vec::new();
    for username in ["designer", "colleague"] {
        let signed_in = serve_as(
            &registry,
            &verifier,
            Some(&accounts),
            Request::json(
                "POST",
                op_editor_core::auth_routes::LOGIN,
                &login_body(username, PASSWORD),
            )
            .with_origin(PUBLIC_ORIGIN),
        );
        assert_eq!(status_line(&signed_in), "HTTP/1.1 200 OK", "{signed_in}");
        tokens.push(set_cookie(&signed_in).expect("a session cookie"));
    }

    // One account writes a document; the other must not see it. The isolation
    // the static-verifier tests cover has to hold for accounts this deployment
    // made itself.
    let pushed = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session(&tokens[0])
            .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&pushed), "HTTP/1.1 200 OK", "{pushed}");

    let theirs = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::new("GET", "/api/mcp/document").with_session(&tokens[1]),
    );
    assert_eq!(status_line(&theirs), "HTTP/1.1 200 OK", "{theirs}");
    assert!(
        !theirs.contains("Tenant Rect"),
        "one account's document reached another: {theirs}"
    );
}

#[test]
fn a_deployment_with_accounts_still_refuses_a_credential_it_cannot_resolve() {
    let (_dir, accounts, verifier) = deployment();
    let response = serve_as(
        &registry(),
        &verifier,
        Some(&accounts),
        Request::new("GET", "/api/mcp/document").with_session("not-a-session"),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 401 Unauthorized",
        "{response}"
    );

    // The routes the device-login pairing used to own are not there for an
    // account either: signed in, they are still unknown paths. (Anonymous they
    // answer 401 before dispatch — the test below holds that half.)
    let user = account(&accounts, "signed-in", &[]);
    let session = accounts
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&user.id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        )
        .expect("a session")
        .token;
    for request in [
        Request::json("POST", op_editor_core::auth_routes::LOGIN_BEGIN, "{}")
            .with_origin(PUBLIC_ORIGIN),
        Request::new("GET", op_editor_core::auth_routes::LOGIN_STATUS),
        Request::json("POST", op_editor_core::auth_routes::AVATAR, "{}").with_origin(PUBLIC_ORIGIN),
    ] {
        let path = request.path;
        let response = serve_as(
            &registry(),
            &verifier,
            Some(&accounts),
            request.with_session(&session),
        );
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 404 Not Found",
            "{path}: {response}"
        );
    }
}

/// The other half of the same fact, and the half that used to be missing from
/// the docs rather than from the daemon (issue #123).
///
/// Online, credentials are resolved BEFORE the route table, the static layer
/// and dispatch (`serve_one_online`), so a caller with no identity is refused
/// `401` for the dead device-login family and never learns that the path is
/// gone. That order is deliberate — a `404` handed out before asking who is
/// asking would let anybody enumerate a public deployment's route table — so
/// `op_editor_core::auth_routes` says so now instead of promising a bare `404`,
/// and this is the test that holds the sentence. The `404` is real, and the
/// test above signs in to see it: it is what an identity buys.
#[test]
fn an_anonymous_caller_is_refused_the_dead_device_login_family_before_dispatch() {
    let (_dir, accounts, verifier) = deployment();
    for request in [
        Request::json("POST", op_editor_core::auth_routes::LOGIN_BEGIN, "{}")
            .with_origin(PUBLIC_ORIGIN),
        Request::new("GET", op_editor_core::auth_routes::LOGIN_STATUS),
        Request::json("POST", op_editor_core::auth_routes::LOGIN_CANCEL, "{}")
            .with_origin(PUBLIC_ORIGIN),
        Request::json("POST", op_editor_core::auth_routes::AVATAR, "{}").with_origin(PUBLIC_ORIGIN),
        // The interstitial is a GET, so it also passes the anonymous prefix's
        // static layer — which does not know this path either, and never did:
        // it is not one the daemon serves a page for.
        Request::new("GET", op_editor_core::auth_routes::LOADING_PAGE),
    ] {
        let path = request.path;
        let response = serve_as(&registry(), &verifier, Some(&accounts), request);
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 401 Unauthorized",
            "{path}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "unauthorized",
            "the verifier's own refusal, not the route table's 404: {path}: {response}"
        );
    }
}

#[test]
fn the_first_admin_from_the_environment_can_sign_in_through_the_loop() {
    let (_dir, accounts, verifier) = deployment();
    // The bootstrap the run loop performs at start-up, with the two variables
    // read for it: an operator's deployment works end to end from here.
    let outcome = crate::web_canvas_server::account_admin::ensure_first_admin(
        accounts.db(),
        Some("operator"),
        Some("a-long-deployment-password"),
        crate::accounts::now_secs(),
    );
    assert!(
        matches!(
            outcome,
            crate::web_canvas_server::account_admin::AdminBootstrap::Created { .. }
        ),
        "{outcome:?}"
    );

    let registry = registry();
    let signed_in = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json(
            "POST",
            op_editor_core::auth_routes::LOGIN,
            &login_body("operator", "a-long-deployment-password"),
        )
        .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&signed_in), "HTTP/1.1 200 OK", "{signed_in}");
    // The first admin's role came from the store, through the verifier, to the
    // identity the route tier decides with.
    let token = set_cookie(&signed_in).expect("a session cookie");
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(token),
        })
        .expect("the session resolves");
    assert!(identity.roles.rights().can_manage_users());
    // And the account's sessions can be ended — the operation `disabled` and a
    // password change depend on.
    assert_eq!(
        accounts
            .db()
            .revoke_user_sessions(&identity.user_id)
            .expect("revoke"),
        1
    );
    assert!(verifier
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(String::new()),
        })
        .is_err());
    assert_eq!(SESSION_TTL_SECS, 30 * 24 * 60 * 60);
}

/// A sign-in whose response the browser will not keep is a sign-in that did not
/// happen, so the attributes are checked through the loop rather than at the
/// builder.
#[test]
fn the_cookie_a_public_deployment_sets_is_secure_and_httponly() {
    let (_dir, accounts, verifier) = deployment();
    account(&accounts, "designer", &[]);
    let response = serve_as(
        &registry(),
        &verifier,
        Some(&accounts),
        Request::json(
            "POST",
            op_editor_core::auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        )
        .with_origin(PUBLIC_ORIGIN),
    );
    let cookie = response
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("a Set-Cookie header");
    for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    assert!(
        cookie.contains(op_editor_core::auth_routes::STATUS) == false,
        "the cookie is not scoped to a route: {cookie}"
    );
    // The response is not cacheable: it contains a credential.
    assert!(response.contains("Cache-Control: no-store"), "{response}");
}

// ---------------------------------------------------------------------------
// Which identity tier a deployment runs
// ---------------------------------------------------------------------------

/// The decision `resolve_identity_tier` makes, with the environment read for
/// it: a process-wide variable cannot be mutated in a test that shares its
/// binary with 1400 others, and the property under test is the decision.
#[test]
fn a_deployment_with_accounts_never_uses_the_development_table() {
    let (_dir, accounts, _verifier) = deployment();
    let tier = identity_tier(Ok(Some(accounts)), "tokA=userA");
    assert!(
        tier.accounts.is_some(),
        "a configured store is the tier, whatever else is set"
    );
    // The static table is not consulted at all: the token it names resolves
    // nothing, because the store is the only verifier in this tier.
    assert_eq!(
        tier.verifier
            .resolve(&PresentedCredentials {
                bearer: Some("tokA".into()),
                session_cookie: None,
            })
            .unwrap_err(),
        OnlineAuthError::UnknownCredential
    );
}

#[test]
fn the_development_table_needs_its_variable_and_says_so() {
    // Reachable by an operator who set the variable on purpose — this is how
    // the multi-tenant route tests still run without a store.
    let tier = identity_tier(Ok(None), "tokA=userA");
    assert!(tier.accounts.is_none());
    assert!(tier
        .verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokA".into()),
            session_cookie: None,
        })
        .is_ok());

    // Nothing configured at all: every credential is refused, which is a
    // deployment state an operator can see rather than a working one nobody
    // can explain.
    let empty = identity_tier(Ok(None), "");
    assert!(empty.accounts.is_none());
    assert_eq!(
        empty
            .verifier
            .resolve(&PresentedCredentials {
                bearer: Some("tokA".into()),
                session_cookie: None,
            })
            .unwrap_err(),
        OnlineAuthError::VerifierUnavailable
    );
}

#[test]
fn a_store_that_is_configured_but_unreadable_does_not_fall_back_to_the_table() {
    // The dangerous configuration: a deployment that HAS accounts, a store
    // that cannot be opened, and a development table left in the environment
    // (a copy-pasted compose file, a stale shell). Serving those tokens would
    // be serving identities out of a string while the real accounts sat
    // unreadable.
    let tier = identity_tier(
        Err(crate::accounts::AccountsError::Database("broken".into())),
        "tokA=userA",
    );
    assert!(tier.accounts.is_none());
    assert_eq!(
        tier.verifier
            .resolve(&PresentedCredentials {
                bearer: Some("tokA".into()),
                session_cookie: None,
            })
            .unwrap_err(),
        OnlineAuthError::VerifierUnavailable
    );
}

/// #82, end to end: a link an administrator hands out is a link somebody else
/// can accept, and the account it makes is an account of this deployment.
///
/// The unit tests prove what each route answers. What only this can prove is
/// that the two halves meet on the wire: the token the issuance response
/// carries is the one the acceptance route reads, and the session the
/// acceptance sets is one this deployment's verifier resolves.
#[test]
fn a_link_an_administrator_issues_through_the_loop_opens_an_account() {
    use crate::web_canvas_server::account_admin_routes::{INVITES, USERS};

    let (_dir, accounts, verifier) = deployment();
    let admin = account(&accounts, "operator", &["admin"]);
    // A session, not a sign-in: this test is about what an ALREADY signed-in
    // administrator does, and the sign-in path has its own.
    let session = accounts
        .db()
        .create_session(
            &crate::accounts::NewSession::new(&admin.id, SESSION_TTL_SECS),
            crate::accounts::now_secs(),
        )
        .expect("a session")
        .token;
    let registry = registry();

    let issued = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json(
            "POST",
            INVITES,
            &serde_json::json!({ "roles": ["qa"] }).to_string(),
        )
        .with_session(&session)
        .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&issued), "HTTP/1.1 201 Created", "{issued}");
    // The link, and no cookie: this route hands out an invitation, not a
    // sign-in.
    assert!(set_cookie(&issued).is_none(), "{issued}");
    let path = body(&issued)["path"].as_str().expect("a path").to_string();
    let token = op_editor_core::route::invite_token(&path)
        .unwrap_or_else(|| panic!("{path} is not a link this product recognises"))
        .to_string();

    // Anonymous, from nothing but the link — which is what the person who
    // received it has.
    let accepted = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::json(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": token,
                "username": "newcomer",
                "password": PASSWORD,
            })
            .to_string(),
        )
        .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&accepted), "HTTP/1.1 200 OK", "{accepted}");
    let newcomer = set_cookie(&accepted).expect("the acceptance signed the account in");

    // The roles the invitation carried are the identity the routes read, and
    // they are not the administrator's.
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(newcomer),
        })
        .expect("the session the acceptance made resolves");
    assert!(identity
        .roles
        .contains(op_editor_core::access::ProductRole::Qa));
    assert!(!identity.roles.rights().can_manage_users());
    assert_ne!(identity.user_id, admin.id);

    // And the account list the administrator reads names it.
    let listed = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        Request::new("GET", USERS).with_session(&session),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    let users = body(&listed)["users"].clone();
    let users = users.as_array().expect("a list of accounts");
    assert!(
        users
            .iter()
            .any(|user| user["username"] == "newcomer" && user["roles"][0] == "qa"),
        "{listed}"
    );
}

/// Issue #76: a session has to remember what asked for it.
///
/// A list of sessions that cannot tell one row from another makes "sign out
/// everywhere" the only control it can offer, and a session nobody recognises
/// — the one signal a person has that a credential leaked — has nothing to
/// stand out with. So the two facts the request knows about its client are
/// recorded at the only moment they exist: the `User-Agent` the client sent,
/// and the address the CONNECTION arrived from. The address comes off the
/// socket (the accept loop's `peer_addr`, presented here as `TEST_PEER`),
/// never out of a header: `X-Forwarded-For` and its family are written by
/// whoever sent the request, so a row built from one would be a guess wearing
/// an address's clothes. Behind a reverse proxy that means every row names the
/// proxy — which is the truth this daemon can actually observe.
///
/// Both halves are read back through the STORE, by the token the answer handed
/// the browser: a test that satisfied itself from the response would prove
/// nothing about the row an operator's session list would show.
#[test]
fn a_sign_in_records_the_client_and_the_address_on_the_session_row() {
    const AGENT: &str = "Norka-Test/1.0 (macOS 15; arm64)";
    let peer = TEST_PEER.expect("the fixture address").to_string();
    let (_dir, accounts, verifier) = deployment();
    let user = account(&accounts, "designer", &[]);
    let registry = registry();
    let login = || {
        Request::json(
            "POST",
            op_editor_core::auth_routes::LOGIN,
            &login_body("designer", PASSWORD),
        )
        .with_origin(PUBLIC_ORIGIN)
    };
    let session_of = |response: &str| {
        let token = set_cookie(response).expect("a session cookie");
        accounts
            .db()
            .resolve_session(&token, crate::accounts::now_secs())
            .expect("resolve")
            .expect("the session the sign-in issued")
    };

    let signed_in = serve_as(
        &registry,
        &verifier,
        Some(&accounts),
        login().with_user_agent(AGENT),
    );
    assert_eq!(status_line(&signed_in), "HTTP/1.1 200 OK", "{signed_in}");
    let recorded = session_of(&signed_in);
    assert_eq!(recorded.user_id, user.id);
    assert_eq!(
        recorded.user_agent.as_deref(),
        Some(AGENT),
        "the row must carry the client's own description of itself"
    );
    assert_eq!(
        recorded.ip.as_deref(),
        Some(peer.as_str()),
        "the row must carry the address the connection came from"
    );

    // The other half of "nothing invented": a client that sends no
    // `User-Agent` leaves that column NULL, rather than an empty string that
    // reads as a client which identified itself as nothing.
    let quiet = serve_as(&registry, &verifier, Some(&accounts), login());
    assert_eq!(status_line(&quiet), "HTTP/1.1 200 OK", "{quiet}");
    let quiet = session_of(&quiet);
    assert_eq!(quiet.user_agent, None);
    assert_eq!(
        quiet.ip.as_deref(),
        Some(peer.as_str()),
        "an absent header says nothing about the address, which is still known"
    );
}
