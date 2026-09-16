//! The deployment's own provider credentials, read once and offered to every
//! account — and the one direction they travel.
//!
//! ## Why a tenant needs them at all
//!
//! `--online` builds each account's editor from that account's own stored
//! document, or from [`EditorState::starter`] when there is none (see
//! `TenantRegistry::restore_editor`). Neither of those reads the process
//! settings file, and the single-document loop's start-up loader
//! (`startup_editor_for_web_canvas`) is not on this path. So on a shared
//! deployment the operator's `settings.json` — key, endpoint and model — was
//! read by nobody: `GET /api/ai/models` answered `[]` to every signed-in
//! account and every design turn failed with "no model configured" until the
//! account pasted a key of its own. Measured on the live deployment, and the
//! reason this module exists.
//!
//! So the file is read ONCE, at start-up, and the operator-owned built-in
//! agents it holds are installed into every tenant as it is created.
//!
//! ## What an account can and cannot change about the deployment's model
//!
//! **Can — bring its own key.** A browser credential is request-scoped: the
//! `credential` field of `POST /api/ai/standard` /
//! `POST /api/ai/models/discover` (`web_credentials::parse_transient_builtin`),
//! or a `POST /api/settings/credentials` snapshot merged into this account's own
//! tenant under a `web-credential:builtin:` id. Either way it lives in this
//! tenant's memory, is never written to the process settings file, and dies with
//! the tenant. That is the shipped fail-closed default and this module does not
//! weaken it.
//!
//! **Cannot — change the shared one.** `ServeMode::allows_settings_persistence`
//! is false online, so `persist_api_settings` returns before it ever calls
//! `settings_io::save_checked`: nothing an account sends reaches the deployment
//! settings file. And [`DeploymentProviders::apply_to`] runs at tenant birth
//! only, so the entries below are not editable through any tenant-scoped route:
//! the account can add its own agents beside them, not replace them. Changing
//! the shared model is the operator's file, and a restart.
//!
//! **What this costs.** The shared credential is present, in memory, in the
//! editor of every signed-in account — so a defect that exposes raw agent
//! settings (`api_key` included) to a browser exposes the deployment's key to
//! every account, not just to the operator. It is not exposed today: the model
//! catalog carries ids and display names only (`ai_proxy::models_json`) and no
//! route serialises `agent_settings`, but that is the invariant to keep, and it
//! is why the entries the browser owns are filtered out on the way in below.
//!
//! The alternative the operator asked about — publishing a shared model without
//! giving accounts write access to the process settings file — is exactly this
//! shape, and it is the one `--online` already had: the write path was closed,
//! and the read path simply did not exist. This module is the read path.

use op_editor_core::{BuiltinAgentConfig, EditorState};

/// The operator-owned built-in agents this deployment offers to every account.
#[derive(Clone, Default)]
pub(crate) struct DeploymentProviders {
    agents: Vec<BuiltinAgentConfig>,
}

impl DeploymentProviders {
    /// Read the deployment's process settings file once, for the whole daemon.
    ///
    /// A file that will not load, or one holding no enabled provider, is
    /// reported and treated as "this deployment offers no shared model":
    /// refusing to start over it would take every account offline for a mistake
    /// whose only consequence is that each of them has to bring a key.
    pub(crate) fn from_process_settings() -> Self {
        let mut state = EditorState::starter();
        if let Err(error) = crate::settings_io::load_checked(&mut state) {
            eprintln!(
                "openpencil --serve-web --online: the deployment settings file did not load, so \
                 no shared model is offered ({error})"
            );
            return Self::default();
        }
        let providers = Self::from_editor(&state);
        // stderr, like every other line this daemon prints.
        eprintln!("{}", providers.startup_line());
        providers
    }

    /// The operator-owned half of an editor's built-in agents.
    ///
    /// The browser-owned half is dropped, not shared: those entries belong to
    /// whichever account wrote them (`web_credentials::browser_owns_builtin_agent`),
    /// and one account's key must never become the deployment's.
    pub(crate) fn from_editor(state: &EditorState) -> Self {
        Self {
            agents: state
                .editor_ui
                .agent_settings
                .builtin_agents
                .iter()
                .filter(|agent| !crate::web_credentials::browser_owns_builtin_agent(agent))
                .cloned()
                .collect(),
        }
    }

    /// Install these providers into a tenant's editor, at birth.
    ///
    /// The assignment is the whole list rather than a merge, and that is safe
    /// because of where a tenant editor comes from: the account's own stored
    /// document (a `.op` file — `TenantStore` persists the document and the
    /// access list, never `agent_settings`) or [`EditorState::starter`]. There
    /// is nothing in it to merge with, and nothing here is written back.
    pub(crate) fn apply_to(&self, editor: &mut EditorState) {
        if self.agents.is_empty() {
            return;
        }
        editor.editor_ui.agent_settings.builtin_agents = self.agents.clone();
        editor.rebuild_chat_models();
    }

    /// What the daemon says about the shared model at start-up.
    ///
    /// Printed because it is the answer to the first question a signed-in
    /// account asks ("why is the model list empty?"), and because the
    /// alternative is that question being answered by a failed turn minutes
    /// later, in a different part of the log.
    fn startup_line(&self) -> String {
        let ready = self
            .agents
            .iter()
            .filter(|agent| agent.ready())
            .flat_map(|agent| agent.models.iter())
            .filter(|model| !model.trim().is_empty())
            .count();
        if ready == 0 {
            return "openpencil --serve-web --online: no shared model is offered — the deployment \
                    settings file holds no enabled built-in provider with a key and a model, so \
                    every account must bring its own"
                .to_string();
        }
        format!(
            "openpencil --serve-web --online: offering {ready} shared model(s) from {} provider(s) \
             to every signed-in account",
            self.agents.len()
        )
    }
}

#[cfg(test)]
#[path = "deployment_providers_tests.rs"]
mod tests;
