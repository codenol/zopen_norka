//! The analytics routes against a real store: what an asset is, who may touch
//! it, and what deleting one leaves behind.
//!
//! The store itself is proved beside it (`analytics_store_tests`); what these
//! drive is the request — the path shape, the ownership decision, the roles the
//! matrix names, and the answer a panel reads.

use super::*;
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::{
    handle_web_canvas_request, RequestAccess, ServeMode, WebCanvasState,
};
use op_editor_core::access::RoleSet;
use op_editor_core::{EditorState, ShareLevel};

/// The daemon state of the local operator, backed by `store`.
fn local_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    state
}

/// The daemon state of an online tenant, backed by `store`.
fn tenant_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(store.clone());
    state
}

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

fn error_code(reply: &WebReply) -> String {
    body_json(reply)["error"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

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

/// Create an asset through the route and hand back its key.
fn create_asset(
    state: &mut WebCanvasState,
    access: &RequestAccess<'_>,
    name: &str,
    md: &str,
) -> String {
    let reply = handle_web_canvas_request(
        "POST",
        "/api/analytics",
        &serde_json::json!({ "name": name, "markdown": md }).to_string(),
        state,
        access,
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    body_json(&reply)["asset"]["key"]
        .as_str()
        .expect("a key")
        .to_string()
}

#[test]
fn a_local_operator_loads_reads_and_lists_an_asset() {
    let dir = TempDir::new("analytics-routes-local-1");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(
        &mut state,
        &access,
        "  Checkout analytics  ",
        "# Why\n\nPeople abandon at the total.",
    );

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(read.status, "200 OK", "{}", read.body);
    let body = body_json(&read);
    assert_eq!(
        body["asset"]["name"], "Checkout analytics",
        "a name is stored trimmed: it is read in a list"
    );
    assert_eq!(body["markdown"], "# Why\n\nPeople abandon at the total.");
    assert!(
        body["digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the digest is a fingerprint of the text as it is now: {}",
        body["digest"]
    );

    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    let assets = body_json(&listed)["assets"].clone();
    let assets = assets.as_array().expect("a list of assets");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["key"], key.as_str());
    assert_eq!(assets[0]["size"].as_u64(), Some(35));
}

#[test]
fn writing_replaces_the_text_and_moves_the_digest() {
    let dir = TempDir::new("analytics-routes-local-2");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "first");

    let before = body_json(&handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    ))["digest"]
        .clone();

    let written = handle_web_canvas_request(
        "POST",
        &format!("/api/analytics/{key}"),
        r#"{"markdown":"second"}"#,
        &mut state,
        &access,
    );
    assert_eq!(written.status, "200 OK", "{}", written.body);
    let after = body_json(&written)["digest"].clone();
    assert_ne!(before, after, "the fingerprint follows the text");

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(body_json(&read)["markdown"], "second");
}

#[test]
fn renaming_keeps_the_address() {
    // The key is how a section's link points at the asset; a rename that moved
    // it would break every section that was built from this document.
    let dir = TempDir::new("analytics-routes-local-3");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "text");

    let renamed = handle_web_canvas_request(
        "POST",
        &format!("/api/analytics/{key}/rename"),
        r#"{"name":"Checkout analytics"}"#,
        &mut state,
        &access,
    );
    assert_eq!(renamed.status, "200 OK", "{}", renamed.body);
    assert_eq!(body_json(&renamed)["asset"]["key"], key.as_str());
    assert_eq!(body_json(&renamed)["asset"]["name"], "Checkout analytics");
}

#[test]
fn deleting_removes_the_asset_from_every_answer() {
    let dir = TempDir::new("analytics-routes-local-4");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "text");

    let deleted = handle_web_canvas_request(
        "DELETE",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(deleted.status, "200 OK", "{}", deleted.body);

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(read.status, "404 Not Found", "{}", read.body);
    assert_eq!(error_code(&read), "analytics-not-found");

    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);
    assert_eq!(
        body_json(&listed)["assets"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn a_key_that_is_not_a_key_is_refused_before_a_file_is_built() {
    // The key is joined to a path, so this is the one check standing between a
    // pasted URL and the filesystem.
    let dir = TempDir::new("analytics-routes-local-5");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);

    let reply = handle_web_canvas_request(
        "GET",
        "/api/analytics/..%2Fsecrets",
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(error_code(&reply), "invalid key");
}

#[test]
fn a_caller_without_an_analytics_role_may_not_write() {
    // The matrix's `AnalyticsWrite` row is admin / UX-UI / analyst. A guest who
    // may edit the document is not automatically one of them.
    let dir = TempDir::new("analytics-routes-tenant-6");
    let store = dir.open();
    let mut state = tenant_state(&store);
    let guest = account("userB", &["frontend"]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Editor));

    let reply = handle_web_canvas_request(
        "POST",
        "/api/analytics",
        r#"{"name":"Analytics"}"#,
        &mut state,
        &access,
    );

    assert!(
        reply.status.starts_with("403"),
        "got {} {}",
        reply.status,
        reply.body
    );
}

#[test]
fn an_analyst_in_one_account_cannot_read_another_accounts_asset() {
    let dir = TempDir::new("analytics-routes-tenant-7");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let analyst = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &analyst, None);
    let key = create_asset(&mut state, &own, "Mine", "text");

    // The same role, a different account: holding the right to write analytics
    // is not holding this asset.
    let other = account("userB", &["analyst"]);
    let access = RequestAccess::online("userB", &other, None);
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(error_code(&reply), "analytics-not-yours");
}

#[test]
fn an_account_sees_only_its_own_assets() {
    let dir = TempDir::new("analytics-routes-tenant-8");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let first = account("userA", &["analyst"]);
    let access = RequestAccess::online("userA", &first, None);
    create_asset(&mut state, &access, "Mine", "text");

    let second = account("userB", &["analyst"]);
    let access = RequestAccess::online("userB", &second, None);
    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);

    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    assert_eq!(
        body_json(&listed)["assets"].as_array().map(Vec::len),
        Some(0),
        "one directory holds every account's assets; the list is what keeps them apart"
    );
}
