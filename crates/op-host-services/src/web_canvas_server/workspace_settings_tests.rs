//! The configuration gate: who may rewrite a tenant's credentials and settings.
//!
//! The decision itself is proved in `request_access_tests.rs`; these prove the
//! classification — which requests ask it, which do not — and the caller
//! shapes. That a refusal really leaves the credentials alone is asserted
//! through the online accept loop in `online_mcp_tests.rs`.

use super::*;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::request_access::DocumentAction;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::ServeMode;
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

/// Every request the table names.
const CONFIGURATION_REQUESTS: &[(&str, &str)] = &[
    ("POST", "/api/settings/credentials"),
    ("POST", "/api/mcp/server"),
];

#[test]
fn the_settings_modal_writes_ask_the_configuration_right() {
    for (method, path) in CONFIGURATION_REQUESTS {
        assert!(
            CONFIGURATION_ROUTES.contains(&(*method, *path)),
            "{method} {path} must be named"
        );
    }
}

#[test]
fn a_request_that_does_not_change_the_configuration_names_no_right() {
    // Reads of the same surface, and the document routes — which the other
    // table owns, and which must not be answered twice.
    let visitor = account("userB", &[]);
    let access = RequestAccess::online("userA", &visitor, true);
    for (method, path) in [
        ("GET", "/api/settings/credential-policy"),
        ("GET", "/api/mcp/server"),
        ("GET", "/api/ai/models"),
        ("POST", "/api/mcp/document"),
        ("POST", "/api/ai/standard"),
        ("POST", "/api/mcp/sync-reset"),
        ("POST", "/api/mcp/selection"),
        ("POST", "/api/files/abcd1234/save"),
    ] {
        assert!(
            check(method, path, &access).is_none(),
            "{method} {path} is not this table's to refuse"
        );
    }
}

#[test]
fn the_local_and_managed_operators_configure_their_own_daemon() {
    for mode in [ServeMode::Local, ServeMode::Managed] {
        let access = RequestAccess::local_operator(mode);
        for (method, path) in CONFIGURATION_REQUESTS {
            assert!(
                check(method, path, &access).is_none(),
                "{mode:?} {method} {path} must stay ungated"
            );
        }
    }
}

#[test]
fn the_owner_configures_their_own_workspace_whatever_roles_the_hub_sends() {
    let owner = account("userA", &[]);
    let access = RequestAccess::online("userA", &owner, false);
    for (method, path) in CONFIGURATION_REQUESTS {
        assert!(
            check(method, path, &access).is_none(),
            "{method} {path} must be allowed for the account itself"
        );
    }
}

#[test]
fn an_admin_configures_a_workspace_shared_with_them() {
    // The operator's decision: an admin maintains the workspace, so the
    // account list is not the only thing their role reaches.
    let admin = account("userB", &["admin"]);
    let access = RequestAccess::online("userA", &admin, true);
    for (method, path) in CONFIGURATION_REQUESTS {
        assert!(
            check(method, path, &access).is_none(),
            "{method} {path} must be allowed for an admin"
        );
    }
}

#[test]
fn an_editing_role_on_a_shared_document_is_not_a_key_to_the_workspace() {
    // The whole point: UX/UI may change a document shared with it and still
    // must not touch what that account pays for.
    for roles in [&[][..], &["ux_ui"][..], &["qa"][..], &["po"][..]] {
        let visitor = account("userB", roles);
        let access = RequestAccess::online("userA", &visitor, true);
        for (method, path) in CONFIGURATION_REQUESTS {
            let reply = check(method, path, &access)
                .unwrap_or_else(|| panic!("{roles:?} {method} {path} was not refused"));
            assert_eq!(reply.status, "403 Forbidden", "{roles:?} {method} {path}");
            assert_eq!(
                error_code(&reply),
                "read-only-role",
                "{roles:?} {method} {path}"
            );
        }
    }
    // And the same visitor is still allowed to write the document it was given
    // an editing role for: this gate narrows settings, not the document.
    let editor = account("userB", &["ux_ui"]);
    let access = RequestAccess::online("userA", &editor, true);
    assert_eq!(access.decide(DocumentAction::Edit), Ok(()));
}

#[test]
fn an_unattributable_request_is_refused_rather_than_granted() {
    // A carrier with no verified caller and no owner — `local_operator` handed
    // `Online` — has no workspace to configure. Fail closed.
    let access = RequestAccess::local_operator(ServeMode::Online);
    for (method, path) in CONFIGURATION_REQUESTS {
        let reply = check(method, path, &access)
            .unwrap_or_else(|| panic!("{method} {path} was not refused"));
        assert_eq!(reply.status, "403 Forbidden", "{method} {path}");
        assert_eq!(error_code(&reply), "tenant-not-shared", "{method} {path}");
    }
}
