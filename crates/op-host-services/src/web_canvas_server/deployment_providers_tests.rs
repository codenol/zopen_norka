//! Tests for the deployment's shared providers and what an account may do to
//! them.
//!
//! The first test is the regression this module exists for: before it, an
//! online tenant was born from `EditorState::starter()` alone, so the catalog
//! an account saw was empty however correct the deployment's settings file was.

use super::*;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::{TenantLease, TenantLimits, TenantRegistry};

fn identity(user_id: &str) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: op_editor_core::access::RoleSet::empty(),
        via: IdentityVia::ApiToken,
        scopes: crate::mcp_serve::tool_profile::McpScopes::FULL,
    }
}

fn builtin(id: &str, key: &str) -> BuiltinAgentConfig {
    BuiltinAgentConfig {
        id: id.into(),
        preset: op_editor_core::BuiltinAgentPresetKey::DeepSeek,
        display_name: "DeepSeek".into(),
        kind: op_editor_core::BuiltinAgentKind::OpenAiCompat,
        api_key: key.into(),
        models: vec!["deepseek-v4-flash-vision-exp".into()],
        base_url: "https://api.deepseek.com/v1".into(),
        enabled: true,
    }
}

/// The deployment's settings file as the daemon reads it: an editor carrying
/// the operator's agent.
fn deployment_editor() -> EditorState {
    let mut state = EditorState::starter();
    state.editor_ui.agent_settings.builtin_agents = vec![builtin("deployment-1", "sk-deploy")];
    state
}

fn registry_with(providers: DeploymentProviders) -> TenantRegistry {
    // No `OPENPENCIL_ONLINE_DATA_DIR` in a test process: the store is disabled
    // and every tenant starts from `EditorState::starter()`.
    TenantRegistry::new(3100, TenantLimits::default(), Vec::new())
        .with_deployment_providers(providers)
}

fn catalog(lease: &TenantLease) -> String {
    let guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    crate::ai_proxy::models_json(&guard.editor)
}

#[test]
fn a_signed_in_account_catalogs_the_deployments_shared_model() {
    let providers = DeploymentProviders::from_editor(&deployment_editor());
    let registry = registry_with(providers);

    let lease = registry.lease_for(&identity("userA")).expect("lease");
    let catalog = catalog(&lease);

    assert!(
        catalog.contains("builtin:deployment-1:deepseek-v4-flash-vision-exp"),
        "the account's model catalog must list the deployment's own model: {catalog}"
    );
}

#[test]
fn every_account_gets_the_shared_model_and_its_own_editor() {
    let providers = DeploymentProviders::from_editor(&deployment_editor());
    let registry = registry_with(providers);

    let a = registry.lease_for(&identity("userA")).expect("lease A");
    let b = registry.lease_for(&identity("userB")).expect("lease B");

    assert!(catalog(&a).contains("deployment-1"));
    assert!(catalog(&b).contains("deployment-1"));
}

/// A key an account brought with it must never become the deployment's: were a
/// browser-owned entry shared, the next account to sign in would be spending
/// somebody else's credential.
#[test]
fn browser_owned_credentials_are_never_shared() {
    let mut state = deployment_editor();
    state
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(builtin("web-credential:builtin:7", "sk-account-owned"));

    let providers = DeploymentProviders::from_editor(&state);
    let registry = registry_with(providers);
    let lease = registry.lease_for(&identity("userA")).expect("lease");
    let catalog = catalog(&lease);

    assert!(catalog.contains("deployment-1"), "{catalog}");
    assert!(
        !catalog.contains("sk-account-owned") && !catalog.contains("builtin:7"),
        "an account's own credential must not be offered to anybody else: {catalog}"
    );
}

/// The shared entries survive an account writing credentials of its own, and the
/// key that account brought still resolves a provider — the whole of the "both"
/// contract: one shared model for everybody, and a person's own key beside it.
#[test]
fn an_accounts_own_credentials_do_not_displace_the_shared_model() {
    let providers = DeploymentProviders::from_editor(&deployment_editor());
    let registry = registry_with(providers);
    let lease = registry.lease_for(&identity("userA")).expect("lease");

    let payload = serde_json::json!({
        "version": 2,
        "builtin_agents": [{
            "id": "mine",
            "preset": "openai",
            "display_name": "Mine",
            "kind": "openai-compat",
            "api_key": "sk-mine",
            "model": "gpt-test",
            "base_url": "https://api.openai.com/v1",
            "enabled": true,
        }],
    })
    .to_string();
    let guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    let mut editor = guard.editor.clone();
    drop(guard);
    crate::web_credentials::apply_json(&mut editor, &payload)
        .expect("the account's own snapshot merges");

    let catalog = crate::ai_proxy::models_json(&editor);
    assert!(
        catalog.contains("builtin:deployment-1:deepseek-v4-flash-vision-exp"),
        "the shared model must survive the account's own credentials: {catalog}"
    );
    assert_eq!(
        editor
            .editor_ui
            .agent_settings
            .builtin_agents
            .iter()
            .filter(|agent| agent.id == "deployment-1")
            .count(),
        1,
        "the account's snapshot must not duplicate or replace the shared provider"
    );
}

/// A key a person brings for one turn rides the request body, not the
/// deployment: `credential` on `POST /api/ai/standard` becomes the turn's own
/// provider, ahead of anything the deployment holds.
#[test]
fn a_request_scoped_credential_still_resolves_beside_the_shared_model() {
    let providers = DeploymentProviders::from_editor(&deployment_editor());
    let registry = registry_with(providers);
    let lease = registry.lease_for(&identity("userA")).expect("lease");

    let body = serde_json::json!({
        "model": "gpt-test",
        "user": "draw a login screen",
        "credential": {
            "id": "mine",
            "preset": "openai",
            "display_name": "Mine",
            "kind": "openai-compat",
            "api_key": "sk-mine",
            "model": "gpt-test",
            "base_url": "https://api.openai.com/v1",
            "enabled": true,
        },
    })
    .to_string();
    let request = crate::ai_proxy::parse_ai_stream_body(&body).expect("request parses");
    assert!(
        request.transient_builtin.is_some(),
        "the browser's own credential must survive body parsing"
    );

    let guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    let provider = crate::ai_proxy::proxy_provider_for_request(
        &guard.editor,
        &request,
        guard.credential_persistence,
    )
    .expect("the request-scoped credential is accepted");
    assert!(
        provider.is_some(),
        "a person's own key must resolve a provider on the shared deployment"
    );
    drop(guard);

    // And the shared model is a separate, still-usable choice.
    let catalog = catalog(&lease);
    assert!(
        catalog.contains("builtin:deployment-1:deepseek-v4-flash-vision-exp"),
        "{catalog}"
    );
}

#[test]
fn a_deployment_without_providers_leaves_the_tenant_as_it_was() {
    let registry = registry_with(DeploymentProviders::default());
    let lease = registry.lease_for(&identity("userA")).expect("lease");

    assert_eq!(catalog(&lease), "[]");
}

#[test]
fn the_startup_line_reports_the_shared_model_or_says_there_is_none() {
    let with_model = DeploymentProviders::from_editor(&deployment_editor()).startup_line();
    assert!(with_model.contains("1 shared model(s)"), "{with_model}");

    let without = DeploymentProviders::default().startup_line();
    assert!(without.contains("no shared model is offered"), "{without}");
}

#[test]
fn the_deployments_model_roles_reach_its_tenants() {
    // Issues #249/#250, measured live: a tenant received BOTH shared models and
    // still resolved the checker to the builder, because `apply_to` copied the
    // agents and dropped the roles. The roles are deployment configuration and
    // have to ride with the models they name.
    let mut editor = deployment_editor();
    editor.editor_ui.agent_settings.builder_model =
        Some("builtin:deployment-1:deepseek-v4-flash".into());
    editor.editor_ui.agent_settings.verifier_model =
        Some("builtin:deployment-1:deepseek-v4-flash-vision-exp".into());
    let providers = DeploymentProviders::from_editor(&editor);
    let registry = registry_with(providers);

    let lease = registry.lease_for(&identity("userA")).expect("lease");
    let guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        guard
            .editor
            .editor_ui
            .agent_settings
            .verifier_model
            .as_deref(),
        Some("builtin:deployment-1:deepseek-v4-flash-vision-exp"),
        "the checker role must reach the tenant"
    );
    assert_eq!(
        guard
            .editor
            .editor_ui
            .agent_settings
            .builder_model
            .as_deref(),
        Some("builtin:deployment-1:deepseek-v4-flash"),
        "and so must the builder role"
    );
}
