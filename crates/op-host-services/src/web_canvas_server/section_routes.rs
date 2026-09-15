//! What a section carries: the analytics it came from, what it says, its flows.
//!
//! | Route | Answers |
//! | --- | --- |
//! | `GET /api/files/<key>/sections` | every section of the document that has properties |
//! | `GET /api/files/<key>/sections/<node>` | one section; empty properties when nobody wrote any |
//! | `POST /api/files/<key>/sections/<node>` | write them |
//! | `DELETE /api/files/<key>/sections/<node>` | forget them |
//!
//! ## Why these live inside the `/api/files` family
//!
//! A section is a frame in a document, and its properties are reached through
//! the document's key, so everything in front of these handlers is a property of
//! that key: the access gate, the key's shape, the store, and the check that the
//! row belongs to an account this caller may address. Writing a second family
//! prefix would mean writing that preamble twice, which is how two authorization
//! preambles drift apart.
//!
//! ## Why the rights are asked by SUBJECT and not by route
//!
//! An editor of the document is not automatically the author of what a section
//! says: the summary is a READING of the analytics and belongs to whoever wrote
//! the analytics, the flow is a design decision and belongs to UX/UI, and the
//! analytics markdown is an asset of its own. [`super::section_rights`] owns
//! that matrix, and it answers about the SUBJECT because that is the question
//! the operator's rule is phrased in — see its module docs for why this is not a
//! wider `Rights` bitmask.
//!
//! ## Why a write compares instead of trusting a field list
//!
//! The route is handed the whole properties object and works out what changed by
//! comparing it with what is stored. A body naming "the fields I touched" would
//! be a client's claim about its own authority, and the one thing a request body
//! may never be is a statement about what the caller is allowed to do. Two
//! consequences worth stating: an unchanged write asks for nothing (there is
//! nothing to authorize), and a body that clears the summary asks for the
//! summary right, exactly as a body that rewrites it does.
//!
//! ## What these routes deliberately do not touch
//!
//! The document itself. Nothing here opens the editor state, so a section's
//! properties cannot move the document's version, dirty it, or change a
//! collaboration hash — the frame a section marks is the document's business,
//! and this table is about what hangs off it.

use op_editor_core::section::SectionProperties;
use op_editor_core::NodeId;

use super::request_access::{refusal_reply, RequestAccess};
use super::section_rights::SubjectAction;
use super::WebReply;
use crate::document_db::DocumentDb;
use crate::section_store;
use crate::section_store_error::SectionStoreError;

/// Handle every `/api/files/<key>/sections*` request that reaches this far.
pub(super) fn handle(
    method: &str,
    key: &str,
    node: Option<&str>,
    body: &str,
    store: &DocumentDb,
    access: &RequestAccess<'_>,
) -> WebReply {
    match (method, node) {
        ("GET", None) => list(store, key),
        ("GET", Some(node)) => read(store, key, node),
        ("POST", Some(node)) => write(store, key, node, body, access),
        ("DELETE", Some(node)) => forget(store, key, node, access),
        _ => super::not_found_reply(),
    }
}

/// Every section of the document that has something written about it.
///
/// A section nobody wrote about has no row and is therefore absent — not
/// missing, and not an error. The document's frames are the authority on which
/// sections exist; this list is the authority on which of them say anything.
fn list(store: &DocumentDb, key: &str) -> WebReply {
    match section_store::list(store, key) {
        Ok(records) => super::files_routes::ok_json(serde_json::json!({
            "ok": true,
            "sections": records
                .iter()
                .map(|record| serde_json::json!({
                    "nodeId": record.node_id.as_str(),
                    "properties": record.properties,
                    "updatedAt": record.updated_at,
                }))
                .collect::<Vec<_>>(),
        })),
        Err(error) => store_error_reply(error),
    }
}

/// One section's properties. Empty when nobody has written any.
fn read(store: &DocumentDb, key: &str, node: &str) -> WebReply {
    let Some(node) = parse_node(node) else {
        return empty_node_reply();
    };
    match section_store::load(store, key, &node) {
        Ok(properties) => super::files_routes::ok_json(serde_json::json!({
            "ok": true,
            "nodeId": node.as_str(),
            "properties": properties.clone().unwrap_or_default(),
            // Whether this is written down or merely the shape of an empty
            // section. A panel that showed "nothing here" for both would tell a
            // reader that somebody erased the summary.
            "stored": properties.is_some(),
        })),
        Err(error) => store_error_reply(error),
    }
}

/// Write a section's properties.
fn write(
    store: &DocumentDb,
    key: &str,
    node: &str,
    body: &str,
    access: &RequestAccess<'_>,
) -> WebReply {
    let Some(node) = parse_node(node) else {
        return empty_node_reply();
    };
    let Ok(properties) = serde_json::from_str::<SectionProperties>(body) else {
        return malformed_reply();
    };
    let before = match section_store::load(store, key, &node) {
        Ok(before) => before.unwrap_or_default(),
        Err(error) => return store_error_reply(error),
    };
    for action in required_actions(&before, &properties) {
        if let Err(refusal) = access.decide_subject(action) {
            return refusal_reply(refusal);
        }
    }
    let changed = before != properties;
    match section_store::save(store, key, &node, &properties) {
        Ok(()) => super::files_routes::ok_json(serde_json::json!({
            "ok": true,
            "nodeId": node.as_str(),
            "changed": changed,
        })),
        Err(error) => store_error_reply(error),
    }
}

/// Forget a section's properties — what unmarking a frame calls.
fn forget(store: &DocumentDb, key: &str, node: &str, access: &RequestAccess<'_>) -> WebReply {
    let Some(node) = parse_node(node) else {
        return empty_node_reply();
    };
    let before = match section_store::load(store, key, &node) {
        Ok(before) => before.unwrap_or_default(),
        Err(error) => return store_error_reply(error),
    };
    // Clearing is a write of "nothing", so it asks for exactly what clearing
    // that content would ask for — no more (an empty section asks nothing) and
    // no less.
    for action in required_actions(&before, &SectionProperties::empty()) {
        if let Err(refusal) = access.decide_subject(action) {
            return refusal_reply(refusal);
        }
    }
    match section_store::delete(store, key, &node) {
        Ok(removed) => super::files_routes::ok_json(serde_json::json!({
            "ok": true,
            "removed": removed,
        })),
        Err(error) => store_error_reply(error),
    }
}

/// Which subjects a write touches, and therefore which rights it needs.
///
/// One action per thing that actually differs. Sorted by the matrix's own order
/// so a refusal names the weakest right the caller is missing rather than
/// whichever comparison happened to run first.
fn required_actions(before: &SectionProperties, after: &SectionProperties) -> Vec<SubjectAction> {
    let mut actions = Vec::new();
    if before.analytics != after.analytics {
        actions.push(SubjectAction::AnalyticsWrite);
    }
    if before.summary != after.summary {
        actions.push(SubjectAction::SummaryWrite);
    }
    if before.flows != after.flows {
        actions.push(SubjectAction::UxFlowWrite);
    }
    actions
}

/// A node id out of the path, or `None` when it names nothing.
///
/// The store refuses an empty id as well; parsing it here means the route can
/// answer a 400 without opening a database to find that out.
fn parse_node(raw: &str) -> Option<NodeId> {
    NodeId::new_opt(raw)
}

fn empty_node_reply() -> WebReply {
    WebReply {
        status: "400 Bad Request",
        body: serde_json::json!({
            "ok": false,
            "error": "empty-node-id",
            "message": "a section is addressed by the id of the frame that marks it",
        })
        .to_string(),
    }
}

fn malformed_reply() -> WebReply {
    WebReply {
        status: "400 Bad Request",
        body: serde_json::json!({
            "ok": false,
            "error": "malformed-section-properties",
            "message": "expected a JSON object with `analytics`, `summary` and `flows`",
        })
        .to_string(),
    }
}

/// The reply for a store failure, in the one shape this family uses.
fn store_error_reply(error: SectionStoreError) -> WebReply {
    let status = match error {
        SectionStoreError::InvalidKey | SectionStoreError::EmptyNodeId => "400 Bad Request",
        SectionStoreError::NotFound => "404 Not Found",
        SectionStoreError::NameTooLong { .. } | SectionStoreError::TooLarge { .. } => {
            "400 Bad Request"
        }
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
#[path = "section_routes_tests.rs"]
mod tests;
