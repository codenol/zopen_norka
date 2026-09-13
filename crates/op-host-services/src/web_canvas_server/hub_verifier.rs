//! The production [`IdentityVerifier`]: op-hub decides who everyone is.
//!
//! M1 shipped `StaticVerifier`, an env token table good enough to exercise
//! the multi-tenant plumbing. This is the real one, and it is deliberately
//! thin: all it does is choose which hub question to ask and map the answer
//! onto [`ResolvedIdentity`]. Every decision that matters — caching, status
//! mapping, fail-closed behaviour — belongs to
//! [`crate::hub_auth_client::HubAuthClient`].
//!
//! ## Which credential wins
//!
//! Bearer first, cookie second, same as `StaticVerifier`. An MCP client that
//! also carries a stale browser cookie means its token, and resolving it as
//! the cookie's account would silently serve the wrong tenant.

use crate::hub_auth_client::{HubAuthClient, HubToken, HubUser};
use crate::hub_auth_error::HubAuthError;
use crate::mcp_serve::tool_profile::McpScopes;
use op_editor_core::access::RoleSet;

use super::tenant_auth::{
    IdentityVerifier, IdentityVia, OnlineAuthError, PresentedCredentials, ResolvedIdentity,
};

/// Verifies credentials against a live op-hub.
pub struct HubVerifier {
    client: HubAuthClient,
}

impl HubVerifier {
    pub const fn new(client: HubAuthClient) -> Self {
        Self { client }
    }

    /// Build from the environment, or `None` when no hub is configured.
    pub fn from_env() -> Result<Option<Self>, HubAuthError> {
        Ok(HubAuthClient::from_env()?.map(Self::new))
    }
}

impl IdentityVerifier for HubVerifier {
    fn resolve(
        &self,
        presented: &PresentedCredentials,
    ) -> Result<ResolvedIdentity, OnlineAuthError> {
        match (&presented.bearer, &presented.session_cookie) {
            (Some(token), _) => self
                .client
                .introspect_token(token)
                .map(identity_from_token)
                .map_err(online_auth_error),
            (None, Some(cookie)) => self
                .client
                .verify_session(cookie)
                .map(identity_from_user)
                .map_err(online_auth_error),
            (None, None) => Err(OnlineAuthError::MissingCredential),
        }
    }
}

/// A browser session's account.
fn identity_from_user(user: HubUser) -> ResolvedIdentity {
    // `display_name` is optional on the hub and is only ever shown, so it
    // falls back to the username rather than being left empty. `user_id` is
    // the tenant key and is never derived from anything else.
    let display_name = user
        .display_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| user.username.clone());
    ResolvedIdentity {
        user_id: user.id,
        username: user.username,
        display_name,
        // The roles the hub just told us about, parsed rather than dropped.
        // `RoleSet::from_wire` keeps the strings it does not recognise
        // instead of failing: a role this build has never heard of must not
        // stop an account from signing in, and it must not be discarded
        // either — silently losing the hub's answer is the bug (#10) this
        // line exists to fix.
        roles: RoleSet::from_wire(&user.roles),
        via: IdentityVia::SessionCookie,
        // A browser session IS the account, so it carries the account's own
        // authority. Scopes exist to narrow an API token below that.
        scopes: McpScopes::FULL,
    }
}

/// An API token's account.
///
/// Introspection carries no display name, so the username stands in. The
/// hub's scope list is reduced to the MCP capabilities here and enforced in
/// the MCP dispatch, where the tool being called — and therefore whether it
/// writes — is known.
fn identity_from_token(token: HubToken) -> ResolvedIdentity {
    let username = if token.username.trim().is_empty() {
        token.user_id.clone()
    } else {
        token.username
    };
    let scopes = McpScopes::from_scope_list(&token.scopes);
    ResolvedIdentity {
        user_id: token.user_id,
        display_name: username.clone(),
        username,
        // Introspection reports scopes and no roles, so a token carries none.
        // Fail closed rather than invent: an account's roles are a property of
        // the person, and `POST /api/v1/tokens/introspect` does not answer
        // about the person. The consequence — an API token cannot act with
        // its owner's product roles — is a known gap in the hub's contract,
        // not something to paper over here.
        roles: RoleSet::empty(),
        via: IdentityVia::ApiToken,
        scopes,
    }
}

/// Collapse a hub failure into the answer a client is allowed to see.
///
/// An upstream failure must not read as "your credential is bad": the client
/// would drop a perfectly good session and force the user to sign in again
/// because the hub was briefly slow.
const fn online_auth_error(error: HubAuthError) -> OnlineAuthError {
    match error {
        HubAuthError::Unauthenticated => OnlineAuthError::UnknownCredential,
        HubAuthError::InvalidCredential => OnlineAuthError::MalformedCredential,
        HubAuthError::NotConfigured
        | HubAuthError::MissingInternalAuth
        | HubAuthError::Upstream
        | HubAuthError::MalformedResponse => OnlineAuthError::VerifierUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::access::{ProductRole, Rights};

    fn user() -> HubUser {
        HubUser {
            id: "user-uuid-1".into(),
            username: "person_name".into(),
            display_name: Some("Person".into()),
            avatar_url: None,
            primary_email: None,
            roles: vec!["user".into()],
        }
    }

    fn token() -> HubToken {
        HubToken {
            active: true,
            user_id: "user-uuid-1".into(),
            username: "person_name".into(),
            scopes: vec!["mcp:read".into()],
            expires_at_unix: None,
        }
    }

    #[test]
    fn a_session_maps_to_its_account_and_records_how_it_was_established() {
        let identity = identity_from_user(user());
        assert_eq!(identity.user_id, "user-uuid-1");
        assert_eq!(identity.username, "person_name");
        assert_eq!(identity.display_name, "Person");
        assert_eq!(identity.via, IdentityVia::SessionCookie);
        assert_eq!(identity.scopes, McpScopes::FULL);
    }

    #[test]
    fn a_token_scope_list_narrows_what_it_may_drive() {
        let read_only = identity_from_token(HubToken {
            scopes: vec!["mcp:read".into()],
            ..token()
        });
        assert!(!read_only.scopes.can_write());

        let both = identity_from_token(HubToken {
            scopes: vec!["mcp:read".into(), "mcp:write".into()],
            ..token()
        });
        assert!(both.scopes.can_write());

        // A token that names no mcp scope may do nothing — fail-closed. See
        // `McpScopes::from_scope_list`: op-hub must issue explicit scopes.
        let unscoped = identity_from_token(HubToken {
            scopes: vec!["billing:read".into()],
            ..token()
        });
        assert!(!unscoped.scopes.can_write());
        assert!(!unscoped.scopes.can_read());
    }

    #[test]
    fn a_session_carries_the_accounts_roles_to_the_decision_point() {
        let identity = identity_from_user(HubUser {
            roles: vec!["ux_ui".into(), "qa".into()],
            ..user()
        });
        assert!(identity.roles.contains(ProductRole::UxUi));
        assert!(identity.roles.contains(ProductRole::Qa));
        // UX/UI with QA is the union case from the operator's matrix.
        assert_eq!(identity.roles.rights(), Rights::EDITOR);
        assert!(identity.roles.rights().can_edit());
        assert!(!identity.roles.rights().can_manage_users());
    }

    #[test]
    fn an_admin_role_reaches_the_decision_point_too() {
        let identity = identity_from_user(HubUser {
            roles: vec!["Админ".into()],
            ..user()
        });
        assert_eq!(identity.roles.rights(), Rights::ADMIN);
        assert!(identity.roles.rights().can_manage_users());
    }

    #[test]
    fn the_hubs_own_role_word_is_not_mistaken_for_a_product_role() {
        // `GET /api/v1/session` ships `roles: ["user"]` — a hub-level role,
        // not one of the seven. It must not block the sign-in, must not grant
        // an edit, and must stay visible to whoever debugs the mismatch
        // rather than being quietly discarded, which is what #10 is about.
        let identity = identity_from_user(user());
        assert!(identity.roles.is_empty());
        assert_eq!(identity.roles.unrecognized(), &["user".to_string()]);
        assert!(!identity.roles.rights().can_edit());
        assert!(identity.roles.rights().can_view());
    }

    #[test]
    fn an_api_token_carries_no_product_roles() {
        // Introspection answers about the token, not about the person, so it
        // reports no roles. Fail closed: the token's authority stays its
        // scope list, and no product role is invented for it.
        let identity = identity_from_token(token());
        assert!(identity.roles.is_empty());
        assert_eq!(identity.roles.rights(), Rights::VIEW_ONLY);
        assert!(!identity.roles.rights().can_comment());
    }

    #[test]
    fn a_blank_display_name_falls_back_to_the_username() {
        for blank in [None, Some(String::new()), Some("   ".into())] {
            let identity = identity_from_user(HubUser {
                display_name: blank.clone(),
                ..user()
            });
            assert_eq!(identity.display_name, "person_name", "{blank:?}");
        }
    }

    #[test]
    fn a_token_maps_to_its_account() {
        let identity = identity_from_token(token());
        assert_eq!(identity.user_id, "user-uuid-1");
        assert_eq!(identity.username, "person_name");
        assert_eq!(identity.via, IdentityVia::ApiToken);
    }

    #[test]
    fn a_token_without_a_username_still_yields_a_usable_identity() {
        let identity = identity_from_token(HubToken {
            username: String::new(),
            ..token()
        });
        assert_eq!(identity.user_id, "user-uuid-1");
        assert_eq!(identity.username, "user-uuid-1");
    }

    #[test]
    fn only_a_definitive_hub_negative_reads_as_a_bad_credential() {
        assert_eq!(
            online_auth_error(HubAuthError::Unauthenticated),
            OnlineAuthError::UnknownCredential
        );
        assert_eq!(
            online_auth_error(HubAuthError::InvalidCredential),
            OnlineAuthError::MalformedCredential
        );
        // A hub outage must never tell a browser its session is invalid, or
        // every signed-in user is signed out by a blip.
        for upstream in [
            HubAuthError::Upstream,
            HubAuthError::MalformedResponse,
            HubAuthError::NotConfigured,
            HubAuthError::MissingInternalAuth,
        ] {
            assert_eq!(
                online_auth_error(upstream),
                OnlineAuthError::VerifierUnavailable,
                "{upstream:?}"
            );
            assert_eq!(
                online_auth_error(upstream).http_status(),
                "503 Service Unavailable"
            );
        }
    }

    #[test]
    fn a_request_with_no_credential_never_reaches_the_hub() {
        // No client is needed to answer this, which is the point: an
        // anonymous request costs the hub nothing.
        let verifier = HubVerifier::new(
            HubAuthClient::new("http://127.0.0.1:1", Some("s".into())).expect("builds"),
        );
        assert_eq!(
            verifier
                .resolve(&PresentedCredentials::default())
                .unwrap_err(),
            OnlineAuthError::MissingCredential
        );
    }
}
