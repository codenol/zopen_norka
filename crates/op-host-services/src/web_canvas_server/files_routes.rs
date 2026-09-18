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
//!
//! ## Two questions, because a key is not an authorization
//!
//! [`RequestAccess::decide`] answers what a caller may DO with the document
//! this request is served against. A key names a row in a store that every
//! account of a deployment shares, so a second question has to be asked:
//! whether that document belongs to an account this caller may address
//! ([`RequestAccess::reaches_stored_document`]). The two are asked in that
//! order — right first, then ownership — and a refusal from either is the
//! daemon's standard coded 403.
//!
//! That pair is what replaced the wholesale refusal of this family in a shared
//! deployment (#20). Nothing here trusts the directory: the list asks for the
//! caller's own rows, a create records the creator as the owner, and every
//! per-key route looks the row up before it touches a file.
//!
//! ## The third question, asked by one route
//!
//! `POST /api/files/<key>/claim` is the exception, because the row it names has
//! no owner to ask a question about — it is the deliberate act that GIVES an
//! unattributed document an owner (#46), and it is what recovers the documents a
//! deployment inherited (files placed in the directory by hand, rows left behind
//! by a daemon that ran without accounts). Its authority is the deployment's
//! rather than a document's; `DocumentAction::Claim` says why, and
//! `document_store::claim` states the whole rule — create or claim, never open
//! or save.

use super::*;
use crate::document_db::DocumentDb;
use crate::document_store::{self, DocumentStoreError};

// The claim route and the preview routes, beside this file for the 800-line
// cap: the claim is the one route of this family with a question of its own, and
// the preview is a raster export rather than a rule about files.
use super::files_routes_claim::claim_document;
use super::files_routes_thumb::{refresh_thumbnail, thumbnail};

/// One file-list row, as the browser sees it.
pub(super) fn entry_json(entry: &document_store::DocumentEntry) -> serde_json::Value {
    serde_json::json!({
        "key": entry.key,
        "name": entry.name,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
        "size": entry.size,
        "hasThumbnail": entry.has_thumbnail,
    })
}

/// The reply for a store failure, in the one shape this family uses.
///
/// `pub(super)` because the conversation routes answer with it too
/// (`super::comment_routes`): to a client they are one family, and a status that
/// drifted between them would be a difference nobody asked for.
pub(super) fn store_error_reply(error: DocumentStoreError) -> WebReply {
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

/// The 200-with-a-JSON-object reply every route in this family answers with.
///
/// Shared with the conversation routes (`super::comment_routes`) rather than
/// copied there: the two are one route family to a client, and a status or an
/// envelope that drifted between them would be a difference no client asked
/// for. `pub(super)` for exactly that reason.
pub(super) fn ok_json(value: serde_json::Value) -> WebReply {
    WebReply {
        status: "200 OK",
        body: value.to_string(),
    }
}

/// Split `/api/files/<key>/<action>` into its parts.
///
/// There is no separate `Create`: creating a document IS `POST /api/files`,
/// which the parser answers with `List`, and the method is what tells the two
/// apart. A `Create` variant used to sit here — matched alongside `List` in the
/// gate and in the handler, but constructed by nothing, so it read as a second
/// accepted spelling that the parser never accepted.
enum FilesRoute<'a> {
    List,
    Document {
        key: &'a str,
        action: &'a str,
    },
    /// `/api/files/<key>/comments`, and `/api/files/<key>/comments/<id>/<action>`.
    ///
    /// Part of this family rather than a route family of its own because
    /// everything in front of a handler is a property of the KEY, not of the
    /// table behind it: the gate, the key's shape, the store, and the check that
    /// the document belongs to an account this caller may address. A second
    /// family prefix would have to repeat that preamble, and two copies of an
    /// authorization preamble is how the two drift.
    Comments {
        key: &'a str,
        /// The thread, as the client spelled it. Parsed where it is used (see
        /// `super::comment_routes`) rather than here, so this parser stays a
        /// statement about the SHAPE of a path.
        thread: Option<&'a str>,
        /// `""` for the collection itself, else `reply` / `resolve` / `reopen`.
        action: &'a str,
    },
    /// `/api/files/<key>/sections`, and `/api/files/<key>/sections/<node>`.
    ///
    /// What a section carries — the analytics it came from, what it says, and
    /// the flows drawn from it. Part of this family for the reason `Comments`
    /// is: a section is a frame in the document, so everything in front of its
    /// handler is a property of the key in the path and of nothing else.
    Sections {
        key: &'a str,
        /// The frame that marks the section, as the client spelled it. Parsed
        /// where it is used (`super::section_routes`), so this parser keeps
        /// saying only what the shape of a path is.
        node: Option<&'a str>,
    },
}

impl FilesRoute<'_> {
    /// The document key this route names, when it names one.
    ///
    /// What the caller does with it is the same for every variant: check the
    /// key's shape before the store is opened, and check that the row belongs to
    /// an account this caller may address. One accessor so a route added later
    /// cannot be the one that forgets the second half.
    fn key(&self) -> Option<&str> {
        match self {
            Self::List => None,
            Self::Document { key, .. }
            | Self::Comments { key, .. }
            | Self::Sections { key, .. } => Some(key),
        }
    }
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
    let Some(second) = segments.next() else {
        return Some(FilesRoute::Document { key, action: "" });
    };
    if second == "sections" {
        return match (segments.next(), segments.next()) {
            // `/api/files/<key>/sections` — every section that says something.
            (None, None) => Some(FilesRoute::Sections { key, node: None }),
            // `/api/files/<key>/sections/<node>`.
            (Some(node), None) => Some(FilesRoute::Sections {
                key,
                node: Some(node),
            }),
            // Deeper than a section is not a route: the properties are written
            // whole (see `super::section_routes` on why the route compares
            // rather than trusting a field list), so there is no per-field path
            // to parse.
            _ => None,
        };
    }
    if second != "comments" {
        return match segments.next() {
            // `/api/files/<key>` — the document itself.
            None => Some(FilesRoute::Document {
                key,
                action: second,
            }),
            // More than two segments after the key is not a route this family
            // has, and the parser is what says so.
            Some(_) => None,
        };
    }
    match (segments.next(), segments.next(), segments.next()) {
        // `/api/files/<key>/comments` — the document's conversation.
        (None, None, None) => Some(FilesRoute::Comments {
            key,
            thread: None,
            action: "",
        }),
        // `/api/files/<key>/comments/<thread>/<action>`.
        (Some(thread), Some(action), None) => Some(FilesRoute::Comments {
            key,
            thread: Some(thread),
            action,
        }),
        // A thread on its own is deliberately not a route: the list brings every
        // thread WITH its comments, so there is nothing a single-thread read
        // would answer that the list does not, and a shape the parser refuses
        // cannot become a handler somebody forgets to gate.
        _ => None,
    }
}

/// Which right each route asks for.
///
/// `None` means "no such route here", and the caller answers 404 — the gate
/// runs BEFORE the handler's own match, so a route arm added there without an
/// entry here is unreachable rather than unchecked. That is the fail-closed
/// direction: a forgotten line costs a route that stops working, never a route
/// that starts granting.
///
/// The table is per (method, route) because the right is a property of what the
/// request does, not of the path: `GET /api/files/<key>/thumb` reads and
/// `DELETE /api/files/<key>` removes, on the same key.
fn required_action(method: &str, route: &FilesRoute<'_>) -> Option<DocumentAction> {
    match (method, route) {
        ("GET", FilesRoute::List) => Some(DocumentAction::View),
        // Creating a document is a write, and `POST /api/files` is the spelling
        // the browser sends.
        ("POST", FilesRoute::List) => Some(DocumentAction::Edit),
        (
            "GET",
            FilesRoute::Document {
                action: "thumb", ..
            },
        ) => Some(DocumentAction::View),
        ("POST", FilesRoute::Document { action: "open", .. }) => Some(DocumentAction::View),
        (
            "POST",
            FilesRoute::Document {
                action: "save" | "autosave" | "rename",
                ..
            },
        ) => Some(DocumentAction::Edit),
        // A claim asks the DEPLOYMENT question rather than a document one — see
        // `DocumentAction::Claim` — and it is the one route in this family whose
        // per-key ownership check `handle` skips, for the reason stated there.
        (
            "POST",
            FilesRoute::Document {
                action: "claim", ..
            },
        ) => Some(DocumentAction::Claim),
        ("DELETE", FilesRoute::Document { action: "", .. }) => Some(DocumentAction::Delete),
        // Reading a document's conversation is reading the document, no more:
        // whoever may open it may see what is pinned to it.
        ("GET", FilesRoute::Comments { thread: None, .. }) => Some(DocumentAction::View),
        // Opening a thread is taking part in the conversation, and so is
        // replying to or closing one. `Comment` — not `Edit` — is the right the
        // five contributor roles hold, and `resolve`/`reopen` ask it here as
        // well: the finer question (is this thread YOURS, or may you edit the
        // document) is one only the thread can answer, and it is asked after the
        // thread has been read (`super::request_access::RequestAccess::
        // decide_thread_resolution`). Asking it here instead would decide from a
        // path that does not name an author.
        ("POST", FilesRoute::Comments { .. }) => Some(DocumentAction::Comment),
        // What a section carries asks for `View` — the floor a read has — even
        // when the request writes. That is not a weaker gate: the right to
        // change a summary or a flow is a question about the SUBJECT rather
        // than about the document (see `super::section_rights`), it can only be
        // answered against the properties actually being replaced, and this
        // table runs before a handler has read them. So the gate asks what it
        // can answer from a path — "may this caller reach this document at
        // all" — and `super::section_routes` asks the subject question with the
        // stored value in hand.
        (_, FilesRoute::Sections { .. }) => Some(DocumentAction::View),
        _ => None,
    }
}

/// Handle every `/api/files*` request.
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
    let Some(action) = required_action(method, &route) else {
        return not_found_reply();
    };
    // The one gate. It runs before anything in this file — before the key is
    // parsed, before the store is touched — so which decision a route makes is
    // readable from the table above instead of from its body.
    if let Err(refusal) = access.decide(action) {
        return request_access::refusal_reply(refusal);
    }
    // A key this store could never have issued is refused BEFORE the database
    // is opened or a directory is created: a pasted URL must not be able to
    // reach the filesystem, and it must not be able to make the daemon touch
    // one either.
    if let Some(key) = route.key() {
        if !document_store::key_is_valid(key) {
            return store_error_reply(DocumentStoreError::InvalidKey);
        }
    }
    // The store for this daemon's documents directory, opened on this request
    // and reused by every later one. Opening can fail (an unwritable volume, a
    // database another process has corrupted), and that is a 500 on the
    // document routes rather than a panic or a fall back to the file index.
    let store = match crate::document_db::local_store(&mut state.documents) {
        Ok(store) => store,
        Err(error) => return store_error_reply(error),
    };
    let dir = store.dir().to_path_buf();
    // Every route below names one document by key, and the key has to be
    // resolved before it is trusted: the store is one directory shared by every
    // account of a deployment, so "this key is well-formed" says nothing about
    // whose document it names. The gate above decided what the caller may DO;
    // this decides whether the document is theirs to do it to — for the
    // conversation routes as much as for the file's own: a thread is reached
    // through its document, and a caller who may not address the document must
    // not reach its comments either.
    //
    // The claim is the one exception, and it is not a hole in that rule: the row
    // it names has no owner BY DEFINITION, which is exactly the row this check
    // refuses — asking it here would refuse every claim there could ever be. Its
    // handler asks the narrower question instead, against the row as it reads it:
    // a document that belongs to an account is refused there, and only an
    // unattributed one is adopted.
    let claiming = matches!(
        route,
        FilesRoute::Document {
            action: "claim",
            ..
        }
    );
    if let Some(key) = route.key() {
        if !claiming {
            match document_store::find(&store, key) {
                Ok(Some(entry)) => {
                    if !access.reaches_stored_document(entry.owner_id.as_deref()) {
                        // The same code a stranger gets from the lease, because
                        // it is the same statement about the same document.
                        return request_access::refusal_reply(AccessRefusal::NotShared);
                    }
                }
                // No row carries the key. Locally that is not the end of it —
                // the store creates the row on the next save, which is how an
                // `.op` dropped into the operator's directory becomes a
                // document. Online it IS the end: a caller naming a key is not a
                // caller creating a document, nothing would attribute the row to
                // anyone, and letting the save through would write an ownerless
                // file that nobody — including whoever wrote it — could ever
                // list or reach again. Creating a document online goes through
                // `POST /api/files`, which records its owner, and recovering one
                // that arrived another way goes through the claim route below.
                Ok(None) if access.mode().is_online() => {
                    return store_error_reply(DocumentStoreError::NotFound)
                }
                Ok(None) => {}
                Err(error) => return store_error_reply(error),
            }
        }
    }
    match (method, route) {
        ("GET", FilesRoute::List) => {
            // Whose documents this list is about. Online, the caller's own: one
            // directory holds every account's files, and a list hands every row
            // over whole — name, size and timestamps included. A document
            // shared WITH the caller is deliberately absent from it; what names
            // such a document is the key its link carried, which is how it is
            // opened. Locally there are no accounts and the directory is the
            // operator's, so every row is theirs to see.
            let listed = match access.caller_id() {
                Some(caller) => document_store::list_owned_by(&store, caller),
                None => document_store::list(&store),
            };
            match listed {
                Ok(entries) => ok_json(serde_json::json!({
                    "ok": true,
                    "files": entries.iter().map(entry_json).collect::<Vec<_>>(),
                })),
                Err(error) => store_error_reply(error),
            }
        }
        ("POST", FilesRoute::List) => create_document(body, state, &store, access),
        (
            "GET",
            FilesRoute::Document {
                key,
                action: "thumb",
            },
        ) => thumbnail(&dir, key),
        ("POST", FilesRoute::Document { key, action }) => match action {
            "open" => open_document(state, &store, key, access),
            "save" => save_document(body, state, &store, key, WriteKind::Explicit),
            // Autosave is the same write without the preview render: the
            // thumbnail is a 480 px rasterization, and paying for it every few
            // seconds is what would make autosave expensive enough to disable.
            "autosave" => save_document(body, state, &store, key, WriteKind::Quiet),
            "rename" => rename_document(body, &store, key),
            // The one route here that can turn a document nobody owns into one
            // an account owns (#46). Every other write in this family needs the
            // row to belong to the caller already; this is how it comes to.
            "claim" => claim_document(&store, key, access),
            _ => not_found_reply(),
        },
        ("DELETE", FilesRoute::Document { key, action: "" }) => {
            match document_store::delete(&store, key) {
                Ok(()) => {
                    // The preview belongs to the document: leaving it behind
                    // would keep a picture of a file the user deleted.
                    if let Ok(path) = document_store::thumb_path(&dir, key) {
                        let _ = std::fs::remove_file(path);
                    }
                    ok_json(serde_json::json!({ "ok": true }))
                }
                Err(error) => store_error_reply(error),
            }
        }
        // The conversation about the document. Its handlers live next door to
        // keep this file about the file, and they are reached only from here —
        // after the gate, the key's shape and the row's owner, which is the
        // whole reason these routes were written as part of this family.
        //
        // Note what is deliberately NOT here: the document's version, its
        // editor state, its file and its row. A comment route answers about a
        // conversation; a comment is not part of the document's content and
        // must not move its version, which is the one thing these routes have
        // in common with the reads above them.
        (
            _,
            FilesRoute::Comments {
                key,
                thread,
                action,
            },
        ) => super::comment_routes::handle(method, key, thread, action, body, &store, access),
        // What a section carries. Its handlers live next door for the same
        // reason the conversation's do — this file stays about the file — and
        // they are reached only from here, after the gate, the key's shape and
        // the row's owner. The subject question (may this caller change THIS
        // subject) is asked there, because only a handler can read what is
        // being replaced.
        (_, FilesRoute::Sections { key, node }) => {
            super::section_routes::handle(method, key, node, body, &store, access)
        }
        _ => not_found_reply(),
    }
}

/// `POST /api/files` — a new document, stored, and opened in the daemon.
///
/// The creator owns what it makes. The account id is read from the verified
/// identity and never from the body: a body a caller can write is not a
/// statement about who the caller is.
fn create_document(
    body: &str,
    state: &mut WebCanvasState,
    store: &DocumentDb,
    access: &RequestAccess<'_>,
) -> WebReply {
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
    // `None` locally: the offline daemon has no accounts, so its rows are
    // ownerless and every one of them is in the operator's own list.
    let owner = access.caller_id();
    // The starter is the same document File → New produces, so a document made
    // here is indistinguishable from one made in the editor.
    let mut next = op_pen_loader::new_skala_editor_state();
    super::preserve_web_canvas_preferences(&state.editor, &mut next);
    let created = document_store::create_with(store, name.as_deref(), owner, |path| {
        crate::doc_io::save_to_path(&next, path)
            .map_err(|error| DocumentStoreError::Io(format!("save {}: {error}", path.display())))
    });
    match created {
        Ok(entry) => {
            next.editor_ui.file_key = Some(entry.key.clone());
            next.editor_ui.file_name_display = Some(entry.name.clone());
            state.adopt_document(next);
            state.current_path = None;
            refresh_thumbnail(state, store, &entry.key);
            remember_last_opened(store, &entry.key, owner);
            ok_json(serde_json::json!({
                "ok": true,
                "file": entry_json(&entry),
                "version": state.version,
            }))
        }
        Err(error) => store_error_reply(error),
    }
}

/// Record the document a restart should come back to — for the local operator
/// only.
///
/// The pointer is ONE row per daemon, keyed by `LOCAL_OWNER` (the empty
/// string), because the daemon holds one document at a time and the operator
/// has no account id to key it by. In a shared deployment that would be a row
/// every account overwrote, holding a document most of them may not open — and
/// nothing reads it there anyway: an online tenant always starts from the
/// starter document (`WebCanvasState::new_for_tenant`), and the one reader,
/// `serve_options::restore_last_document`, is the local daemon's start-up. So
/// the pointer is written only when there IS a local operator.
fn remember_last_opened(store: &DocumentDb, key: &str, owner: Option<&str>) {
    if owner.is_none() {
        let _ = document_store::remember_last(store, key);
    }
}

/// `POST /api/files/<key>/open` — load that document into the daemon.
fn open_document(
    state: &mut WebCanvasState,
    store: &DocumentDb,
    key: &str,
    access: &RequestAccess<'_>,
) -> WebReply {
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
    let path = match document_store::path_for(store.dir(), key) {
        Ok(path) => path,
        Err(error) => return store_error_reply(error),
    };
    if !path.exists() {
        return store_error_reply(DocumentStoreError::NotFound);
    }
    match crate::mcp_serve::load_editor_state(&path) {
        Ok(mut next) => {
            super::preserve_web_canvas_preferences(&state.editor, &mut next);
            // The row, not the store's whole list: the name of the one document
            // this caller is opening is the only name this answer may carry.
            let name = document_store::find(store, key)
                .ok()
                .flatten()
                .map(|entry| entry.name);
            next.editor_ui.file_key = Some(key.to_string());
            next.editor_ui.file_name_display = name.clone();
            // A restart should come back to this document, not to the kit.
            remember_last_opened(store, key, access.caller_id());
            state.adopt_document(next);
            state.current_path = None;
            // The name comes back with the open so the browser can title the
            // tab and the file list row without a second request.
            ok_json(serde_json::json!({
                "ok": true,
                "version": state.version,
                "name": name,
                // Whether THIS caller may write the document. The browser used
                // to find out by being refused, one wasted push at a time: a
                // visitor reading a shared document pushed the whole document
                // on every edit and the selection on every click, and every one
                // of those requests was refused (issue #43). One additive field
                // saves all of them.
                "canWrite": access.decide(super::request_access::DocumentAction::Edit).is_ok(),
            }))
        }
        Err(error) => WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error.to_string() }).to_string(),
        },
    }
}

/// Whether a write also refreshes the stored preview.
///
/// `pub(super)` because the stale-autosave decision is made in
/// [`super::files_routes_autosave_guard`], beside this file for the 800-line cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WriteKind {
    /// A user asked: the preview is part of what they expect to see.
    Explicit,
    /// Autosave: the document only.
    Quiet,
}

/// `POST /api/files/<key>/save` — write the document back to its file.
///
/// A body wins over the daemon's own state when one is present. The browser
/// holds the document the user is editing; the daemon's copy arrives over the
/// sync channel, which is size-limited, so for a large document the daemon's
/// state can be a stale echo. Saving what was sent is what makes "Save" mean
/// "what I see".
fn save_document(
    body: &str,
    state: &mut WebCanvasState,
    store: &DocumentDb,
    key: &str,
    kind: WriteKind,
) -> WebReply {
    // A save installs the document it wrote, which is a whole-document swap —
    // the same gate the local-path save clears. Without it a guest in a shared
    // session could write the document through this route while the same write
    // through `/api/file/save` would be refused.
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
    let path = match document_store::path_for(store.dir(), key) {
        Ok(path) => path,
        Err(error) => return store_error_reply(error),
    };
    let body = body.trim();
    // An autosave from a tab that never took the document the daemon drew is an
    // older copy, and adopting it would put that copy over the screen, in the
    // file and in memory (issues #247/#248). An explicit Save is not subject to
    // this: "save" means "what I see", the operator's call (#169).
    if let Some(refusal) =
        super::files_routes_autosave_guard::stale_autosave_refusal(state, kind, body)
    {
        return refusal;
    }
    // One error type for both branches: the caller only needs to know the
    // write failed and why.
    let saved: std::result::Result<(), String> = if body.is_empty() || body == "{}" {
        crate::doc_io::save_to_path(&state.editor, &path).map_err(|error| error.to_string())
    } else {
        // The body is the browser's document; it is written to the file and
        // adopted, so the daemon and the disk agree afterwards. Taken: this
        // write carried the turn's own nodes, so the guard comes down with it.
        super::save_editor_from_body(body, &state.editor, &path)
            .map(|next| state.adopt_document(next))
            .map_err(|error| error.to_string())
    };
    if let Err(error) = saved {
        return WebReply {
            status: "500 Internal Server Error",
            body: serde_json::json!({ "ok": false, "error": error }).to_string(),
        };
    }
    state.editor.mark_saved_revision();
    if kind == WriteKind::Explicit {
        refresh_thumbnail(state, store, key);
    }
    match document_store::touch(store, key) {
        Ok(entry) => ok_json(serde_json::json!({
            "ok": true,
            "file": entry_json(&entry),
            "version": state.version,
        })),
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/files/<key>/rename` — the key stays, so links keep working.
fn rename_document(body: &str, store: &DocumentDb, key: &str) -> WebReply {
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
    match document_store::rename(store, key, &name) {
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
        assert!(matches!(parse_route("/api/files/"), Some(FilesRoute::List)));
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
            owner_id: Some("userA".into()),
            created_at: 1,
            updated_at: 2,
            size: 3,
            has_thumbnail: false,
        };
        let json = entry_json(&entry);
        assert_eq!(json["key"], "k");
        assert_eq!(json["createdAt"], 1);
        assert_eq!(json["updatedAt"], 2);
        // The owner is the daemon's business: it is the account id the identity
        // verifier issued, and the browser has its own account projection.
        assert!(
            json.get("ownerId").is_none(),
            "the file list must not carry another account's id: {json}"
        );
    }
}

/// The access gate on these routes: the sibling module keeps this file's own
/// tests about parsing and serialization, and puts every caller-shaped test —
/// who may list, open, save, rename or delete — beside the table that decides
/// it.
#[cfg(test)]
#[path = "files_routes_access_tests.rs"]
mod access_tests;

/// The routes against a real document database: what the gate admits has to
/// reach the rows, since `access_tests` deliberately stops short of them.
#[cfg(test)]
#[path = "files_routes_store_tests.rs"]
mod store_tests;
