//! The document-write table, and the gate it feeds.
//!
//! The decision itself is proved in `request_access_tests.rs`; these prove the
//! classification: which requests ask something of the document, which ask
//! nothing, and what the gate answers for each caller shape. The route-level
//! half — that a refused write reaches the document not at all — is asserted
//! through the online accept loop in `online_mcp_tests.rs`, where the real
//! request path exists.

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

fn error_code(reply: &WebReply) -> String {
    serde_json::from_str::<serde_json::Value>(&reply.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

/// A `tools/call` message for `tool`.
fn call(tool: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{{}}}}}}"#
    )
}

/// Every request the table names as a document write, with a body it is
/// classified from.
const WRITE_REQUESTS: &[(&str, &str, &str)] = &[
    ("POST", "/api/mcp/document", r#"{"document":{}}"#),
    ("POST", "/api/mcp/sync-reset", ""),
    ("POST", "/api/mcp/selection", r#"{"selectedIds":[]}"#),
    ("POST", "/api/ai/standard", "{}"),
    ("POST", "/api/collab/action", r#"{"type":"requestUndo"}"#),
    ("POST", "/api/file/new", ""),
    ("POST", "/api/file/save", ""),
    ("POST", "/api/file/open-recent", r#"{"path":"/tmp/x.op"}"#),
    (
        "POST",
        "/mcp",
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"add_page","arguments":{}}}"#,
    ),
];

#[test]
fn every_route_that_changes_the_document_names_the_edit_right() {
    let cases: &[(&str, &str, &str)] = &[
        ("POST", "/api/mcp/document", r#"{"document":{}}"#),
        ("POST", "/api/mcp/sync-reset", ""),
        ("POST", "/api/mcp/selection", r#"{"selectedIds":[]}"#),
        ("POST", "/api/ai/standard", "{}"),
        ("POST", "/api/collab/action", r#"{"type":"requestUndo"}"#),
        ("POST", "/api/file/new", ""),
        ("POST", "/api/file/save", ""),
        ("POST", "/api/file/open-recent", r#"{"path":"/tmp/x.op"}"#),
    ];
    for (method, path, body) in cases {
        assert_eq!(
            required_action(method, path, body, ServeMode::Local),
            Some(DocumentAction::Edit),
            "{method} {path}"
        );
    }
}

#[test]
fn every_collaboration_action_asks_the_edit_right_not_only_undo() {
    // Undo applies a command to this document here and now; the rest feed the
    // session that carries the peers' commands, and two of them admit a peer to
    // it. Asking per action would make the answer a property of a list that
    // grows — so the route is asked whole, unknown actions included.
    for action in [
        r#"{"type":"requestUndo"}"#,
        r#"{"type":"openCreate"}"#,
        r#"{"type":"start"}"#,
        r#"{"type":"leave"}"#,
        r#"{"type":"approveAdmissionEditor","requestKey":"k"}"#,
        r#"{"type":"approveAdmissionViewer","requestKey":"k"}"#,
        r#"{"type":"not-an-action"}"#,
    ] {
        assert_eq!(
            required_action("POST", "/api/collab/action", action, ServeMode::Local),
            Some(DocumentAction::Edit),
            "{action}"
        );
    }
}

#[test]
fn a_request_that_asks_nothing_of_the_document_names_no_right() {
    // Reads, settings, auth, sharing, export and the two tiers that own finer
    // tables of their own (`files_routes`, `recovery_routes`: opening a stored
    // document is a View while saving it is an Edit, on the same path).
    let cases: &[(&str, &str, &str)] = &[
        ("GET", "/api/mcp/document", ""),
        ("GET", "/api/mcp/version", ""),
        ("GET", "/api/mcp/selection", ""),
        ("GET", "/api/mcp/indicators", ""),
        ("GET", "/api/mcp/server", ""),
        ("GET", "/api/auth/status", ""),
        ("GET", "/api/ai/models", ""),
        ("POST", "/api/ai/stream", "{}"),
        ("POST", "/api/export/pdf", "{}"),
        ("POST", "/api/export/raster", "{}"),
        ("POST", "/api/share/grant", r#"{"userId":"userB"}"#),
        ("GET", "/api/collab/state", ""),
        // A cursor is not a command — see the table's own note on this route.
        (
            "POST",
            "/api/collab/presence",
            r#"{"cursor":{"x":1,"y":2}}"#,
        ),
        // The account's own configuration is the other table's question
        // (`workspace_settings`); named here only to prove this one leaves it
        // alone, and that the two cannot both answer.
        ("POST", "/api/mcp/server", r#"{"port":3102}"#),
        ("POST", "/api/settings/credentials", "{}"),
        // Purposely not this table's either: each owns a per-route table that
        // is finer than (method, path). A second answer here would be a second
        // policy.
        ("GET", "/api/files", ""),
        ("POST", "/api/files/abcd1234/save", ""),
        ("POST", "/api/files/abcd1234/open", ""),
        ("DELETE", "/api/files/abcd1234", ""),
        ("POST", "/api/recovery", ""),
        ("POST", "/api/recovery/restore", ""),
    ];
    for (method, path, body) in cases {
        assert_eq!(
            required_action(method, path, body, ServeMode::Local),
            None,
            "{method} {path}"
        );
    }
}

#[test]
fn the_json_rpc_tier_is_classified_from_its_message_not_its_path() {
    let mode = ServeMode::Local;
    // The catalog's own classification decides, in both spellings a client uses.
    for message in [call("add_page"), call("batch_design")] {
        assert_eq!(
            required_action("POST", "/mcp", &message, mode),
            Some(DocumentAction::Edit),
            "{message}"
        );
    }
    assert_eq!(
        required_action("POST", "/mcp", r#"{"id":1,"method":"add_page"}"#, mode),
        Some(DocumentAction::Edit),
        "the legacy direct-method spelling writes too"
    );
    for message in [call("get_node"), call("get_document_info")] {
        assert_eq!(
            required_action("POST", "/mcp", &message, mode),
            None,
            "{message}"
        );
    }
    // Fail closed: a method the catalog does not classify is treated as a
    // mutation, exactly as `tool_profile::access_of` treats it.
    assert_eq!(
        required_action(
            "POST",
            "/mcp",
            r#"{"id":1,"method":"resources/read"}"#,
            mode
        ),
        Some(DocumentAction::Edit)
    );
    // Nothing a message could be is a write if it is not a message.
    for body in ["", "   ", "not json", "[]"] {
        assert_eq!(required_action("POST", "/mcp", body, mode), None, "{body}");
    }
}

#[test]
fn the_root_alias_is_a_tool_tier_only_where_the_deployment_keeps_it() {
    let write = call("add_page");
    assert_eq!(
        required_action("POST", "/", &write, ServeMode::Local),
        Some(DocumentAction::Edit)
    );
    assert_eq!(
        required_action("POST", "/", &write, ServeMode::Managed),
        Some(DocumentAction::Edit)
    );
    // Online answers that path with 405 and never dispatches it as a tool
    // call, so classifying it as one would replace a 405 with a 403.
    assert_eq!(
        required_action("POST", "/", &write, ServeMode::Online),
        None
    );
}

#[test]
fn handshake_methods_never_write() {
    // The predicate's handshake list has to stay in step with the
    // short-circuit in `process_message_with_applier_profiled`, or a read-only
    // caller loses `tools/list` (over-refused) or an unlisted mutating method
    // slips through (under-refused). So each one is driven through the real
    // dispatch: it must apply no command AND must not be called a write.
    let messages = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","method":"initialized"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
    ];
    for message in messages {
        let mut applied = false;
        let mut state = op_editor_core::EditorState::new();
        crate::mcp_serve::process_message_with_applier(&mut state, message, |_, _, _| {
            applied = true;
            true
        })
        .expect("a handshake method is answered, not failed");
        assert!(
            !applied,
            "{message} reached a tool, so it can no longer be called a non-write"
        );
        assert!(
            !crate::mcp_serve::message_writes_document(message),
            "{message} must not be admitted as a write"
        );
    }
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
fn the_local_and_managed_operators_are_never_refused() {
    for mode in [ServeMode::Local, ServeMode::Managed] {
        let access = RequestAccess::local_operator(mode);
        for (method, path, body) in WRITE_REQUESTS {
            assert!(
                check(method, path, body, &access).is_none(),
                "{mode:?} {method} {path} must stay ungated"
            );
        }
    }
}

#[test]
fn a_visitor_with_no_editing_role_is_refused_every_write() {
    let visitor = as_visitor(&[]);
    let access = RequestAccess::online("userA", &visitor, true);
    for (method, path, body) in WRITE_REQUESTS {
        let reply = check(method, path, body, &access)
            .unwrap_or_else(|| panic!("{method} {path} was not refused"));
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
    // The same answer for a contributor: every non-design product role reads.
    let contributor = as_visitor(&["qa"]);
    let access = RequestAccess::online("userA", &contributor, true);
    assert_eq!(
        check("POST", "/api/mcp/document", "", &access).map(|reply| error_code(&reply)),
        Some("read-only-role".to_string())
    );
}

#[test]
fn a_visitor_whose_roles_grant_an_edit_passes() {
    let editor = as_visitor(&["ux_ui"]);
    let access = RequestAccess::online("userA", &editor, true);
    for (method, path, body) in WRITE_REQUESTS {
        assert!(
            check(method, path, body, &access).is_none(),
            "{method} {path} must be allowed for an editor"
        );
    }
}

#[test]
fn the_owner_passes_without_asking_a_role() {
    // The operator's decision: a document belongs to someone who may work on
    // it, whatever roles the hub sends.
    let owner = as_owner(&[]);
    let access = RequestAccess::online("userA", &owner, false);
    for (method, path, body) in WRITE_REQUESTS {
        assert!(
            check(method, path, body, &access).is_none(),
            "{method} {path} must be allowed for the document's owner"
        );
    }
}

#[test]
fn a_stranger_is_refused_the_document_before_the_action_is_read() {
    // An editing role on a document that is not theirs: the answer is about
    // the document, not about the roles, and it is the same code the tenant
    // lease answers with.
    let stranger = account("userC", &["admin"]);
    let access = RequestAccess::online("userA", &stranger, false);
    for (method, path, body) in WRITE_REQUESTS {
        let reply = check(method, path, body, &access)
            .unwrap_or_else(|| panic!("{method} {path} was not refused"));
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "tenant-not-shared", "{method} {path}");
    }
}

#[test]
fn a_route_that_asks_nothing_is_never_refused_whatever_the_roles() {
    // The gate must not become a second policy for the routes it does not own:
    // a role-less visitor keeps every read, the settings modal and the routes
    // whose own tables already decide.
    let visitor = as_visitor(&[]);
    let access = RequestAccess::online("userA", &visitor, true);
    for (method, path, body) in [
        ("GET", "/api/mcp/document", ""),
        ("GET", "/api/mcp/selection", ""),
        ("POST", "/api/mcp/server", r#"{"port":3102}"#),
        ("POST", "/api/ai/stream", "{}"),
        ("POST", "/api/files/abcd1234/open", ""),
        ("POST", "/api/recovery/restore", ""),
    ] {
        assert!(
            check(method, path, body, &access).is_none(),
            "{method} {path} is not this table's to refuse"
        );
    }
}
