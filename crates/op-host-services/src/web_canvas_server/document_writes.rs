//! Which requests change the document, and the one gate in front of them.
//!
//! The document's access decision ([`super::request_access`]) was wired through
//! the routes that own a document — the file store and the recovery draft — but
//! not through the routes that *carry* one. `POST /api/mcp/document` installs
//! the document the browser is editing, the JSON-RPC `/mcp` tier applies tool
//! calls to it, `POST /api/mcp/sync-reset` replaces it with the starter, and the
//! AI design turn applies commands to it. None of them asked. The only gate on
//! that half was the credential's scopes ([`super::tool_scopes`]), and a browser
//! session carries every scope — a session IS the account, and scopes exist to
//! narrow a token *below* it. So an account whose roles grant no edit could
//! rewrite the document through any of these routes, which is the hole (#33)
//! this module closes.
//!
//! ## One table, in front of everything
//!
//! A route asks by naming what it does, and this table is where that name comes
//! from — the same shape [`super::files_routes::required_action`] already uses
//! one tier down. The gate is called from the two entry points that between
//! them see every route:
//!
//! 1. [`handle_web_canvas_request`](super::handle_web_canvas_request), as its
//!    first statement, so the whole REST tier is decided before its `match` —
//!    including the document push, which installs a whole document inside it.
//! 2. [`connection::dispatch`](super::connection), immediately after the scope
//!    gate, for the tiers that are dispatched *ahead* of the REST handler: the
//!    pre-parsed document push, the JSON-RPC `/mcp` tier and the `/api/ai/*`
//!    tier. This is where the scope gate itself had to move to, for exactly
//!    this reason — checked inside the `/api/*` branch it left every
//!    specially-dispatched route ungated, and a gate that one of those routes
//!    can walk around is not a gate.
//!
//! A REST request is therefore classified twice, by a pure function with the
//! same inputs both times: the second answer cannot differ from the first. The
//! alternative — one check per tier, each with its own list — is how the route
//! that gets missed stays missed.
//!
//! ## The table names what changes the document, and nothing else
//!
//! A route the table does not name does not change the document, so nothing is
//! asked about it, and it proceeds exactly as it did before. That polarity is
//! the opposite of [`super::files_routes::required_action`], where an unlisted
//! route answers 404 and the unknown direction is therefore the safe one; here
//! the unknown direction would be the unsafe one, so the table stays short and
//! every entry points at a route a reader can find in a dispatcher. Refusing
//! everything unlisted is not available: it would refuse the settings modal,
//! the auth projection and every read that a client needs to mount.
//!
//! ## Why reads are not in the table
//!
//! [`RequestAccess::decide`] with [`DocumentAction::View`] asks one question —
//! may this caller reach this document — and every request that gets as far as
//! the gate has already had it answered: the online accept loop takes the lease
//! on the owner's tenant through `TenantRegistry::lease_for_shared`, which is
//! the same check on the same two facts. A read classified `View` here could
//! only ever answer `Ok`. Writes are where the two questions differ, so writes
//! are what the table names.
//!
//! ## What this module deliberately does not cover
//!
//! `/api/files*` and `/api/recovery*` keep their own tables
//! ([`super::files_routes::required_action`] and
//! [`super::recovery_routes::required_action`]), which are finer than a
//! (method, path) pair can be: opening a stored document for viewing is a
//! `View` while saving it is an `Edit`, on the same path. Naming them here as
//! well would either duplicate those tables or contradict them.

use super::online_policy::ServeMode;
use super::request_access::{self, DocumentAction, RequestAccess};
use super::WebReply;

/// Which action a request asks of the document, or `None` when it asks nothing
/// of it.
///
/// `mode` is read for one reason only: the JSON-RPC tier has two spellings, and
/// which of them exists depends on the deployment
/// ([`ServeMode::allows_root_jsonrpc_alias`]). Without it, `POST /` on a public
/// deployment would be classified as a tool call and refused as a document
/// write, when the answer it must keep getting is the 405 the dispatcher has
/// always given it.
pub(super) fn required_action(
    method: &str,
    path: &str,
    body: &str,
    mode: ServeMode,
) -> Option<DocumentAction> {
    // The whole-document sync push, asked through the predicate the shared sync
    // wire owns (`mcp_serve::is_document_sync_route`) rather than by spelling
    // the path a second time, so this table and the live-MCP tier cannot drift
    // over which request carries a document.
    if crate::mcp_serve::is_document_sync_route(method, path) {
        return Some(DocumentAction::Edit);
    }
    if is_jsonrpc_route(method, path, mode) {
        // A tool call, not a handshake: which tools write is the catalog's own
        // classification (`tool_profile`), so a tool that starts writing is
        // covered the moment the table there says so.
        return crate::mcp_serve::message_writes_document(body).then_some(DocumentAction::Edit);
    }
    match (method, path) {
        // Replaces the open document with the daemon's starter (or with the
        // file the daemon was launched on) and bumps the version. Online the
        // mode table answers first and makes the route a no-op that touches
        // nothing, so this is the answer for the mode where it is not — and it
        // is deliberately the same answer either way, so that lifting the
        // no-op cannot quietly open the route.
        ("POST", "/api/mcp/sync-reset") => Some(DocumentAction::Edit),
        // The renderer's selection push. Not document *content*: it writes no
        // node, bumps no version, and a save does not record it as an edit. It
        // is still a write to the tenant's editor state — the selection and the
        // active page that the same daemon's MCP tools act on, that the owner's
        // own session reads back, and that the tenant store persists — so a
        // caller whose roles grant only reading must not be able to set it. A
        // `View` classification would be no check at all here (see the module
        // docs: reaching this point already proves the caller may reach the
        // document), which is why this one is an `Edit`.
        //
        // The MCP catalog already draws the line in the same place: the
        // `set_active_page` tool — the same state, reached the other way — is
        // classified `ToolAccess::Write` in `tool_profile`.
        ("POST", "/api/mcp/selection") => Some(DocumentAction::Edit),
        // The AI design turn: it classifies the prompt and, for a modify or a
        // new design, applies editor commands to this daemon's document and
        // bumps its version. Dispatched ahead of the REST handler, which is why
        // the gate is at the connection tier as well.
        ("POST", "/api/ai/standard") => Some(DocumentAction::Edit),
        // The collaboration panel, whole. `RequestUndo` applies an editor
        // command to this document here and now, and every other action feeds
        // the session that carries the peers' commands — including the two that
        // admit a peer. Which of them is "only" panel state is a property of a
        // list that grows, so the route is asked as one thing: a caller that may
        // watch a document is not a caller that may drive a session on it.
        //
        // Online this refuses a reach rather than a use — the relay is
        // unavailable there (`ServeMode::allows_relay_collaboration`), so the
        // panel stays a projection and its actions sit in a slot no driver
        // drains. Locally and under a supervisor the operator is the only
        // client, as always.
        //
        // `POST /api/collab/presence` is deliberately not here: a cursor is not
        // a command, and a viewer's cursor is what a session with a viewer in
        // it is made of.
        ("POST", "/api/collab/action") => Some(DocumentAction::Edit),
        // The three local-path routes: each installs a document into
        // `state.editor`. Online they are refused wholesale, before this gate
        // (the daemon host's filesystem is one filesystem for every account);
        // they are named here so that the answer does not depend on which of
        // the two refusals a deployment happens to reach first.
        ("POST", "/api/file/new" | "/api/file/save" | "/api/file/open-recent") => {
            Some(DocumentAction::Edit)
        }
        // Reads, settings, auth, static assets, sharing, export, collaboration
        // and the two tiers that own their own tables. Nothing to ask.
        _ => None,
    }
}

/// Whether this request is the JSON-RPC tool tier.
///
/// Mirrors the connection tier's own predicate, including the `/` alias, which
/// only the single-user daemons keep.
fn is_jsonrpc_route(method: &str, path: &str, mode: ServeMode) -> bool {
    method == "POST" && (path == "/mcp" || (path == "/" && mode.allows_root_jsonrpc_alias()))
}

/// The gate. `None` when the request may proceed with what it asked for.
///
/// Renders the refusal with [`request_access::refusal_reply`] — the daemon's one
/// coded-error body — so "this document is not yours" and "you may read it but
/// not change it" read the same here as they do on the file routes, whatever
/// tier the request arrived through.
pub(super) fn check(
    method: &str,
    path: &str,
    body: &str,
    access: &RequestAccess<'_>,
) -> Option<WebReply> {
    let action = required_action(method, path, body, access.mode())?;
    match access.decide(action) {
        Ok(()) => None,
        Err(refusal) => Some(request_access::refusal_reply(refusal)),
    }
}

#[cfg(test)]
#[path = "document_writes_tests.rs"]
mod tests;
