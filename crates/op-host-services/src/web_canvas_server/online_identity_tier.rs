//! Which identities this deployment trusts, and the answers that carry them.
//!
//! Split out of `online_run_loop` at the 800-line cap: the accept loop is a
//! bind/thread/dispatch spine, and everything here is one decision made once
//! at start-up plus the two renderings of an account answer.
//!
//! [`IdentityTier`] is that decision — the verifier every credential is
//! resolved against, and, when this deployment has accounts, the routes that
//! start and end sessions. They travel together because a daemon that could
//! resolve a session but not create one, or the reverse, is a daemon nobody
//! can sign in to.

use std::sync::Arc;

use std::io::Write;

use super::account_admin::AdminBootstrap;
use super::account_routes::{AccountAuth, AccountReply};
use super::tenant_auth::{IdentityVerifier, StaticVerifier};
use super::Result;

/// Write one account-tier answer, its `Set-Cookie` included.
///
/// The header is carried in the reply rather than returned beside it so that a
/// route which forgets to send the cookie cannot also forget to mention it:
/// a sign-in whose cookie went missing looks like a successful sign-in to
/// every test that only reads the body.
pub(super) fn write_account_reply<S: Write>(
    stream: &mut S,
    reply: &AccountReply,
    cors_origin: Option<&str>,
) -> Result<()> {
    let headers: Vec<(&str, &str)> = reply
        .cookies
        .iter()
        .map(|cookie| ("Set-Cookie", cookie.as_str()))
        .collect();
    crate::mcp_serve::write_mcp_http_response_with_headers(
        stream,
        reply.status,
        &reply.body,
        cors_origin,
        &headers,
    )?;
    Ok(())
}

/// Say what the deployment did about its first administrator.
///
/// Only the outcomes an operator has to act on are printed: a deployment that
/// never set the variables says nothing (it has either been running for months
/// or has an admin made by `op admin create`), and one that still has them set
/// says nothing either — the variables are read and ignored on every start
/// after the first, and a line about it on every boot is a line that trains its
/// reader to stop looking. Never printed: the password.
pub(super) fn report_first_admin(outcome: AdminBootstrap) {
    const PREFIX: &str = "openpencil --serve-web --online:";
    if outcome.is_quiet() {
        return;
    }
    match outcome {
        // Answered by `is_quiet` above; listed so the match stays exhaustive
        // and a new outcome has to be considered here.
        AdminBootstrap::NotRequested | AdminBootstrap::Ignored { .. } => {}
        AdminBootstrap::Created { username } => eprintln!(
            "{PREFIX} created the first administrator `{username}` from {}; the \
             {}-variables are ignored from now on",
            super::account_admin::ADMIN_USERNAME_ENV,
            super::account_admin::ADMIN_USERNAME_ENV,
        ),
        AdminBootstrap::Incomplete => eprintln!(
            "{PREFIX} {} and {} must be set together; no administrator was created",
            super::account_admin::ADMIN_USERNAME_ENV,
            super::account_admin::ADMIN_PASSWORD_ENV
        ),
        AdminBootstrap::Refused(reason) => eprintln!(
            "{PREFIX} {} was refused ({reason}); no administrator was created — set a \
             stronger one, or run `op admin create`",
            super::account_admin::ADMIN_PASSWORD_ENV
        ),
        AdminBootstrap::Failed(error) => {
            eprintln!("{PREFIX} the first administrator could not be created: {error}")
        }
    }
}

/// The identity tier this deployment serves with.
pub(super) struct IdentityTier {
    pub(super) verifier: Arc<dyn IdentityVerifier>,
    pub(super) accounts: Option<AccountAuth>,
}

/// Pick the identity tier this deployment runs.
///
/// The account store is the production answer, and the only one a deployment
/// can reach: with a data directory configured — which `--online` requires for
/// its documents anyway — identities come from this deployment's own rows.
///
/// [`StaticVerifier`] stays reachable so the development smoke test works
/// without a store, and NOT as a fallback a real deployment can land on: it
/// needs `OPENPENCIL_ONLINE_STATIC_IDENTITIES` to be set by hand, and every
/// start that uses it says so on stderr. A deployment that sets that variable
/// has said, in its own environment, that it wants a development identity
/// table.
pub(super) fn resolve_identity_tier() -> IdentityTier {
    let configured = AccountAuth::open_from_env();
    let static_table = std::env::var(super::tenant_auth::STATIC_IDENTITIES_ENV).unwrap_or_default();
    identity_tier(configured, &static_table)
}

/// The decision [`resolve_identity_tier`] makes, with the environment already
/// read.
///
/// Split out so the property that matters can be asserted without mutating a
/// process-wide variable: **a deployment with a configured account store runs
/// the account store and nothing else**, and the development table is reachable
/// only by an operator who set its variable on purpose.
pub(super) fn identity_tier(
    configured: std::result::Result<Option<AccountAuth>, crate::accounts::AccountsError>,
    static_table: &str,
) -> IdentityTier {
    const PREFIX: &str = "openpencil --serve-web --online:";
    match configured {
        Ok(Some(accounts)) => {
            eprintln!(
                "{PREFIX} identities come from this deployment's accounts in {}",
                accounts.db().dir().display()
            );
            return IdentityTier {
                verifier: Arc::new(accounts.verifier()),
                accounts: Some(accounts),
            };
        }
        Ok(None) => {
            // No account store: every credential is refused with
            // `verifier-unavailable` unless the operator has explicitly asked
            // for the development table below. Said here rather than left to
            // the first failed request.
            eprintln!(
                "{PREFIX} no account store configured (set {}); every authenticated route \
                 will answer 503",
                crate::accounts::DATA_DIR_ENV
            );
        }
        Err(error) => {
            // Configured and unusable. Falling back to the development table
            // here — even when its variable IS set — would mean a deployment
            // with real accounts quietly serving identities out of an
            // environment string instead. The tier is empty instead, and every
            // credentialed route answers 503: a diagnosable state that cannot
            // be mistaken for a working deployment.
            eprintln!(
                "{PREFIX} the account store in {} cannot be opened ({error}); every \
                 authenticated route will answer 503",
                std::env::var(crate::accounts::DATA_DIR_ENV).unwrap_or_default()
            );
            return IdentityTier {
                verifier: Arc::new(StaticVerifier::parse("")),
                accounts: None,
            };
        }
    }
    let static_verifier = StaticVerifier::parse(static_table);
    if static_verifier.is_empty() {
        // Fail loud but keep serving: every request answers 503
        // `verifier-unavailable`, which is a diagnosable state. Serving
        // requests with NO verifier would be the unsafe alternative.
        eprintln!(
            "{PREFIX} no identity verifier configured (set {} for a deployment, or {} for \
             development); every authenticated route will answer 503",
            crate::accounts::DATA_DIR_ENV,
            super::tenant_auth::STATIC_IDENTITIES_ENV
        );
    } else {
        eprintln!(
            "{PREFIX} using the DEVELOPMENT static identity table from {}; this is not a \
             deployment",
            super::tenant_auth::STATIC_IDENTITIES_ENV
        );
    }
    IdentityTier {
        verifier: Arc::new(static_verifier),
        accounts: None,
    }
}
