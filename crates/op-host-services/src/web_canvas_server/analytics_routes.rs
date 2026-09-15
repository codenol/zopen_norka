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
//! an owner — the account that loaded it — which answers every WRITE here, and a
//! READ is answered by that owner or by a section that links the asset (the
//! table below, and [`load_readable`] for the rule itself).
//!
//! ## Who may, and what makes that answer wider than owning
//!
//! | What | Who |
//! | --- | --- |
//! | Read an asset | the account that owns it, or a reader of a document that links it and belongs to the same account |
//! | Create, edit, rename, delete | the owner, holding a role that may write analytics: admin, UX/UI, analyst |
//!
//! The write rule is the `AnalyticsWrite` row of the operator's matrix
//! ([`super::section_rights`]), and it is applied from the roles here rather than
//! through `decide_subject`, because that decision starts from a DOCUMENT — it
//! asks whether the caller reaches the document the subject lives in — and an
//! asset is reached by its own key. A local deployment (no accounts) has nobody
//! to distinguish between callers, exactly as `RequestAccess::decide` says.
//!
//! The READ rule is the matrix's other row — "Reading any of it: anyone who may
//! read the document" — and it is narrower than that sentence sounds, because an
//! asset is reached by a key rather than by a document. See [`load_readable`] for
//! the rule as it is implemented and why each half of it is load-bearing. Until
//! issue #110 landed this route answered from ownership alone, and a visitor
//! given a section to read could see the analytics its link named and not fetch
//! the markdown behind it — the one click the whole feature exists for.
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
    match load_readable(store, key, access) {
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
///
/// Reading is [`load_readable`], not this: a WRITE is the owner's alone, where a
/// read may be vouched for by a section.
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

/// The asset, if this caller may READ it. The reply is the refusal to send.
///
/// Reading is wider than owning, and the matrix says so in as many words
/// ("Reading any of it — anyone who may read the document",
/// [`super::section_rights`]). The rule this implements is the narrowest honest
/// spelling of that sentence, and issue #110 is where the gap was found:
///
/// > An asset is readable by whoever may read a document that links it, when
/// > the asset belongs to the same account as that document.
///
/// Both halves are needed. The first is the operator's sentence: a section that
/// names an analytics document is what makes it part of the document a reader
/// was given, and without it the feature's whole promise — "get from the screen
/// to the reasoning without asking anybody" — stops one click short for exactly
/// the reader it was designed for. The second is what keeps that promise from
/// becoming a key hunt: anybody who may write a section could otherwise paste a
/// guessed key into their own document and read whatever it named, which is the
/// leak the issue refuses to trade the gap for. The document that vouches must
/// belong to the account the asset belongs to, so a link can only ever vouch for
/// an asset of its own workspace.
///
/// The document is the one this request was ADMITTED through
/// ([`RequestAccess::lease_document`]), not merely one the caller may open:
/// admission is per document (`TenantRegistry::lease_for_shared`), so the request
/// already had to name a document the caller is on the access list of, and the
/// share that lets them here is the same share that lets them read the section
/// naming the asset. A caller who cannot name one — a request that says nothing
/// about which document it is about — is refused, which is the fail-closed
/// direction.
///
/// Writes are deliberately not widened with it: [`write`], [`rename`] and
/// [`delete`] still go through [`load_owned`], because a reader's share of an
/// asset is not authority over it.
fn load_readable(
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
    if asset.owner_id.as_deref() != access.caller_id()
        && !linked_from_a_readable_document(store, key, &asset, access)
    {
        return Err(forbidden_reply());
    }
    match analytics_store::read(store, key) {
        Ok(document) => Ok(document),
        Err(error) => Err(store_error_reply(error)),
    }
}

/// Whether a section of a document this caller may read links the asset.
///
/// The facts the rule above is made of, and nothing else: the document this
/// request was admitted through, its owner (which must be the asset's, or a link
/// would vouch across accounts), that the caller reaches it, and a stored
/// section of it naming the asset's key. A store failure anywhere in here
/// answers `false`: the question is "does something vouch for this reader", and
/// a link this daemon cannot read is not something that can.
fn linked_from_a_readable_document(
    store: &DocumentDb,
    asset_key: &str,
    asset: &AnalyticsAsset,
    access: &RequestAccess<'_>,
) -> bool {
    // One operator, one directory: everything in it is theirs, and the ownership
    // branch above has already answered. This is the branch that keeps a local
    // deployment out of the rest of this function.
    if !access.mode().is_online() {
        return false;
    }
    let Some(document) = access.lease_document() else {
        return false;
    };
    // The row's owner, not the lease's: a document row is what a share is
    // actually about, and the asset must belong to the same account as the
    // document that links it. `None` on either side matches nothing online — an
    // unattributed asset is nobody's, so nobody's link vouches for it.
    let owner = match crate::document_store::find(store, document) {
        Ok(Some(entry)) => entry.owner_id,
        _ => return false,
    };
    if owner.as_deref() != asset.owner_id.as_deref() || owner.is_none() {
        return false;
    }
    if !access.reaches_stored_document(owner.as_deref()) {
        return false;
    }
    matches!(
        crate::section_store::references_asset(store, document, asset_key),
        Ok(true)
    )
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
