//! `/api/files*` — the browser's file list and document open/save.
//!
//! The browser has no filesystem, so the shell cannot offer "open a file"
//! the way the desktop does. These routes are that missing half: they list
//! what the daemon stores, create a document, open one by key, save the open
//! document back, rename and delete.
//!
//! Keys, not paths, are what crosses this boundary. A path in a URL would be
//! a filesystem oracle for anyone who can reach the daemon; a key is opaque
//! and validated (`document_store::key_is_valid`) before it resolves at all.

use super::*;
use crate::document_store::{self, DocumentStoreError};

/// One file-list row, as the browser sees it.
fn entry_json(entry: &document_store::DocumentEntry) -> serde_json::Value {
    serde_json::json!({
        "key": entry.key,
        "name": entry.name,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
        "size": entry.size,
    })
}

fn store_error_reply(error: DocumentStoreError) -> WebReply {
    let status = match error {
        DocumentStoreError::InvalidKey => "400 Bad Request",
        DocumentStoreError::NotFound => "404 Not Found",
        DocumentStoreError::CorruptIndex(_) | DocumentStoreError::Io(_) => "500 Internal Server Error",
    };
    WebReply {
        status,
        body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
    }
}

fn ok_json(value: serde_json::Value) -> WebReply {
    WebReply {
        status: "200 OK",
        body: value.to_string(),
    }
}

/// Split `/api/files/<key>/<action>` into its parts.
enum FilesRoute<'a> {
    List,
    Create,
    Document { key: &'a str, action: &'a str },
}

fn parse_route(path: &str) -> Option<FilesRoute<'_>> {
    let rest = path.strip_prefix("/api/files")?;
    // A path boundary, not a string prefix: `/api/filesx` is a different
    // route (the test below caught exactly that when this was `strip_prefix`
    // without the check).
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Some(FilesRoute::List);
    }
    let mut segments = rest.split('/');
    let key = segments.next().filter(|key| !key.is_empty())?;
    match (segments.next(), segments.next()) {
        // `/api/files/<key>` — the document itself.
        (None, None) => Some(FilesRoute::Document { key, action: "" }),
        (Some(action), None) => Some(FilesRoute::Document { key, action }),
        _ => None,
    }
}

/// Handle every `/api/files*` request.
pub(super) fn handle(method: &str, path: &str, body: &str, state: &mut WebCanvasState) -> WebReply {
    let Some(route) = parse_route(path) else {
        return not_found_reply();
    };
    let dir = document_store::documents_dir();
    match (method, route) {
        ("GET", FilesRoute::List) => match document_store::list(&dir) {
            Ok(entries) => ok_json(serde_json::json!({
                "ok": true,
                "files": entries.iter().map(entry_json).collect::<Vec<_>>(),
            })),
            Err(error) => store_error_reply(error),
        },
        ("POST", FilesRoute::List | FilesRoute::Create) => create_document(body, state, &dir),
        ("POST", FilesRoute::Document { key, action }) => match action {
            "open" => open_document(state, &dir, key),
            "save" => save_document(state, &dir, key),
            "rename" => rename_document(body, &dir, key),
            _ => not_found_reply(),
        },
        ("DELETE", FilesRoute::Document { key, action: "" }) => match document_store::delete(&dir, key)
        {
            Ok(()) => ok_json(serde_json::json!({ "ok": true })),
            Err(error) => store_error_reply(error),
        },
        _ => not_found_reply(),
    }
}

/// `POST /api/files` — a new document, stored, and opened in the daemon.
fn create_document(body: &str, state: &mut WebCanvasState, dir: &std::path::Path) -> WebReply {
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
    let name = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("name")
                .and_then(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        });
    // The starter is the same document File → New produces, so a document made
    // here is indistinguishable from one made in the editor.
    let mut next = op_pen_loader::new_skala_editor_state();
    super::preserve_web_canvas_preferences(&state.editor, &mut next);
    let created = document_store::create_with(dir, name.as_deref(), |path| {
        crate::doc_io::save_to_path(&next, path)
            .map_err(|error| DocumentStoreError::Io(format!("save {}: {error}", path.display())))
    });
    match created {
        Ok(entry) => {
            next.editor_ui.file_key = Some(entry.key.clone());
            next.editor_ui.file_name_display = Some(entry.name.clone());
            state.editor = next;
            state.current_path = None;
            state.version += 1;
            ok_json(serde_json::json!({
                "ok": true,
                "file": entry_json(&entry),
                "version": state.version,
            }))
        }
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/files/<key>/open` — load that document into the daemon.
fn open_document(state: &mut WebCanvasState, dir: &std::path::Path, key: &str) -> WebReply {
    if let Err(refusal) = state.gate_daemon_mutation(
        op_editor_core::CollabGateAction::ReplaceDocument,
        op_editor_core::CollabEditSource::ExternalSync,
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
    let path = match document_store::path_for(dir, key) {
        Ok(path) => path,
        Err(error) => return store_error_reply(error),
    };
    if !path.exists() {
        return store_error_reply(DocumentStoreError::NotFound);
    }
    match crate::mcp_serve::load_editor_state(&path) {
        Ok(mut next) => {
            super::preserve_web_canvas_preferences(&state.editor, &mut next);
            let name = document_store::list(dir)
                .ok()
                .and_then(|entries| entries.into_iter().find(|entry| entry.key == key))
                .map(|entry| entry.name);
            next.editor_ui.file_key = Some(key.to_string());
            next.editor_ui.file_name_display = name;
            state.editor = next;
            state.current_path = None;
            state.version += 1;
            // The name comes back with the open so the browser can title the
            // tab and the file list row without a second request.
            ok_json(serde_json::json!({
                "ok": true,
                "version": state.version,
                "name": name,
            }))
        }
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
        },
    }
}

/// `POST /api/files/<key>/save` — write the open document back to its file.
fn save_document(state: &mut WebCanvasState, dir: &std::path::Path, key: &str) -> WebReply {
    let path = match document_store::path_for(dir, key) {
        Ok(path) => path,
        Err(error) => return store_error_reply(error),
    };
    if let Err(error) = crate::doc_io::save_to_path(&state.editor, &path) {
        return WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
        };
    }
    // The write went through the same path desktop Save uses, so the
    // document is by definition in sync with its file now.
    state.editor.mark_saved_revision();
    match document_store::touch(dir, key) {
        Ok(entry) => ok_json(serde_json::json!({ "ok": true, "file": entry_json(&entry) })),
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/files/<key>/rename` — the key stays, so links keep working.
fn rename_document(body: &str, dir: &std::path::Path, key: &str) -> WebReply {
    let Some(name) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("name")
                .and_then(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
    else {
        return WebReply {
            status: "400 Bad Request",
            body: crate::mcp_serve::rest_error_body("Missing name string"),
        };
    };
    match document_store::rename(dir, key, &name) {
        Ok(entry) => ok_json(serde_json::json!({ "ok": true, "file": entry_json(&entry) })),
        Err(error) => store_error_reply(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_parser_accepts_the_shapes_the_client_sends() {
        assert!(matches!(parse_route("/api/files"), Some(FilesRoute::List)));
        assert!(matches!(
            parse_route("/api/files/"),
            Some(FilesRoute::List)
        ));
        match parse_route("/api/files/abcd1234") {
            Some(FilesRoute::Document { key, action }) => {
                assert_eq!(key, "abcd1234");
                assert_eq!(action, "");
            }
            other => panic!("expected a document route, got {}", other.is_some()),
        }
        match parse_route("/api/files/abcd1234/save") {
            Some(FilesRoute::Document { key, action }) => {
                assert_eq!(key, "abcd1234");
                assert_eq!(action, "save");
            }
            _ => panic!("expected an action route"),
        }
    }

    #[test]
    fn foreign_paths_are_not_ours() {
        for path in ["/api/file/new", "/api/filesx", "/api/files/a/b/c"] {
            assert!(parse_route(path).is_none(), "{path}");
        }
    }

    #[test]
    fn entries_serialize_with_camel_case_fields() {
        let entry = document_store::DocumentEntry {
            key: "k".into(),
            name: "n".into(),
            created_at: 1,
            updated_at: 2,
            size: 3,
        };
        let json = entry_json(&entry);
        assert_eq!(json["key"], "k");
        assert_eq!(json["createdAt"], 1);
        assert_eq!(json["updatedAt"], 2);
    }
}
