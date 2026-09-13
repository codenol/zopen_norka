//! The recovery routes under a caller's access decision.
//!
//! The draft slot is not a document in the store, but it is still the daemon's
//! volume — so who may write it, read it, restore it or drop it is decided by
//! the same function as the stored documents. See `files_routes_access_tests.rs`
//! for why these drive `handle` directly, and `online_run_loop_tests` for the
//! same routes through the real accept loop.
//!
//! ## Why every state here owns a store
//!
//! The draft lives in the documents directory, which these routes now take from
//! the state's own store rather than resolving the environment a second time.
//! That is what keeps a test off the operator's real `~/.norka/files`: the
//! directory is the one `TempDir` handed the state, and nothing here reads or
//! writes the machine's own documents.

use super::*;
use crate::document_test_dir::TempDir;
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

/// One online account's document authority, backed by `dir`'s store.
fn tenant_state(dir: &TempDir) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(dir.open());
    state
}

/// The same for the local operator's daemon.
fn local_state(dir: &TempDir) -> WebCanvasState {
    let mut state = WebCanvasState::new(EditorState::starter(), 3102);
    state.documents = Some(dir.open());
    state
}

/// A whole-document autosave body, in the shape `POST /api/recovery` takes.
const DRAFT_BODY: &str = r##"{"document":{"version":"1.0.0","children":[{"id":"n1","type":"rectangle","name":"Unsaved","x":0,"y":0,"width":10,"height":10}]}}"##;

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
    let dir = TempDir::new("recovery-access-roles");
    let visitor = account("userB", &[]);
    let access = RequestAccess::online("userA", &visitor, true);
    let read = handle("GET", "/api/recovery", "", &mut tenant_state(&dir), &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);

    for (method, path) in [
        ("POST", "/api/recovery"),
        ("POST", "/api/recovery/restore"),
        ("DELETE", "/api/recovery"),
    ] {
        let reply = handle(method, path, "", &mut tenant_state(&dir), &access);
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
}

#[test]
fn a_stranger_is_refused_the_draft_whole() {
    // The draft holds a whole document's unsaved work: whoever reaches it reads
    // it. So the document question is asked first here too — and the refusal
    // lands before the store is even opened, which is why this state has none.
    let stranger = account("userC", &["admin"]);
    let access = RequestAccess::online("userA", &stranger, false);
    for (method, path) in [
        ("GET", "/api/recovery"),
        ("POST", "/api/recovery"),
        ("POST", "/api/recovery/restore"),
        ("DELETE", "/api/recovery"),
    ] {
        let reply = handle(
            method,
            path,
            "",
            &mut WebCanvasState::new_for_tenant(EditorState::starter(), 3102),
            &access,
        );
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "tenant-not-shared", "{method} {path}");
    }
}

#[test]
fn an_owner_with_an_editing_role_reaches_the_draft_routes() {
    // The autosave path for unsaved work is what this proves is not blocked.
    let dir = TempDir::new("recovery-access-owner");
    let owner = account("userA", &["ux_ui"]);
    let access = RequestAccess::online("userA", &owner, false);
    let read = handle("GET", "/api/recovery", "", &mut tenant_state(&dir), &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);
    let body: serde_json::Value = serde_json::from_str(&read.body).expect("json");
    assert_eq!(body["ok"], true);
    assert_eq!(body["exists"], false, "nothing has been written yet");
}

#[test]
fn two_accounts_never_see_each_others_draft() {
    // The property the per-owner slot exists for, and the one a shared slot
    // could not have: an account's unsaved work is invisible — and
    // unrestorable — to every other account, whatever their roles.
    let dir = TempDir::new("recovery-access-isolation");
    let a = account("userA", &["ux_ui"]);
    let b = account("userB", &["ux_ui"]);
    let access_a = RequestAccess::online("userA", &a, false);
    let access_b = RequestAccess::online("userB", &b, false);

    let written = handle(
        "POST",
        "/api/recovery",
        DRAFT_BODY,
        &mut tenant_state(&dir),
        &access_a,
    );
    assert_eq!(written.status, "200 OK", "{}", written.body);

    let mine = handle(
        "GET",
        "/api/recovery",
        "",
        &mut tenant_state(&dir),
        &access_a,
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&mine.body).expect("json")["exists"],
        true,
        "the writer is offered its own draft back"
    );

    let theirs = handle(
        "GET",
        "/api/recovery",
        "",
        &mut tenant_state(&dir),
        &access_b,
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&theirs.body).expect("json")["exists"],
        false,
        "and no other account is told it exists"
    );
    let restore = handle(
        "POST",
        "/api/recovery/restore",
        "",
        &mut tenant_state(&dir),
        &access_b,
    );
    assert_eq!(
        restore.status, "404 Not Found",
        "there is nothing of theirs to restore: {}",
        restore.body
    );

    // One file per account, and the operator's slot is neither of them.
    let drafts: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("recovery.") && name.ends_with(".op"))
        .collect();
    assert_eq!(drafts.len(), 1, "one account wrote one draft: {drafts:?}");
    assert!(
        !dir.join("recovery.op").exists(),
        "an account's draft is not the operator's slot"
    );
}

#[test]
fn the_local_operator_is_unaffected_by_any_of_this() {
    let dir = TempDir::new("recovery-access-local");
    let access = RequestAccess::local_operator(ServeMode::Local);
    let reply = handle("GET", "/api/recovery", "", &mut local_state(&dir), &access);
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    let body: serde_json::Value = serde_json::from_str(&reply.body).expect("json");
    assert_eq!(body["ok"], true);
    assert!(body["exists"].is_boolean());

    // And the operator's draft keeps the name it has always had, so an upgrade
    // finds the work that was already sitting there.
    let written = handle(
        "POST",
        "/api/recovery",
        DRAFT_BODY,
        &mut local_state(&dir),
        &access,
    );
    assert_eq!(written.status, "200 OK", "{}", written.body);
    assert!(dir.join("recovery.op").is_file());
}
