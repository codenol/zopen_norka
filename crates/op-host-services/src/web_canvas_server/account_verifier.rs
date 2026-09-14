//! Our own identity provider: the account store decides who is asking.
//!
//! ## Where this sits
//!
//! [`IdentityVerifier`] is the online daemon's identity boundary — every
//! request is resolved to an account before anything reads a document — and
//! this is the only implementation a deployment runs. It replaced a client for
//! a hub that verified sessions somewhere else and told us who the caller was;
//! the accounts now live in this deployment's own data directory
//! ([`crate::accounts`]), so the answer is a row, not a remote call.
//!
//! ## What it decides, and what it deliberately does not
//!
//! It decides three things: which credential was presented, whether a live
//! session matches it, and whether the account behind that session may be
//! used. Everything else belongs elsewhere and stays there:
//!
//! * whether a NAME AND PASSWORD open an account —
//!   [`AccountsDb::authenticate`](crate::accounts::AccountsDb::authenticate),
//!   the one place that decision is made (see `accounts_signin`);
//! * what the account may then do — the roles travel with the identity and the
//!   route tier reads [`op_editor_core::access::Rights`] from them;
//! * what a credential is allowed to drive over MCP — `scopes`, enforced where
//!   the tool being called is known.
//!
//! ## Why a database fault is not a bad credential
//!
//! [`OnlineAuthError`] separates "this credential opens nothing" from "this
//! deployment cannot check credentials right now", and the difference is not
//! cosmetic: the first tells a browser to drop its session and sign in again,
//! the second tells an operator that the store is unreadable. A store that
//! turned a failed query into `UnknownCredential` would sign every account out
//! during a disk fault and log nothing that said why.

use std::sync::Arc;

use crate::accounts::{AccountsDb, User};
use crate::mcp_serve::tool_profile::McpScopes;
use op_editor_core::access::RoleSet;

use super::tenant_auth::{
    IdentityVerifier, IdentityVia, OnlineAuthError, PresentedCredentials, ResolvedIdentity,
    MAX_CREDENTIAL_CHARS,
};

/// Resolves a presented credential against this deployment's account store.
///
/// Holds an `Arc` because one verifier is shared by every connection thread
/// and the store is one connection behind a mutex — see the account store's
/// own docs for why that is the right shape at this size.
pub struct AccountVerifier {
    db: Arc<AccountsDb>,
}

impl AccountVerifier {
    pub fn new(db: Arc<AccountsDb>) -> Self {
        Self { db }
    }

    /// The store this verifier answers from. The sign-in and sign-out routes
    /// share it, which is why it is reachable rather than private: one store,
    /// one set of accounts, whoever is asking.
    pub fn db(&self) -> &Arc<AccountsDb> {
        &self.db
    }
}

impl IdentityVerifier for AccountVerifier {
    fn resolve(
        &self,
        presented: &PresentedCredentials,
    ) -> Result<ResolvedIdentity, OnlineAuthError> {
        let (credential, via) = presented_credential(presented)?;
        // A store fault here is the deployment being unable to answer, NOT a
        // bad credential — see the module docs.
        let session = self
            .db
            .resolve_session(credential, crate::accounts::now_secs())
            .map_err(|_| OnlineAuthError::VerifierUnavailable)?;
        // No such token, an expired session, or a session whose account was
        // deleted: one answer, because the response to all three is "this
        // browser is not signed in".
        let Some(session) = session else {
            return Err(OnlineAuthError::UnknownCredential);
        };
        let user = self
            .db
            .find_user_by_id(&session.user_id)
            .map_err(|_| OnlineAuthError::VerifierUnavailable)?
            .ok_or(OnlineAuthError::UnknownCredential)?;
        // A live session of an account that may no longer be used is refused
        // here. `disabled` means "may not sign in" wherever it is read — the
        // status owns that answer (`UserStatus::may_sign_in`) — and a session
        // that kept working after the account was disabled would make disabling
        // an account a label rather than a decision.
        if !user.status.may_sign_in() {
            return Err(OnlineAuthError::UnknownCredential);
        }
        Ok(identity_from_user(user, via))
    }
}

/// Which credential this request presented, and how it arrived.
///
/// Bearer first, cookie second: an API client that also happens to carry a
/// stale browser cookie means its token, and resolving it as the cookie's
/// account would serve the wrong tenant.
///
/// Both forms are the SAME credential in this deployment — the store issues one
/// kind of token and the browser is told to keep it in a cookie. A client that
/// cannot hold a cookie (an MCP client, a script) presents that value as a
/// bearer token, and it is accepted as what it is rather than as a second kind
/// of credential with a second set of rules. `via` still records how it
/// arrived, because that is what the cross-origin write rule is written
/// against: a browser attaches a cookie by itself, so a cookie-authenticated
/// write has to prove where it came from, while an `Authorization` header is
/// only ever attached by code that already holds the token.
fn presented_credential(
    presented: &PresentedCredentials,
) -> Result<(&str, IdentityVia), OnlineAuthError> {
    let (credential, via) = match (&presented.bearer, &presented.session_cookie) {
        (Some(token), _) => (token.as_str(), IdentityVia::ApiToken),
        (None, Some(cookie)) => (cookie.as_str(), IdentityVia::SessionCookie),
        (None, None) => return Err(OnlineAuthError::MissingCredential),
    };
    // The request parser already drops an over-long value, so this is defence
    // in depth rather than a reachable case — and it stays because "the
    // credential is longer than any credential" is a client bug worth naming.
    if credential.chars().count() > MAX_CREDENTIAL_CHARS {
        return Err(OnlineAuthError::MalformedCredential);
    }
    Ok((credential, via))
}

/// The account a session belongs to, as the rest of the daemon sees it.
///
/// One function for the two ways an identity is built — resolving a credential
/// and answering a successful sign-in — so the account a route hands back and
/// the account the next request resolves to are the same account, built the
/// same way. A second copy would be the one that forgot to parse the roles.
pub(super) fn identity_from_user(user: User, via: IdentityVia) -> ResolvedIdentity {
    // A blank display name is only ever shown, so it falls back to the name
    // rather than putting an empty label in the account button. `user_id` — the
    // tenant key — is never derived from anything else.
    let display_name = if user.display_name.trim().is_empty() {
        user.username.clone()
    } else {
        user.display_name
    };
    ResolvedIdentity {
        user_id: user.id,
        username: user.username,
        display_name,
        // The stored role strings, parsed rather than trusted and rather than
        // dropped: `RoleSet::from_wire` keeps what it does not recognise, so a
        // role this build has never heard of cannot stop an account from
        // signing in and cannot be silently discarded either.
        roles: RoleSet::from_wire(user.roles.iter().map(String::as_str)),
        via,
        // A session IS the account, so it carries the account's own authority;
        // narrowing a credential below that is what scopes are for, and this
        // deployment issues no narrower credential yet.
        scopes: McpScopes::FULL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{NewSession, NewUser, UserStatus, SESSION_TTL_SECS};
    use crate::document_test_dir::TempDir;
    use op_editor_core::access::{ProductRole, Rights};

    /// The real clock, because the verifier reads it: `resolve` asks the store
    /// whether a session is live AT the instant it is asked, so a session minted at a fixed past
    /// instant would be expired before the test ran. The tests that are about
    /// expiry pass their own instant to the store directly.
    fn now() -> i64 {
        crate::accounts::now_secs()
    }

    const PASSWORD: &str = "correct-horse-battery-staple-norka-7";

    fn store() -> (TempDir, AccountsDb) {
        let dir = TempDir::new("account-verifier");
        let db = AccountsDb::open(dir.path()).expect("open the account store");
        (dir, db)
    }

    fn verifier(db: &AccountsDb) -> AccountVerifier {
        AccountVerifier::new(Arc::new(db.clone()))
    }

    fn account(db: &AccountsDb, username: &str, roles: &[&str]) -> User {
        db.create_user(
            &NewUser {
                id: None,
                username,
                display_name: "Person",
                email: None,
                password: Some(PASSWORD),
                roles,
            },
            now(),
        )
        .expect("create an account")
    }

    fn session(db: &AccountsDb, user_id: &str) -> String {
        db.create_session(&NewSession::new(user_id, SESSION_TTL_SECS), now())
            .expect("start a session")
            .token
    }

    fn cookie(token: &str) -> PresentedCredentials {
        PresentedCredentials {
            bearer: None,
            session_cookie: Some(token.to_string()),
        }
    }

    #[test]
    fn a_session_cookie_resolves_to_its_account() {
        let (_dir, db) = store();
        let user = account(&db, "designer", &["ux_ui"]);
        let token = session(&db, &user.id);

        let identity = verifier(&db).resolve(&cookie(&token)).expect("resolves");
        assert_eq!(identity.user_id, user.id);
        assert_eq!(identity.username, "designer");
        assert_eq!(identity.display_name, "Person");
        assert_eq!(identity.via, IdentityVia::SessionCookie);
        assert_eq!(identity.scopes, McpScopes::FULL);
    }

    #[test]
    fn the_roles_in_the_store_reach_the_decision_points() {
        let (_dir, db) = store();
        let user = account(&db, "designer", &["ux_ui", "qa"]);
        let token = session(&db, &user.id);

        let identity = verifier(&db).resolve(&cookie(&token)).expect("resolves");
        assert!(identity.roles.contains(ProductRole::UxUi));
        assert!(identity.roles.contains(ProductRole::Qa));
        // Both halves of the identity are exercised downstream: the roles are
        // parsed into rights here, and the tenant key is the store's own id.
        assert_eq!(identity.roles.rights(), Rights::EDITOR);
        assert!(!identity.roles.rights().can_manage_users());
    }

    #[test]
    fn an_admin_role_in_the_store_reaches_the_account_list_right() {
        let (_dir, db) = store();
        let user = account(&db, "operator", &["admin"]);
        let token = session(&db, &user.id);

        let identity = verifier(&db).resolve(&cookie(&token)).expect("resolves");
        assert_eq!(identity.roles.rights(), Rights::ADMIN);
        assert!(identity.roles.rights().can_manage_users());
    }

    #[test]
    fn an_unknown_or_expired_session_is_not_an_account() {
        let (_dir, db) = store();
        let user = account(&db, "designer", &[]);

        // A token nobody ever issued.
        assert_eq!(
            verifier(&db).resolve(&cookie("not-a-token")).unwrap_err(),
            OnlineAuthError::UnknownCredential
        );

        // A session that existed and has run out. Its row is still there —
        // expiry is enforced on the resolve, not by deleting.
        let expired = db
            .create_session(&NewSession::new(&user.id, -1), now())
            .expect("start an already-dead session")
            .token;
        assert_eq!(
            verifier(&db).resolve(&cookie(&expired)).unwrap_err(),
            OnlineAuthError::UnknownCredential
        );

        // A session whose account was deleted: the cascade took the row.
        let revoked = session(&db, &user.id);
        db.revoke_user_sessions(&user.id).expect("revoke");
        assert_eq!(
            verifier(&db).resolve(&cookie(&revoked)).unwrap_err(),
            OnlineAuthError::UnknownCredential
        );
    }

    #[test]
    fn a_request_with_no_credential_is_its_own_answer() {
        let (_dir, db) = store();
        // Nothing about the store is consulted, which is the point: an
        // anonymous request costs the database nothing.
        assert_eq!(
            verifier(&db)
                .resolve(&PresentedCredentials::default())
                .unwrap_err(),
            OnlineAuthError::MissingCredential
        );
    }

    #[test]
    fn a_credential_too_long_to_be_one_is_malformed_rather_than_unknown() {
        // The request parser drops an over-long value outright, so the
        // verifier is handed one only by a caller that built the credentials
        // itself. It still refuses to look it up.
        let (_dir, db) = store();
        let too_long = "x".repeat(MAX_CREDENTIAL_CHARS + 1);
        assert_eq!(
            verifier(&db).resolve(&cookie(&too_long)).unwrap_err(),
            OnlineAuthError::MalformedCredential
        );
    }

    #[test]
    fn a_disabled_account_stops_being_resolvable_at_once() {
        let (_dir, db) = store();
        let user = account(&db, "designer", &["ux_ui"]);
        let token = session(&db, &user.id);
        assert!(verifier(&db).resolve(&cookie(&token)).is_ok());

        db.set_status(&user.id, UserStatus::Disabled, now())
            .expect("disable the account");

        // The session row is untouched and still live — disabling is a
        // decision about the account, and it has to reach the sessions that
        // already exist or disabling somebody would not log them out.
        assert!(db
            .resolve_session(&token, crate::accounts::now_secs())
            .expect("resolve the raw session")
            .is_some());
        assert_eq!(
            verifier(&db).resolve(&cookie(&token)).unwrap_err(),
            OnlineAuthError::UnknownCredential
        );
    }

    #[test]
    fn a_bearer_token_is_the_session_credential_carried_another_way() {
        let (_dir, db) = store();
        let user = account(&db, "designer", &[]);
        let token = session(&db, &user.id);

        let identity = verifier(&db)
            .resolve(&PresentedCredentials {
                bearer: Some(token.clone()),
                session_cookie: None,
            })
            .expect("resolves");
        assert_eq!(identity.user_id, user.id);
        // `via` is how it arrived, which is what the cross-origin write rule
        // reads — the account and its authority are the same either way.
        assert_eq!(identity.via, IdentityVia::ApiToken);
        assert_eq!(identity.scopes, McpScopes::FULL);

        // A token that opens nothing is refused the same way in both forms.
        assert_eq!(
            verifier(&db)
                .resolve(&PresentedCredentials {
                    bearer: Some("not-a-token".into()),
                    session_cookie: Some(token),
                })
                .unwrap_err(),
            OnlineAuthError::UnknownCredential
        );
    }

    #[test]
    fn an_unreadable_store_is_unavailable_rather_than_a_bad_credential() {
        let (dir, db) = store();
        let user = account(&db, "designer", &[]);
        let token = session(&db, &user.id);
        assert!(verifier(&db).resolve(&cookie(&token)).is_ok());

        // A real fault, not a mock: a second connection drops the table out
        // from under the verifier's own. SQLite notices the schema change when
        // the next statement is prepared, so the verifier's next resolve fails
        // at the database — which is the case this test is about.
        //
        // Deleting the file would not do it: the open connection keeps reading
        // from its own handle and its WAL, so nothing would fail.
        let other = rusqlite::Connection::open(dir.path().join("accounts.db"))
            .expect("a second connection to the same file");
        other
            .execute_batch("DROP TABLE sessions")
            .expect("drop the table under the verifier");

        let failure = verifier(&db).resolve(&cookie(&token)).unwrap_err();
        assert_eq!(
            failure,
            OnlineAuthError::VerifierUnavailable,
            "a store fault must not read as an invalid session"
        );
        assert_eq!(failure.http_status(), "503 Service Unavailable");
        assert_eq!(failure.code(), "verifier-unavailable");
    }

    #[test]
    fn a_deployment_with_no_data_directory_has_no_verifier() {
        // `AccountsDb::open_from_env` answers `None` for an unset or blank
        // directory rather than inventing a path — the rule itself lives in the
        // store, where it is tested. What matters here is that the online run
        // loop turns that into "every credential is refused" and never into an
        // account nobody created; that path is asserted in
        // `online_run_loop_tests`.
        assert_eq!(crate::accounts::DATA_DIR_ENV, "OPENPENCIL_ONLINE_DATA_DIR");
    }
}
