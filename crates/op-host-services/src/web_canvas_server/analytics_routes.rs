//! Analytics assets: the markdown a section is built from.
//!
//! | Route | Answers |
//! | --- | --- |
//! | `GET /api/analytics` | the caller's own assets, most recently touched first |
//! | `POST /api/analytics` | `{name, markdown}` — a new asset |
//! | `GET /api/analytics/<key>` | the markdown, its name, and its digest |
//! | `POST /api/analytics/<key>` | `{markdown}` — replace the text |
//! | `POST /api/analytics/<key>/rename` | `{name}` — rename, keeping the address |
//! | `DELETE /api/analytics/<key>` | remove it, file and row |
//!
//! ## Why this is its own route family
//!
//! A document route is reached through a document's key, and everything in front
//! of it is a question about that document. An analytics asset is not a document
//! and is reached by nobody's key but its own: it is an ASSET, referenced by any
//! number of sections in any number of documents (the operator's decision), so
//! there is no document to hang the access preamble on. What it has instead is
//! an owner — the account that loaded it — and that is what these routes decide
//! from.
//!
//! ## Who may, and the one that is deliberately narrow
//!
//! | What | Who |
//! | --- | --- |
//! | Read an asset | the account that owns it |
//! | Create, edit, rename, delete | the owner, holding a role that may write analytics: admin, UX/UI, analyst |
//!
//! The write rule is the `AnalyticsWrite` row of the operator's matrix
//! ([`super::section_rights`]), and it is applied from the roles here rather than
//! through `decide_subject`, because that decision starts from a DOCUMENT — it
//! asks whether the caller reaches the document the subject lives in — and an
//! asset is reached by its own key. A local deployment (no accounts) has nobody
//! to distinguish between callers, exactly as `RequestAccess::decide` says.
//!
//! Reading is narrower than the matrix's "anyone who may read the document", and
//! that is a known gap rather than a decision: a visitor given a section to read
//! cannot fetch the analytics that section names, because the asset's route has
//! no document in it to ask about. Filed as issue #110 — see it before widening
//! this, because the fix is to decide how a section vouches for its asset, not
//! to open the route to every signed-in caller.
//!
//! ## What these routes never touch
//!
//! The document, and the sections that reference an asset. Deleting one leaves
//! the links alone on purpose: the sections did not change, something they were
//! built from went away, and their link state says exactly that
//! (`op_editor_core::section::LinkState::AssetMissing`).

use crate::analytics_store::{self, AnalyticsAsset, AnalyticsDocument};
use crate::document_db::DocumentDb;
use crate::section_store_error::SectionStoreError;
use op_editor_core::access::ProductRole;

use super::request_access::{refusal_reply, RequestAccess};
use super::WebCanvasState;
use super::WebReply;

/// Handle every `/api/analytics*` request that reaches this far.
pub(super) fn handle(
    method: &str,
    path: &str,
    body: &str,
    state: &mut WebCanvasState,
    access: &RequestAccess<'_>,
) -> WebReply {
    let Some(route) = parse_route(path) else {
        return super::not_found_reply();
    };
    let store = match crate::document_db::local_store(&mut state.documents) {
        Ok(store) => store,
        Err(error) => return super::files_routes::store_error_reply(error),
    };
    match (method, route) {
        ("GET", AnalyticsRoute::List) => list(&store, access),
        ("POST", AnalyticsRoute::List) => create(&store, body, access),
        ("GET", AnalyticsRoute::Asset { key }) => read(&store, key, access),
        ("POST", AnalyticsRoute::Asset { key }) => write(&store, key, body, access),
        ("POST", AnalyticsRoute::Rename { key }) => rename(&store, key, body, access),
        ("DELETE", AnalyticsRoute::Asset { key }) => delete(&store, key, access),
        _ => super::not_found_reply(),
    }
}

/// What a path under `/api/analytics` names.
enum AnalyticsRoute<'a> {
    /// The collection: list, or create.
    List,
    /// One asset: read, write, delete.
    Asset { key: &'a str },
    /// One asset's name, keeping its address.
    Rename { key: &'a str },
}

fn parse_route(path: &str) -> Option<AnalyticsRoute<'_>> {
    let rest = path.strip_prefix("/api/analytics")?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Some(AnalyticsRoute::List);
    }
    let mut segments = rest.split('/');
    let Some(key) = segments.next().filter(|key| !key.is_empty()) else {
        return None;
    };
    match (segments.next(), segments.next()) {
        (None, None) => Some(AnalyticsRoute::Asset { key }),
        (Some("rename"), None) => Some(AnalyticsRoute::Rename { key }),
        // Deeper than an asset is not a route: the markdown is replaced whole,
        // and the name has its own path.
        _ => None,
    }
}

fn list(store: &DocumentDb, access: &RequestAccess<'_>) -> WebReply {
    // `caller_id()` is `None` for the local operator, which is exactly the
    // owner filter an unattributed asset needs — one statement, no branch.
    match analytics_store::list(store, access.caller_id()) {
        Ok(assets) => ok_json(serde_json::json!({
            "ok": true,
            "assets": assets.iter().map(asset_json).collect::<Vec<_>>(),
        })),
        Err(error) => store_error_reply(error),
    }
}

fn create(store: &DocumentDb, body: &str, access: &RequestAccess<'_>) -> WebReply {
    if let Err(refusal) = may_write(access) {
        return refusal_reply(refusal);
    }
    let Ok(request) = serde_json::from_str::<CreateRequest>(body) else {
        return malformed_reply("expected a JSON object with `name` and optionally `markdown`");
    };
    let markdown = request.markdown.unwrap_or_default();
    match analytics_store::create(store, request.name.trim(), access.caller_id(), &markdown) {
        Ok(asset) => ok_json(serde_json::json!({
            "ok": true,
            "asset": asset_json(&asset),
        })),
        Err(error) => store_error_reply(error),
    }
}

fn read(store: &DocumentDb, key: &str, access: &RequestAccess<'_>) -> WebReply {
    match load_owned(store, key, access) {
        Ok(document) => ok_json(serde_json::json!({
            "ok": true,
            "asset": asset_json(&document.asset),
            "markdown": document.markdown,
            // The fingerprint of the text as it is NOW — what a section's link
            // is compared against, never a copy of what a link recorded.
            "digest": document.digest,
        })),
        Err(reply) => reply,
    }
}

fn write(store: &DocumentDb, key: &str, body: &str, access: &RequestAccess<'_>) -> WebReply {
    if let Err(refusal) = may_write(access) {
        return refusal_reply(refusal);
    }
    if let Err(reply) = load_owned(store, key, access) {
        return reply;
    }
    let Ok(request) = serde_json::from_str::<WriteRequest>(body) else {
        return malformed_reply("expected a JSON object with `markdown`");
    };
    match analytics_store::write(store, key, &request.markdown) {
        Ok(document) => ok_json(serde_json::json!({
            "ok": true,
            "asset": asset_json(&document.asset),
            "digest": document.digest,
        })),
        Err(error) => store_error_reply(error),
    }
}

fn rename(store: &DocumentDb, key: &str, body: &str, access: &RequestAccess<'_>) -> WebReply {
    if let Err(refusal) = may_write(access) {
        return refusal_reply(refusal);
    }
    if let Err(reply) = load_owned(store, key, access) {
        return reply;
    }
    let Ok(request) = serde_json::from_str::<RenameRequest>(body) else {
        return malformed_reply("expected a JSON object with `name`");
    };
    match analytics_store::rename(store, key, request.name.trim()) {
        Ok(asset) => ok_json(serde_json::json!({
            "ok": true,
            "asset": asset_json(&asset),
        })),
        Err(error) => store_error_reply(error),
    }
}

fn delete(store: &DocumentDb, key: &str, access: &RequestAccess<'_>) -> WebReply {
    if let Err(refusal) = may_write(access) {
        return refusal_reply(refusal);
    }
    if let Err(reply) = load_owned(store, key, access) {
        return reply;
    }
    match analytics_store::delete(store, key) {
        Ok(()) => ok_json(serde_json::json!({ "ok": true })),
        Err(error) => store_error_reply(error),
    }
}

/// The asset, if this caller owns it. The reply is the refusal to send.
///
/// Ownership rather than a role: the asset belongs to the account that loaded
/// it, and an account that may write analytics in general may not rewrite
/// somebody else's asset. A local deployment has no accounts, so both sides are
/// `None` and the operator reaches their own files exactly as they always have.
fn load_owned(
    store: &DocumentDb,
    key: &str,
    access: &RequestAccess<'_>,
) -> Result<AnalyticsDocument, WebReply> {
    let found = match analytics_store::find(store, key) {
        Ok(found) => found,
        Err(error) => return Err(store_error_reply(error)),
    };
    let Some(asset) = found else {
        return Err(not_found_asset_reply());
    };
    if asset.owner_id.as_deref() != access.caller_id() {
        // The same answer for "not yours" as for "not there" would be the
        // quieter option, and the wrong one: an owner who mistyped a key and a
        // caller reaching for somebody else's asset are different facts, and
        // only one of them is the caller's problem.
        return Err(forbidden_reply());
    }
    match analytics_store::read(store, key) {
        Ok(document) => Ok(document),
        Err(error) => Err(store_error_reply(error)),
    }
}

/// Whether this caller may change an analytics asset at all.
fn may_write(access: &RequestAccess<'_>) -> Result<(), AccessRefusal> {
    // A deployment with no accounts has nobody to distinguish between callers.
    if !access.mode().is_online() {
        return Ok(());
    }
    let holds = access.caller_roles().iter().any(|role| {
        matches!(
            role,
            ProductRole::Admin | ProductRole::UxUi | ProductRole::Analyst
        )
    });
    if holds {
        Ok(())
    } else {
        Err(AccessRefusal::ReadOnly)
    }
}

/// The refusal a role the matrix does not name earns.
type AccessRefusal = super::request_access::AccessRefusal;

fn asset_json(asset: &AnalyticsAsset) -> serde_json::Value {
    serde_json::json!({
        "key": asset.key,
        "name": asset.name,
        "createdAt": asset.created_at,
        "updatedAt": asset.updated_at,
        "size": asset.size,
    })
}

#[derive(serde::Deserialize)]
struct CreateRequest {
    name: String,
    #[serde(default)]
    markdown: Option<String>,
}

#[derive(serde::Deserialize)]
struct WriteRequest {
    markdown: String,
}

#[derive(serde::Deserialize)]
struct RenameRequest {
    name: String,
}

fn ok_json(value: serde_json::Value) -> WebReply {
    WebReply {
        status: "200 OK",
        body: value.to_string(),
    }
}

fn malformed_reply(message: &str) -> WebReply {
    WebReply {
        status: "400 Bad Request",
        body: serde_json::json!({
            "ok": false,
            "error": "malformed-analytics-request",
            "message": message,
        })
        .to_string(),
    }
}

fn not_found_asset_reply() -> WebReply {
    WebReply {
        status: "404 Not Found",
        body: serde_json::json!({
            "ok": false,
            "error": "analytics-not-found",
            "message": "no analytics document carries that key",
        })
        .to_string(),
    }
}

fn forbidden_reply() -> WebReply {
    WebReply {
        status: "403 Forbidden",
        body: serde_json::json!({
            "ok": false,
            "error": "analytics-not-yours",
            "message": "this analytics document belongs to another account",
        })
        .to_string(),
    }
}

/// The reply for a store failure, in the one shape this family uses.
fn store_error_reply(error: SectionStoreError) -> WebReply {
    let status = match error {
        SectionStoreError::InvalidKey
        | SectionStoreError::EmptyNodeId
        | SectionStoreError::NameTooLong { .. }
        | SectionStoreError::TooLarge { .. } => "400 Bad Request",
        SectionStoreError::NotFound => "404 Not Found",
        SectionStoreError::Properties(_)
        | SectionStoreError::MissingFile { .. }
        | SectionStoreError::Database(_)
        | SectionStoreError::Io(_) => "500 Internal Server Error",
    };
    WebReply {
        status,
        body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
    }
}

#[cfg(test)]
#[path = "analytics_routes_tests.rs"]
mod tests;
