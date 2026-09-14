//! The request-level gates and the refusals that are not authorization decisions.
//!
//! Fixtures are in the parent module; `use super::*` brings them in.

use super::*;

// ---------------------------------------------------------------------------
// The gates that stand in front of every one of them
// ---------------------------------------------------------------------------

#[test]
fn an_administration_write_from_another_origin_is_refused_before_anything_else() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let hostile = HttpRequest {
        origin: Some("https://evil.example".into()),
        ..as_account(&operator, "POST", INVITES, &issue_body(&["admin"]))
    };
    let reply = handle(&auth, &hostile);
    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(auth.db().list_invites(10, 0).expect("list").len(), 0);
}

#[test]
fn an_administration_write_that_is_not_json_is_refused() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let mut plain = as_account(&operator, "POST", INVITES, &issue_body(&["admin"]));
    // `text/plain` is a CORS "simple request": a drive-by page can send it
    // without a preflight, so the content type is part of the boundary.
    plain.content_type = Some("text/plain".into());
    assert_eq!(handle(&auth, &plain).status, "415 Unsupported Media Type");
    assert_eq!(auth.db().list_invites(10, 0).expect("list").len(), 0);
}

#[test]
fn a_body_this_tier_has_no_reading_for_is_refused_rather_than_defaulted() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    for body in ["", "not json", "[]", "42", r#"{"roles":"qa"}"#] {
        let reply = handle(&auth, &as_account(&operator, "POST", INVITES, body));
        assert_eq!(reply.status, "400 Bad Request", "{body:?}");
        assert_eq!(json(&reply)["error"], "malformed-body", "{body:?}");
    }
    for (path, body) in [
        (USER_ROLES, r#"{"roles":["qa"]}"#),
        (USER_ROLES, r#"{"id":"u_x"}"#),
        (USER_STATUS, r#"{"id":"u_x"}"#),
        (USER_STATUS, r#"{"id":"u_x","status":"nonsense"}"#),
        (INVITE_REVOKE, "{}"),
    ] {
        let reply = handle(&auth, &as_account(&operator, "POST", path, body));
        assert!(
            reply.status == "400 Bad Request" || reply.status == "404 Not Found",
            "{path} {body:?} -> {} {}",
            reply.status,
            reply.body
        );
    }
}

#[test]
fn a_change_to_an_account_that_does_not_exist_is_answered_as_such() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    for (path, body) in [
        (
            USER_ROLES,
            serde_json::json!({ "id": "u_nobody", "roles": ["qa"] }).to_string(),
        ),
        (
            USER_STATUS,
            serde_json::json!({ "id": "u_nobody", "status": "disabled" }).to_string(),
        ),
    ] {
        let reply = handle(&auth, &as_account(&operator, "POST", path, &body));
        assert_eq!(reply.status, "404 Not Found", "{path}: {}", reply.body);
        assert_eq!(json(&reply)["error"], "user-not-found", "{path}");
    }
}

#[test]
fn a_store_that_cannot_be_read_is_a_fault_and_not_a_refusal() {
    let (dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let other = rusqlite::Connection::open(dir.path().join("accounts.db"))
        .expect("a second connection to the same file");
    // The invitation table alone, so that the fault lands AFTER the gate: the
    // session still resolves (the account table is untouched) and the listing
    // is what fails. Dropping `users` instead would take the sessions with it
    // through `ON DELETE CASCADE` and prove only that a signed-out caller gets
    // a 401.
    other
        .execute_batch("DROP TABLE invites;")
        .expect("drop the invitation table under the store");
    assert!(
        auth.db()
            .resolve_session(&operator.token, crate::accounts::now_secs())
            .expect("resolve")
            .is_some(),
        "the caller is still signed in; it is the store's other half that is gone"
    );

    // The listing answers with a fault, and never with "you may not": a disk
    // error read as an authorization decision would lock an administrator out
    // of their own deployment for the length of the fault.
    let reply = handle(&auth, &as_account(&operator, "GET", INVITES, ""));
    assert_eq!(reply.status, "503 Service Unavailable", "{}", reply.body);
    assert_eq!(json(&reply)["error"], "accounts-unavailable");
}

#[test]
fn every_administration_route_stays_under_the_gated_prefix() {
    // Not a convention: the sensitive-POST gate keys on the API prefix, so a
    // route outside it would lose the origin and content-type check it exists
    // for. Asserted against the shared constant rather than a literal, so a
    // moved prefix moves this test with it.
    for route in [INVITES, INVITE_REVOKE, USERS, USER_ROLES, USER_STATUS] {
        assert!(
            route.starts_with(op_editor_core::auth_routes::API_PREFIX),
            "{route} is outside {}",
            op_editor_core::auth_routes::API_PREFIX
        );
    }
}

#[test]
fn a_request_that_is_not_an_administration_route_is_left_alone() {
    let (_dir, auth) = deployment();
    for (method, path) in [
        ("GET", "/api/auth/admin/"),
        ("POST", "/api/auth/admin/invites/x"),
        // The right path with the wrong method falls through to the route
        // table, which answers 404 rather than pretending.
        ("POST", USERS),
        ("GET", INVITE_REVOKE),
        ("GET", USER_ROLES),
    ] {
        assert!(
            auth.handle(&request(method, path, "{}"), &allowed_origins())
                .is_none(),
            "{method} {path} should not be answered by the account tier"
        );
    }
}

#[test]
fn the_credential_is_held_to_its_own_scope_here_too() {
    // Every other REST route gets this from the connection tier, which this
    // tier is dispatched ahead of. A token narrowed to reading must not be able
    // to hand out invitations, and a browser session — which IS the account —
    // must not be narrowed by it at all.
    use crate::mcp_serve::tool_profile::McpScopes;
    use crate::web_canvas_server::tenant_auth::IdentityVia;

    let narrowed = credential_refusal(IdentityVia::ApiToken, McpScopes::READ_ONLY, "POST", INVITES)
        .expect("a read-only token may not write");
    assert_eq!(narrowed.status, "403 Forbidden");
    assert_eq!(json(&narrowed)["error"], "scope-insufficient");
    assert!(
        credential_refusal(IdentityVia::ApiToken, McpScopes::READ_ONLY, "GET", USERS).is_none(),
        "and reading the list is what its scope is for"
    );
    assert!(credential_refusal(IdentityVia::ApiToken, McpScopes::FULL, "POST", INVITES).is_none());
    assert!(
        credential_refusal(IdentityVia::SessionCookie, McpScopes::NONE, "POST", INVITES).is_none(),
        "a session is the account; scopes exist to narrow a token BELOW it"
    );
}
