//! Which requests change a tenant's own configuration, and the gate in front of
//! them.
//!
//! [`super::document_writes`] answers "does this request change the document".
//! This one answers the other question the same pass turned up (#41): two routes
//! write the *workspace* rather than the document — the AI provider credentials
//! and the MCP server card — and neither asked anything.
//!
//! They are reachable with a `?tenant=` lease, which is how a browser addresses
//! a document someone shared with it. So a visitor whose roles grant no more
//! than reading could rewrite the owner's provider keys and the owner's MCP
//! port: the document was protected and the account behind it was not.
//!
//! The right is not the document's. Answering with
//! [`DocumentAction::Edit`](super::DocumentAction::Edit) would have made the
//! hole *worse* rather than smaller — an editing role is handed out to work on
//! a shared document, and it must not carry the owner's credentials with it. So
//! the question is [`RequestAccess::decide_workspace_settings`]: whose
//! workspace is this. The owner's, and an admin's.
//!
//! ## Why a table of its own rather than a row in `document_writes`
//!
//! The other table's contract is that it names the routes that change the
//! document, and a route in it that changes no document would make that
//! sentence false — the next reader would have to check every row. Two entries
//! and one predicate are cheap; an honest table is what makes the other one
//! trustworthy.

use super::request_access::{self, RequestAccess};
use super::WebReply;

/// The routes that change this account's own configuration.
///
/// Both are the settings modal: the credentials the AI panel talks to the
/// provider with, and the MCP server switch. Everything else under
/// `/api/settings*` and `/api/mcp/server` reads — `GET
/// /api/settings/credential-policy` reports what this deployment allows, `GET
/// /api/mcp/server` is the health probe a client needs before it can be told
/// anything at all — and is deliberately not named here.
pub(super) const CONFIGURATION_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/settings/credentials"),
    ("POST", "/api/mcp/server"),
];

/// The gate. `None` when the request may proceed.
pub(super) fn check(method: &str, path: &str, access: &RequestAccess<'_>) -> Option<WebReply> {
    if !CONFIGURATION_ROUTES.contains(&(method, path)) {
        return None;
    }
    match access.decide_workspace_settings() {
        Ok(()) => None,
        Err(refusal) => Some(request_access::refusal_reply(refusal)),
    }
}

#[cfg(test)]
#[path = "workspace_settings_tests.rs"]
mod tests;
