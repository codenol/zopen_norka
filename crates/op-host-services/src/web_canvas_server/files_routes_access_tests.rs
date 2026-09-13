//! The stored-document routes under a caller's access decision.
//!
//! The decision itself is proved in `request_access_tests.rs`; these prove the
//! wiring: that each route asks for the right right, that a refusal happens
//! BEFORE the store is touched, and that an allowed caller really does reach
//! the handler.
//!
//! ## What these do not cover any more
//!
//! This family used to be refused wholesale by the dispatcher in a shared
//! deployment, in front of this tier, because the document directory had no
//! owner dimension (#20). It does now, so the tier IS what an online request
//! reaches: the deployment-level refusal these notes used to point at is gone,
//! and `online_run_loop_tests` drives the routes through the real accept loop
//! against a directory of two accounts.
//!
//! Right and ownership are two questions ([`RequestAccess::decide`], then
//! [`RequestAccess::reaches_stored_document`]); these cover the first, and
//! `files_routes_store_tests` covers the second against a real store.
//!
//! ## Why the keys are deliberately invalid
//!
//! Several of these prove pass-through by showing the caller reached the
//! STORE: an invalid key answers `400 invalid document key`, which is a
//! different answer from the gate's `403`. That keeps every one of them off
//! the real documents directory — nothing here creates, writes or deletes a
//! file.

use super::*;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use op_editor_core::access::RoleSet;

/// A key this store would never issue, so the store answers before any I/O.
const INVALID_KEY: &str = "not-a-valid-key";

/// A verified account holding `roles`.
fn account(user_id: &str, roles: &[&str]) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: RoleSet::from_wire(roles),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

/// One online account's document authority — what a tenant's routes serve.
fn tenant_state() -> WebCanvasState {
    WebCanvasState::new_for_tenant(EditorState::starter(), 3102)
}

fn error_code(reply: &WebReply) -> String {
    serde_json::from_str::<serde_json::Value>(&reply.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

/// The owner of the document, with `roles`, asking from their own tenant.
fn as_owner(roles: &[&str]) -> ResolvedIdentity {
    account("userA", roles)
}

/// A caller on the owner's access list.
fn as_visitor(roles: &[&str]) -> ResolvedIdentity {
    account("userB", roles)
}

#[test]
fn every_route_the_parser_produces_names_the_right_it_asks_for() {
    let cases: &[(&str, &str, DocumentAction)] = &[
        ("GET", "/api/files", DocumentAction::View),
        ("POST", "/api/files", DocumentAction::Edit),
        ("GET", "/api/files/abcd1234/thumb", DocumentAction::View),
        ("POST", "/api/files/abcd1234/open", DocumentAction::View),
        ("POST", "/api/files/abcd1234/save", DocumentAction::Edit),
        ("POST", "/api/files/abcd1234/autosave", DocumentAction::Edit),
        ("POST", "/api/files/abcd1234/rename", DocumentAction::Edit),
        ("DELETE", "/api/files/abcd1234", DocumentAction::Delete),
        // The conversation about a document. Reading it is reading the
        // document; everything that writes asks for the right the five
        // contributor roles hold, which is not the right that writes the
        // document itself.
        ("GET", "/api/files/abcd1234/comments", DocumentAction::View),
        (
            "POST",
            "/api/files/abcd1234/comments",
            DocumentAction::Comment,
        ),
        (
            "POST",
            "/api/files/abcd1234/comments/7/reply",
            DocumentAction::Comment,
        ),
        (
            "POST",
            "/api/files/abcd1234/comments/7/resolve",
            DocumentAction::Comment,
        ),
        (
            "POST",
            "/api/files/abcd1234/comments/7/reopen",
            DocumentAction::Comment,
        ),
    ];
    for (method, path, expected) in cases {
        let route = parse_route(path).unwrap_or_else(|| panic!("unparsed {path}"));
        assert_eq!(
            required_action(method, &route),
            Some(*expected),
            "{method} {path}"
        );
    }
}

#[test]
fn a_combination_no_route_handles_names_no_right() {
    // Fail closed: these answer 404 before the handler's own match, so a route
    // arm added without an entry in the table is unreachable rather than
    // unchecked.
    let cases: &[(&str, &str)] = &[
        ("POST", "/api/files/abcd1234"),
        ("POST", "/api/files/abcd1234/thumb"),
        ("GET", "/api/files/abcd1234/save"),
        ("DELETE", "/api/files/abcd1234/open"),
        ("PATCH", "/api/files"),
        // A thread on its own is not a route: the list brings every thread with
        // its comments, and a shape the parser refuses cannot become a handler
        // somebody forgets to gate.
        ("GET", "/api/files/abcd1234/comments/7"),
        ("DELETE", "/api/files/abcd1234/comments"),
        ("GET", "/api/files/abcd1234/comments/7/reply"),
        ("POST", "/api/files/abcd1234/comments/7"),
        ("POST", "/api/files/abcd1234/comments/7/reply/extra"),
    ];
    // A caller who may reach the document, so the 404 can only come from the
    // route table and not from a refusal.
    let visitor = as_visitor(&[]);
    let access = RequestAccess::online("userA", &visitor, true);
    for (method, path) in cases {
        let route = parse_route(path);
        if let Some(route) = &route {
            assert_eq!(required_action(method, route), None, "{method} {path}");
        }
        assert_eq!(
            handle(method, path, "", &mut tenant_state(), &access).status,
            "404 Not Found",
            "{method} {path}"
        );
    }
}

#[test]
fn a_caller_with_no_role_may_read_the_store() {
    let visitor = as_visitor(&[]);
    // Reading a document's conversation is reading the document: whoever may
    // open the key may see what is pinned to it.
    for path in [
        format!("/api/files/{INVALID_KEY}/thumb"),
        format!("/api/files/{INVALID_KEY}/comments"),
    ] {
        let reply = handle(
            "GET",
            &path,
            "",
            &mut tenant_state(),
            &RequestAccess::online("userA", &visitor, true),
        );
        // Not the gate's 403: the request reached the store, which refused the
        // key.
        assert_eq!(reply.status, "400 Bad Request", "{path}: {}", reply.body);
        assert_eq!(error_code(&reply), "invalid document key", "{path}");
    }
}

#[test]
fn a_caller_with_no_role_is_refused_every_write() {
    let writes: &[(&str, &str, &str)] = &[
        ("POST", "/api/files", r#"{"name":"x"}"#),
        (
            "POST",
            "/api/files/not-a-valid-key/save",
            r#"{"document":{}}"#,
        ),
        ("POST", "/api/files/not-a-valid-key/autosave", ""),
        (
            "POST",
            "/api/files/not-a-valid-key/rename",
            r#"{"name":"x"}"#,
        ),
        ("DELETE", "/api/files/not-a-valid-key", ""),
        // A guest given a link to READ is refused the conversation too: they
        // hold no role, and commenting is exactly what a role is needed for.
        (
            "POST",
            "/api/files/not-a-valid-key/comments",
            r#"{"nodeId":"n1","text":"hello"}"#,
        ),
        (
            "POST",
            "/api/files/not-a-valid-key/comments/7/reply",
            r#"{"text":"hello"}"#,
        ),
        ("POST", "/api/files/not-a-valid-key/comments/7/resolve", ""),
        ("POST", "/api/files/not-a-valid-key/comments/7/reopen", ""),
    ];
    let visitor = as_visitor(&[]);
    let access = RequestAccess::online("userA", &visitor, true);
    for (method, path, body) in writes {
        let reply = handle(method, path, body, &mut tenant_state(), &access);
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
}

#[test]
fn a_contributor_may_comment_and_still_may_not_write_the_document() {
    // The split `DocumentAction::Comment` exists for, proved through the route
    // table rather than through the model: the same caller, the same key, one
    // family of routes — admitted to the conversation, refused the document.
    let contributor = as_visitor(&["analyst"]);
    let access = RequestAccess::online("userA", &contributor, true);
    let commented = handle(
        "POST",
        &format!("/api/files/{INVALID_KEY}/comments"),
        r#"{"nodeId":"n1","text":"hello"}"#,
        &mut tenant_state(),
        &access,
    );
    // Past the gate, into the store, refused there: the gate is not what
    // answered.
    assert_eq!(commented.status, "400 Bad Request", "{}", commented.body);
    assert_eq!(error_code(&commented), "invalid document key");

    for (method, path, body) in [
        (
            "POST",
            format!("/api/files/{INVALID_KEY}/save"),
            "{}".to_string(),
        ),
        (
            "POST",
            format!("/api/files/{INVALID_KEY}/rename"),
            r#"{"name":"x"}"#.to_string(),
        ),
        ("DELETE", format!("/api/files/{INVALID_KEY}"), String::new()),
    ] {
        let reply = handle(method, &path, &body, &mut tenant_state(), &access);
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
}

#[test]
fn the_refusal_comes_before_the_store_is_reached() {
    // An editor role on the wrong document: the key is never parsed, so the
    // answer is the gate's, not the store's.
    let stranger = account("userC", &["admin"]);
    let access = RequestAccess::online("userA", &stranger, false);
    let reply = handle(
        "GET",
        &format!("/api/files/{INVALID_KEY}/thumb"),
        "",
        &mut tenant_state(),
        &access,
    );
    assert_eq!(reply.status, "403 Forbidden");
    assert_eq!(error_code(&reply), "tenant-not-shared");
}

#[test]
fn a_visitor_whose_roles_grant_an_edit_reaches_the_store() {
    let visitor = as_visitor(&["ux_ui"]);
    let access = RequestAccess::online("userA", &visitor, true);
    let reply = handle(
        "POST",
        &format!("/api/files/{INVALID_KEY}/save"),
        "",
        &mut tenant_state(),
        &access,
    );
    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(error_code(&reply), "invalid document key");
}

#[test]
fn the_owner_reaches_their_own_document_without_asking_a_role() {
    // The operator's decision: a document belongs to someone who may work on
    // it, whatever roles the hub sends — otherwise a deployment whose hub sends
    // none would be read-only for the very people the documents belong to.
    // The gate lets the request through, so the answer now comes from the
    // store, about a key that does not exist.
    let owner = as_owner(&[]);
    let reached = handle(
        "DELETE",
        &format!("/api/files/{INVALID_KEY}"),
        "",
        &mut tenant_state(),
        &RequestAccess::online("userA", &owner, false),
    );
    assert_eq!(reached.status, "400 Bad Request", "{}", reached.body);

    // Someone else's document still needs an editing role.
    let visitor = as_visitor(&[]);
    let refused = handle(
        "DELETE",
        &format!("/api/files/{INVALID_KEY}"),
        "",
        &mut tenant_state(),
        &RequestAccess::online("userA", &visitor, true),
    );
    assert_eq!(refused.status, "403 Forbidden");
    assert_eq!(error_code(&refused), "read-only-role");

    // And with the designer role the visitor reaches the store as well.
    let editor = as_visitor(&["ux_ui"]);
    let allowed = handle(
        "DELETE",
        &format!("/api/files/{INVALID_KEY}"),
        "",
        &mut tenant_state(),
        &RequestAccess::online("userA", &editor, true),
    );
    assert_eq!(allowed.status, "400 Bad Request", "{}", allowed.body);
}

#[test]
fn the_local_operator_is_unaffected_by_any_of_this() {
    let state = &mut WebCanvasState::new(EditorState::starter(), 3102);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let reply = handle(
        "POST",
        &format!("/api/files/{INVALID_KEY}/autosave"),
        "",
        state,
        &access,
    );
    // The local autosave path is the one that must keep working: it reaches the
    // store, and the store's answer about the key is the only thing that stops
    // it.
    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(error_code(&reply), "invalid document key");
}
