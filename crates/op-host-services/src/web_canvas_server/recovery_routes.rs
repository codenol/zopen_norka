//! `/api/recovery` — the draft a document with no home writes itself into.
//!
//! Autosave needs somewhere to put a document that has no server key and no
//! path: an untitled screen the user has been working on. Writing a file the
//! user never asked for would be wrong, and losing the work is worse — so the
//! daemon keeps exactly one draft slot beside its documents and offers it back
//! with a banner on the next launch (the decision recorded in issue #16).
//!
//! The draft is deliberately not a document in the store: it has no key, it is
//! never listed, and it is dropped the moment it is restored or refused.

use super::*;
use crate::document_store::{self, DocumentStoreError};

/// Split `/api/recovery[/(restore)]` into its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryRoute {
    /// The draft itself: read its state, write it, or drop it.
    Draft,
    /// Adopt the draft as the open document.
    Restore,
}

fn parse_route(path: &str) -> Option<RecoveryRoute> {
    let rest = path.strip_prefix("/api/recovery")?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    match rest {
        "" | "/" => Some(RecoveryRoute::Draft),
        "/restore" => Some(RecoveryRoute::Restore),
        _ => None,
    }
}

/// Which right each route asks for.
///
/// `None` means "no such route here" — see `files_routes::required_action` for
/// why the gate is a table in front of the handler rather than a check inside
/// each arm.
///
/// Reading the draft's metadata is a read; writing it is a write, even though
/// the draft is not a document in the store — it is the caller's own unsaved
/// work going to disk on the daemon's volume, and a caller whose roles do not
/// grant an edit must not be able to leave bytes there.
fn required_action(method: &str, route: RecoveryRoute) -> Option<DocumentAction> {
    match (method, route) {
        ("GET", RecoveryRoute::Draft) => Some(DocumentAction::View),
        ("POST", RecoveryRoute::Draft) => Some(DocumentAction::Edit),
        ("POST", RecoveryRoute::Restore) => Some(DocumentAction::Restore),
        ("DELETE", RecoveryRoute::Draft) => Some(DocumentAction::Delete),
        _ => None,
    }
}

pub(super) fn handle(
    method: &str,
    path: &str,
    body: &str,
    state: &mut WebCanvasState,
    access: &RequestAccess<'_>,
) -> WebReply {
    let Some(route) = parse_route(path) else {
        return not_found_reply();
    };
    let Some(action) = required_action(method, route) else {
        return not_found_reply();
    };
    // The one gate, before the slot is read, written or dropped.
    if let Err(refusal) = access.decide(action) {
        return request_access::refusal_reply(refusal);
    }
    let dir = document_store::documents_dir();
    match (method, route) {
        ("GET", RecoveryRoute::Draft) => match document_store::recovery_info(&dir) {
            Some(info) => WebReply {
                status: "200 OK",
                body: serde_json::json!({
                    "ok": true,
                    "exists": true,
                    "savedAt": info.saved_at,
                    "size": info.size,
                })
                .to_string(),
            },
            None => WebReply {
                status: "200 OK",
                body: serde_json::json!({ "ok": true, "exists": false }).to_string(),
            },
        },
        ("POST", RecoveryRoute::Draft) => write_draft(body, state, &dir),
        ("POST", RecoveryRoute::Restore) => restore_draft(state, &dir),
        ("DELETE", RecoveryRoute::Draft) => match document_store::clear_recovery(&dir) {
            Ok(()) => WebReply {
                status: "200 OK",
                body: serde_json::json!({ "ok": true }).to_string(),
            },
            Err(error) => store_error(error),
        },
        _ => not_found_reply(),
    }
}

/// Write the draft from a request body.
fn write_draft(body: &str, state: &WebCanvasState, dir: &std::path::Path) -> WebReply {
    let path = document_store::recovery_path(dir);
    match super::save_editor_from_body(body, &state.editor, &path) {
        Ok(_) => WebReply {
            status: "200 OK",
            body: serde_json::json!({ "ok": true }).to_string(),
        },
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
        },
    }
}

/// Adopt the draft as the open document, then drop it.
///
/// The draft exists to be handed back; keeping it after restoring would offer
/// the same recovery again on every launch.
fn restore_draft(state: &mut WebCanvasState, dir: &std::path::Path) -> WebReply {
    if let Err(refusal) = state.gate_daemon_mutation(
        op_editor_core::CollabGateAction::ReplaceDocument,
        op_editor_core::CollabEditSource::User,
    ) {
        return WebReply {
            status: refusal.http_status(),
            body: serde_json::json!({
                "ok": false,
                "error": refusal.code(),
                "message": refusal.to_string(),
            })
            .to_string(),
        };
    }
    let path = document_store::recovery_path(dir);
    if !path.exists() {
        return store_error(DocumentStoreError::NotFound);
    }
    match crate::mcp_serve::load_editor_state(&path) {
        Ok(mut next) => {
            super::preserve_web_canvas_preferences(&state.editor, &mut next);
            // The recovered document has no key: it is exactly as unbound as it
            // was when the draft was written.
            next.editor_ui.file_key = None;
            next.mark_saved_revision();
            state.editor = next;
            state.current_path = None;
            state.version += 1;
            let _ = document_store::clear_recovery(dir);
            WebReply {
                status: "200 OK",
                body: serde_json::json!({ "ok": true, "version": state.version }).to_string(),
            }
        }
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
        },
    }
}

fn store_error(error: DocumentStoreError) -> WebReply {
    let status = match error {
        DocumentStoreError::InvalidKey => "400 Bad Request",
        DocumentStoreError::NotFound => "404 Not Found",
        DocumentStoreError::Database(_) | DocumentStoreError::Io(_) => "500 Internal Server Error",
    };
    WebReply {
        status,
        body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_parser_accepts_the_shapes_the_client_sends() {
        assert_eq!(parse_route("/api/recovery"), Some(RecoveryRoute::Draft));
        assert_eq!(parse_route("/api/recovery/"), Some(RecoveryRoute::Draft));
        assert_eq!(
            parse_route("/api/recovery/restore"),
            Some(RecoveryRoute::Restore)
        );
    }

    #[test]
    fn foreign_paths_are_not_ours() {
        for path in ["/api/recoveryx", "/api/recovery/a/b", "/api/other"] {
            assert_eq!(parse_route(path), None, "{path}");
        }
    }
}

/// The access gate on these routes — who may read the offer, write the draft,
/// restore it or drop it. See the sibling's module docs for why it lives
/// beside the file rather than inside it.
#[cfg(test)]
#[path = "recovery_routes_access_tests.rs"]
mod access_tests;
