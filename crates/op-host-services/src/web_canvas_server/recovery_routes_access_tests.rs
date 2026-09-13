//! The recovery routes under a caller's access decision.
//!
//! The draft slot is not a document in the store, but it is still the daemon's
//! volume and still one shared slot — so who may write it, read it, restore it
//! or drop it is decided by the same function as the stored documents. See
//! `files_routes_access_tests.rs` for why these drive `handle` directly and
//! why an online deployment still refuses the family in front of this tier.
//!
//! Nothing here writes the slot: the allowed cases stop at the read of its
//! metadata, and every write is asserted as refused.

use super::*;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use op_editor_core::access::RoleSet;

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

fn tenant_state() -> WebCanvasState {
    WebCanvasState::new_for_tenant(EditorState::starter(), 3102)
}

fn error_code(reply: &WebReply) -> String {
    serde_json::from_str::<serde_json::Value>(&reply.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

#[test]
fn every_route_the_parser_produces_names_the_right_it_asks_for() {
    let cases: &[(&str, &str, DocumentAction)] = &[
        ("GET", "/api/recovery", DocumentAction::View),
        ("POST", "/api/recovery", DocumentAction::Edit),
        ("POST", "/api/recovery/restore", DocumentAction::Restore),
        ("DELETE", "/api/recovery", DocumentAction::Delete),
    ];
    for (method, path, expected) in cases {
        let route = parse_route(path).unwrap_or_else(|| panic!("unparsed {path}"));
        assert_eq!(
            required_action(method, route),
            Some(*expected),
            "{method} {path}"
        );
    }
    for (method, path) in [("PUT", "/api/recovery"), ("GET", "/api/recovery/restore")] {
        let route = parse_route(path).unwrap_or_else(|| panic!("unparsed {path}"));
        assert_eq!(required_action(method, route), None, "{method} {path}");
    }
}

#[test]
fn a_caller_with_no_role_may_ask_about_the_draft_but_not_touch_it() {
    // Reading the offer needs no role: the bar that offers recovered work is
    // drawn for every caller who may see the document.
    let visitor = account("userB", &[]);
    let access = RequestAccess::online("userA", &visitor, true);
    let read = handle("GET", "/api/recovery", "", &mut tenant_state(), &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);

    for (method, path) in [
        ("POST", "/api/recovery"),
        ("POST", "/api/recovery/restore"),
        ("DELETE", "/api/recovery"),
    ] {
        let reply = handle(method, path, "", &mut tenant_state(), &access);
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
}

#[test]
fn a_stranger_is_refused_the_draft_whole() {
    // The draft is one slot for the whole daemon: whoever reaches it reads the
    // unsaved work in it. So the document question is asked first here too.
    let stranger = account("userC", &["admin"]);
    let access = RequestAccess::online("userA", &stranger, false);
    for (method, path) in [
        ("GET", "/api/recovery"),
        ("POST", "/api/recovery"),
        ("POST", "/api/recovery/restore"),
        ("DELETE", "/api/recovery"),
    ] {
        let reply = handle(method, path, "", &mut tenant_state(), &access);
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "tenant-not-shared", "{method} {path}");
    }
}

#[test]
fn an_owner_with_an_editing_role_reaches_the_draft_routes() {
    // The autosave path for unsaved work is what this proves is not blocked:
    // the gate lets the read through to the slot.
    //
    // The two WRITES of this family are deliberately not driven here — the
    // draft slot is a real file beside the operator's documents, and a test
    // that wrote it would overwrite, and a restore would consume, whatever
    // unsaved work is actually in it. Their right is asserted by the table
    // above and by `request_access_tests`.
    let owner = account("userA", &["ux_ui"]);
    let access = RequestAccess::online("userA", &owner, false);
    let read = handle("GET", "/api/recovery", "", &mut tenant_state(), &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);
    let body: serde_json::Value = serde_json::from_str(&read.body).expect("json");
    assert_eq!(body["ok"], true);
}

#[test]
fn the_local_operator_is_unaffected_by_any_of_this() {
    let access = RequestAccess::local_operator(ServeMode::Local);
    let reply = handle(
        "GET",
        "/api/recovery",
        "",
        &mut WebCanvasState::new(EditorState::starter(), 3102),
        &access,
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    let body: serde_json::Value = serde_json::from_str(&reply.body).expect("json");
    assert_eq!(body["ok"], true);
    assert!(body["exists"].is_boolean());
}
