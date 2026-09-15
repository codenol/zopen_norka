//! `/api/share/*` handlers — who else may open this account's document.
//!
//! Online only. The local and managed daemons have one document and one
//! operator, so there is nothing to share and these routes are not mounted.
//!
//! ## The one invariant
//!
//! The grantor is always the request's VERIFIED identity. The body names the
//! account being granted, never the account doing the granting — otherwise
//! any caller could add themselves to any document's access list, which is
//! the whole security property inverted. The same identity is what the grant
//! RECORDS ([`op_editor_core::ShareGrant::invited_by`]), so "who added this
//! person" is answered from the same fact that authorized the add.
//!
//! ## Why a grant now names a level
//!
//! Because the dialog offers four (see [`op_editor_core::ShareLevel`]) and a
//! level the server does not store is a level the server does not enforce —
//! which would make the picker a decoration. Two checks guard it:
//!
//! 1. `RequestAccess::decide(DocumentAction::Invite)` — may this caller add
//!    anybody at all?
//! 2. [`ShareLevel::is_grantable_by`] against the caller's rights, floored at
//!    what ownership confers — you may not hand out what you do not hold.
//!
//! Grants run on the connection thread rather than under the document lock:
//! the access list is its own mutex on the tenant, so sharing is answerable
//! while a large document push is in flight.

use op_editor_core::access::Rights;
use op_editor_core::share_routes::{self, LINK_ACCESS};
use op_editor_core::{ShareGrant, ShareLevel};

use super::request_access::{AccessRefusal, DocumentAction, RequestAccess};
use super::tenant::{AclChange, TenantLease, TenantRegistry};
use super::tenant_auth::ResolvedIdentity;
use super::WebReply;

/// Longest body either POST accepts. Both are a single short account id, an
/// optional level, and — for the link switch — one flag.
const MAX_SHARE_BODY_BYTES: usize = 4 * 1024;

/// Longest account id accepted in a body.
const MAX_ACCOUNT_ID_CHARS: usize = 256;

/// Why a share request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareError {
    BodyTooLarge,
    MalformedRequest,
    /// The body named the caller's own account.
    SelfShare,
    /// The caller may not add people to this document.
    ///
    /// Carried as the same [`AccessRefusal`] the document routes answer with,
    /// so a client that already handles "read-only role" needs no second
    /// branch for the share route.
    Access(AccessRefusal),
    /// The level is above what the caller itself holds.
    ///
    /// You may not give away what you do not have. The owner is floored at
    /// [`Rights::EDITOR`] over its own document — ownership is what makes it
    /// the owner — so an owner without the account list may hand out anything
    /// up to editor and no further.
    LevelAboveOwn {
        level: ShareLevel,
        own: ShareLevel,
    },
    /// The body named an account this deployment does not have.
    ///
    /// A refusal rather than a recorded grant: an access list is keyed by
    /// account id, so an id nobody holds is a row that grants nothing — and a
    /// `200` for it tells the person who typed a NAME that they shared
    /// something.
    UnknownAccount,
    /// The account list could not be read, so whether the account exists is
    /// unknown. Fail closed.
    LookupUnavailable,
    /// The request did not name a document.
    ///
    /// A share is about ONE document: the account-wide list this replaced let a
    /// grant made in one document's dialog open every document the account
    /// owned (issue #127), so a request that names no document is refused
    /// rather than guessed at.
    MissingDocument,
}

impl ShareError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::BodyTooLarge => "payload-too-large",
            Self::MalformedRequest => "malformed-share-request",
            Self::SelfShare => "cannot-share-with-self",
            Self::Access(refusal) => refusal.code(),
            Self::LevelAboveOwn { .. } => "level-above-your-own",
            Self::UnknownAccount => "unknown-account",
            Self::MissingDocument => "missing-document",
            Self::LookupUnavailable => "account-lookup-unavailable",
        }
    }

    pub const fn http_status(&self) -> &'static str {
        match self {
            Self::BodyTooLarge => "413 Payload Too Large",
            Self::MalformedRequest | Self::SelfShare => "400 Bad Request",
            Self::Access(refusal) => refusal.http_status(),
            Self::LevelAboveOwn { .. } => "403 Forbidden",
            // Not 404: the ROUTE is here, the account is not. A client that
            // parsed a name out of its invite field needs to be told which of
            // the two it got wrong.
            Self::UnknownAccount | Self::MissingDocument => "400 Bad Request",
            Self::LookupUnavailable => "500 Internal Server Error",
        }
    }
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BodyTooLarge => f.write_str("body too large"),
            Self::MalformedRequest => f.write_str("malformed share request"),
            Self::SelfShare => f.write_str("an account already has access to its own document"),
            Self::Access(refusal) => refusal.fmt(f),
            Self::LevelAboveOwn { level, own } => write!(
                f,
                "you may not grant the {} level; you hold {}",
                level.wire(),
                own.wire()
            ),
            Self::UnknownAccount => f.write_str("this deployment has no account with that id"),
            Self::LookupUnavailable => {
                f.write_str("the account list could not be read, so the account was not checked")
            }
            Self::MissingDocument => f.write_str(
                "a share is about one document; name it with ?file=<key> or in the body",
            ),
        }
    }
}

impl std::error::Error for ShareError {}

/// Whether `path` is one of the share routes.
pub(super) fn is_share_route(path: &str) -> bool {
    matches!(
        path,
        share_routes::GRANT | share_routes::REVOKE | share_routes::LIST | LINK_ACCESS
    )
}

/// Dispatch one `/api/share/*` request.
///
/// `lease` is the CALLER's own tenant: grant and revoke edit the caller's
/// access list, never the tenant a `?tenant=` parameter pointed at. A visitor
/// cannot re-share a document they were merely given access to.
pub(super) fn handle(
    method: &str,
    path: &str,
    body: &str,
    identity: &ResolvedIdentity,
    lease: &TenantLease,
    registry: &TenantRegistry,
    // The deployment's account list, when it has one. The grant route asks it
    // whether the account in the body exists — see `ShareError::UnknownAccount`.
    accounts: Option<&super::account_routes::AccountAuth>,
    // The document this call is about, read from the path, the query or the
    // body by `share_routes::document_from_request`. `None` is refused: a share
    // that does not name a document is a share of everything the account owns
    // (issue #127).
    document: Option<&str>,
) -> WebReply {
    let Some(key) = document else {
        return error_reply(ShareError::MissingDocument);
    };
    match (method, path) {
        ("POST", share_routes::GRANT) => mutate(
            body,
            identity,
            lease,
            registry,
            accounts,
            key,
            Mutation::Grant,
        ),
        ("POST", share_routes::REVOKE) => mutate(
            body,
            identity,
            lease,
            registry,
            accounts,
            key,
            Mutation::Revoke,
        ),
        ("GET", share_routes::LIST) => list(identity, lease, registry, key),
        ("POST", LINK_ACCESS) => link_access(body, identity, lease, registry, key),
        _ => WebReply {
            status: "405 Method Not Allowed",
            body: crate::mcp_serve::rest_error_body("method not allowed for this share route"),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutation {
    Grant,
    Revoke,
}

/// What a share body asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShareRequest {
    account: String,
    level: ShareLevel,
}

fn mutate(
    body: &str,
    identity: &ResolvedIdentity,
    lease: &TenantLease,
    registry: &TenantRegistry,
    accounts: Option<&super::account_routes::AccountAuth>,
    key: &str,
    mutation: Mutation,
) -> WebReply {
    let parsed = match parse_account(body, &identity.user_id) {
        Ok(parsed) => parsed,
        Err(error) => return error_reply(error),
    };
    // Does that account exist? A deployment that has an account list can answer,
    // and it must: without this the route accepts a NAME, records it as a grant,
    // answers `200 changed:true` and grants nothing — the person it names is
    // refused with `tenant-not-shared` and the list shows a row that looks like
    // somebody. Found by running the share scenario against a real deployment.
    if mutation == Mutation::Grant {
        if let Some(accounts) = accounts {
            match accounts.db().find_user_by_id(&parsed.account) {
                Ok(Some(_)) => {}
                Ok(None) => return error_reply(ShareError::UnknownAccount),
                // A store that cannot answer must not turn into a grant: the
                // caller is told the deployment could not check, not that the
                // account is fine.
                Err(_) => return error_reply(ShareError::LookupUnavailable),
            }
        }
    }
    // May this caller add anybody at all? The same decision the document
    // routes make, from the same place, so "may invite" cannot mean two
    // things. The owner passes by identity; a visitor holding the access list
    // of somebody else's document does not.
    let rights = identity.roles.rights();
    let owner_rights = rights.union(Rights::EDITOR);
    if let Err(refusal) =
        RequestAccess::online(lease.owner_id(), identity, None).decide(DocumentAction::Invite)
    {
        return error_reply(ShareError::Access(refusal));
    }
    if mutation == Mutation::Grant {
        // Ownership is the floor: the owner of a document may always edit it
        // (`RequestAccess`'s module docs), so the owner may always hand out up
        // to editing. Above that the caller's own roles decide — only an
        // account that may manage users may create an administrator.
        let held = if lease.owner_id() == identity.user_id {
            owner_rights
        } else {
            rights
        };
        if !parsed.level.is_grantable_by(held) {
            return error_reply(ShareError::LevelAboveOwn {
                level: parsed.level,
                own: ShareLevel::from_rights(held),
            });
        }
    }
    let change = match mutation {
        Mutation::Grant => AclChange::Grant {
            account: parsed.account,
            level: parsed.level,
            // The verified identity, never the body.
            invited_by: Some(identity.user_id.clone()),
        },
        Mutation::Revoke => AclChange::Revoke(parsed.account),
    };
    // The edit and its write are one serialised operation. Persisted
    // immediately rather than at eviction: a share the user was told had
    // succeeded must survive a restart, and the document it applies to may not
    // be written for another half hour.
    match registry.update_acl(lease.owner_id(), lease.tenant(), key, change) {
        Ok(update) => WebReply {
            status: "200 OK",
            body: serde_json::json!({
                "ok": true,
                "changed": update.changed,
                "sharedWith": update
                    .grants
                    .iter()
                    .map(ShareGrant::to_json)
                    .collect::<Vec<_>>(),
            })
            .to_string(),
        },
        // A full access list is the caller's problem, not the server's: the
        // store writes a bounded list, so accepting the grant would report a
        // success that vanishes on the next save.
        Err(super::tenant_store::TenantStoreError::ShareLimitReached(limit)) => WebReply {
            status: "400 Bad Request",
            body: serde_json::json!({
                "ok": false,
                "error": "share-limit-reached",
                "limit": limit,
                "message": format!("this document is already shared with {limit} accounts"),
            })
            .to_string(),
        },
        // The change has been rolled back, so memory and disk agree and a
        // retry starts from a known state. Reporting 200 here — as the
        // previous code did — told the user a share had succeeded that would
        // vanish on the next restart.
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({
                "ok": false,
                "error": "share-not-persisted",
                "message": error.to_string(),
            })
            .to_string(),
        },
    }
}

/// Turn "anybody with the link" on, off, or to another level.
///
/// The widest thing this route family can do — it opens the document to every
/// signed-in account on the deployment — so it takes the same two checks a
/// grant does, and one rule of its own: the level is capped by what the caller
/// holds, exactly as a grant is.
fn link_access(
    body: &str,
    identity: &ResolvedIdentity,
    lease: &TenantLease,
    registry: &TenantRegistry,
    key: &str,
) -> WebReply {
    let parsed = match parse_link_access(body) {
        Ok(parsed) => parsed,
        Err(error) => return error_reply(error),
    };
    let rights = identity.roles.rights();
    if let Err(refusal) =
        RequestAccess::online(lease.owner_id(), identity, None).decide(DocumentAction::Invite)
    {
        return error_reply(ShareError::Access(refusal));
    }
    let held = if lease.owner_id() == identity.user_id {
        rights.union(Rights::EDITOR)
    } else {
        rights
    };
    if let Some(level) = parsed {
        if !level.is_grantable_by(held) {
            return error_reply(ShareError::LevelAboveOwn {
                level,
                own: ShareLevel::from_rights(held),
            });
        }
    }
    match registry.update_link_access(lease.owner_id(), lease.tenant(), key, parsed) {
        Ok(level) => WebReply {
            status: "200 OK",
            body: serde_json::json!({
                "ok": true,
                "linkAccess": level.map(|level| level.wire()),
            })
            .to_string(),
        },
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({
                "ok": false,
                "error": "share-not-persisted",
                "message": error.to_string(),
            })
            .to_string(),
        },
    }
}

fn list(
    identity: &ResolvedIdentity,
    lease: &TenantLease,
    registry: &TenantRegistry,
    key: &str,
) -> WebReply {
    WebReply {
        status: "200 OK",
        body: serde_json::json!({
            "ok": true,
            // Who may open this account's document, at what level, and who
            // added them…
            "sharedWith": lease
                .tenant()
                .share_grants(key)
                .iter()
                .map(ShareGrant::to_json)
                .collect::<Vec<_>>(),
            "linkAccess": lease
                .tenant()
                .link_access(key)
                .map(|level| level.wire()),
            // …and whose documents this account may open, with the level each
            // one gives them — a guest who is not told what they hold cannot
            // act on it. Resident owners only; see
            // `TenantRegistry::shared_with_visitor`.
            "sharedWithMe": registry
                .shared_with_visitor(&identity.user_id)
                .iter()
                .map(|shared| serde_json::json!({
                    "owner": shared.owner,
                    "file": shared.key,
                    "level": shared.level.wire(),
                }))
                .collect::<Vec<_>>(),
        })
        .to_string(),
    }
}

/// Pull the target account and level out of a share body.
///
/// The account id is not validated against the hub: this deployment has no
/// user-lookup endpoint yet, so an id that belongs to nobody simply grants
/// access to nobody. The dialog says so rather than implying a lookup
/// happened — see `share.invite.placeholder`'s caption.
fn parse_account(body: &str, caller: &str) -> Result<ShareRequest, ShareError> {
    if body.len() > MAX_SHARE_BODY_BYTES {
        return Err(ShareError::BodyTooLarge);
    }
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ShareError::MalformedRequest)?;
    let account = parsed
        .get("userId")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ShareError::MalformedRequest)?;
    if account.chars().count() > MAX_ACCOUNT_ID_CHARS {
        return Err(ShareError::MalformedRequest);
    }
    if account == caller {
        return Err(ShareError::SelfShare);
    }
    // A body that names no level gets the weakest one. Fail closed: a client
    // older than levels must not be able to hand out editing by omission.
    let level = match parsed.get("level").and_then(|value| value.as_str()) {
        Some(raw) => ShareLevel::from_wire(raw).map_err(|_| ShareError::MalformedRequest)?,
        None => ShareLevel::DEFAULT,
    };
    Ok(ShareRequest {
        account: account.to_string(),
        level,
    })
}

/// Pull the link-access switch out of its body: `{"enabled":bool,"level":str}`.
fn parse_link_access(body: &str) -> Result<Option<ShareLevel>, ShareError> {
    if body.len() > MAX_SHARE_BODY_BYTES {
        return Err(ShareError::BodyTooLarge);
    }
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ShareError::MalformedRequest)?;
    let enabled = parsed
        .get("enabled")
        .and_then(|value| value.as_bool())
        .ok_or(ShareError::MalformedRequest)?;
    if !enabled {
        return Ok(None);
    }
    let level = parsed
        .get("level")
        .and_then(|value| value.as_str())
        .ok_or(ShareError::MalformedRequest)?;
    Ok(Some(
        ShareLevel::from_wire(level).map_err(|_| ShareError::MalformedRequest)?,
    ))
}

fn error_reply(error: ShareError) -> WebReply {
    WebReply {
        status: error.http_status(),
        body: serde_json::json!({
            "ok": false,
            "error": error.code(),
            "message": error.to_string(),
        })
        .to_string(),
    }
}

#[cfg(test)]
#[path = "share_routes_tests.rs"]
mod tests;
