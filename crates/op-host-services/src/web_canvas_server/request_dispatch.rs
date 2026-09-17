//! The request dispatch table: one arm per route the daemon answers itself.
//!
//! A child of the `web_canvas_server` spine for the 800-line cap, beside the
//! route modules it dispatches to. A child rather than a module of its own
//! because the table reads the private modules and helpers that spine
//! declares. Everything below is the original `web_canvas_server.rs` lines
//! 454-786, byte-for-byte; `not_found_reply` stayed in the spine beside
//! `WebReply` because `files_routes_claim` reaches it through `use super::*`.

use super::connect_routes::update_mcp_server_settings;
use super::doc_routes::{apply_selection_sync, new_untitled_file, open_recent_file};
use super::document_push::collab_aware_error_reply;
use super::export_routes::{export_pdf_download, export_raster_download, save_current_file};
use super::not_found_reply;
use super::{
    analytics_routes, collab_routes, document_writes, files_routes, online_policy, recovery_routes,
    tenant_auth, workspace_settings, RequestAccess, WebCanvasState, WebReply,
};

/// Handle one parsed web-canvas REST request against the in-memory state. Pure
/// w.r.t. IO — fully unit-testable without a socket. Mirrors the TS Nitro
/// routes:
/// - `GET  /api/mcp/server`   → health `{ok:true,…}` (like `server.get.ts`)
/// - `GET  /api/mcp/document` → `{document:<doc>,version}` (like `document.get.ts`)
/// - `POST /api/mcp/document` → whole-doc replace → `{ok:true,version}` (like
///   `document.post.ts`); an optional top-level `baseVersion` makes the write
///   conditional — a stale value 409s with `{ok:false,error:"version-conflict",
///   version}` and leaves the document untouched instead of clobbering it
/// - `GET  /api/mcp/version`  → `{version}` — Rust-only cheap change probe; the
///   TS stack pushes documents over SSE instead, so it never needs one. The
///   browser shell polls this and fetches the full document only on a bump.
/// - `GET  /api/mcp/selection` → `{selectedIds,activePageId}` (like `selection.get.ts`)
/// - `POST /api/mcp/selection` → renderer selection push (like `selection.post.ts`)
/// - `POST /api/file/save` → write the browser document to this daemon's
///   backing local path, including embedded active-page metadata
///   (and legacy `.opmeta` fallback compatibility)
/// - `POST /api/file/open-recent` → local-daemon recent-file open, used by the
///   browser shell because only the daemon can read local paths.
/// - `POST /api/file/new` → untitled starter + Skala library, unbound path
/// - anything else → 404 (the JSON-RPC `/mcp` path + SSE are handled by the
///   caller's connection loop, not here).
///
/// `access` is who is asking — see [`RequestAccess`]. Only the route tiers that
/// can refuse a caller take it; the rest ignore it.
pub fn handle_web_canvas_request(
    method: &str,
    path: &str,
    body: &str,
    state: &mut WebCanvasState,
    access: &RequestAccess<'_>,
) -> WebReply {
    // Who may change what, ahead of the `match` rather than inside the arms
    // that need it: the document first (`document_writes`), then this account's
    // own configuration (`workspace_settings`) — the same shape one right
    // apart. The connection tier asks the document question again for the
    // routes it dispatches itself (the pre-parsed push, JSON-RPC, `/api/ai/*`).
    if let Some(refusal) = document_writes::check(method, path, body, access)
        .or_else(|| workspace_settings::check(method, path, access))
    {
        return refusal;
    }
    match (method, path) {
        ("GET", "/api/mcp/server") => WebReply {
            status: "200 OK",
            // `{running,port,localIp}` matches TS `server.get.ts`; the daemon
            // binds 127.0.0.1 (localhost-only) so localIp is loopback. Extra
            // `server`/`mode` fields are additive diagnostics.
            // `serveMode` is additive: the browser reads it to learn whether
            // the daemon is the sole sequencer for this document (online) or
            // merely a peer holding the operator's file (local/managed). That
            // decides whether a sync conflict may be auto-resolved — see
            // `op-host-web/src/live_sync_glue.rs::auto_resolve_is_safe`.
            body: format!(
                r#"{{"running":true,"port":{},"localIp":"127.0.0.1","server":"openpencil-mcp","mode":"web-canvas","serveMode":"{}"}}"#,
                state.port,
                state.mode.wire_name()
            ),
        },
        ("POST", "/api/mcp/server") => update_mcp_server_settings(body, state),
        // Taking the document is what catches a tab up with a turn the daemon
        // ran on its own, so the guard that keeps a stale autosave off it (issue
        // #247) comes down here.
        ("GET", "/api/mcp/document") => {
            state.daemon_document_ahead = false;
            match serde_json::to_string(&state.editor.doc) {
                Ok(doc_json) => WebReply {
                    status: "200 OK",
                    body: format!(
                        // `fileKey` is the STORE's name for this document, when the
                        // daemon is holding one it took from the store. A tab on
                        // `/` otherwise has no way to learn it: its Save then takes
                        // the key-less route, the daemon refuses (no bound path),
                        // and the client silently downloads the document instead of
                        // saving it — issue #97.
                        r#"{{"document":{doc_json},"version":{},"activePageIndex":{},"preserveAuthoredGeometry":{},"scenario":{},"fileKey":{}}}"#,
                        state.version,
                        state.editor.ui.active_page_index,
                        state.editor.editor_ui.preserve_authored_geometry,
                        // The scene tag rides the same wire as the rest of the
                        // editor meta: a Slides deck must reach the browser as a
                        // deck or web preview cannot enter its presentation.
                        match state.editor.editor_ui.scenario {
                            Some(scene) => format!("\"{}\"", scene.as_str()),
                            None => "null".to_string(),
                        },
                        match state.editor.editor_ui.file_key.as_deref() {
                            Some(key) => format!("\"{key}\""),
                            None => "null".to_string(),
                        }
                    ),
                },
                Err(e) => WebReply {
                    status: "500 Internal Server Error",
                    body: crate::mcp_serve::rest_error_body(&e.to_string()),
                },
            }
        }
        ("POST", "/api/mcp/document") => match state.apply_document_push(body, None) {
            Ok(outcome) if outcome.applied => WebReply {
                status: "200 OK",
                body: crate::mcp_serve::document_sync_ok(outcome.current_version),
            },
            Ok(outcome) => WebReply {
                // Stale baseVersion: reject without writing, TS-style error
                // envelope plus the current version so the caller can decide
                // whether to refetch and retry.
                status: "409 Conflict",
                body: serde_json::json!({
                    "ok": false,
                    "error": "version-conflict",
                    "version": outcome.current_version,
                })
                .to_string(),
            },
            Err(error) => collab_aware_error_reply(&error),
        },
        ("GET", "/api/mcp/version") => WebReply {
            // `collabSeq` rides along so the existing 400 ms version poll also
            // notices collaboration changes without a second request. Adding a
            // field is backward compatible: a client that reads only `version`
            // is unaffected.
            status: "200 OK",
            body: format!(
                r#"{{"version":{},"collabSeq":{}}}"#,
                state.version,
                state.collab.seq()
            ),
        },
        ("GET", "/api/mcp/indicators") => WebReply {
            // Agent-indicator relay: design runs execute inside this
            // daemon, so the process-global registry the canvas paints
            // from lives HERE — the browser polls this and mirrors it
            // into its own registry (agent_indicators::apply_remote) so
            // agent borders / badges / reveal animations show on web.
            //
            // That registry has no tenant dimension, so a shared deployment
            // relays the empty projection instead of showing one account the
            // shape of another account's design run.
            status: "200 OK",
            body: if state.mode.allows_agent_indicator_relay() {
                op_editor_core::agent_indicators::relay_json()
            } else {
                online_policy::EMPTY_INDICATOR_RELAY.to_string()
            },
        },
        // The wasm shell posts a sync-reset on every mount. Locally that
        // means "the browser just booted, drop the transient document";
        // online it would wipe the document the returning account left
        // behind, so the route answers with the already-reset shape — the
        // same body a second local reset produces — and touches nothing.
        ("POST", "/api/mcp/sync-reset") if state.mode.sync_reset_is_noop() => WebReply {
            status: "200 OK",
            body: format!(
                r#"{{"ok":true,"skipped":true,"version":{}}}"#,
                state.version
            ),
        },
        ("POST", "/api/mcp/sync-reset") => match state.reset_document_guarded() {
            Ok(outcome) if outcome.skipped => WebReply {
                status: "200 OK",
                body: format!(
                    r#"{{"ok":true,"skipped":true,"version":{}}}"#,
                    state.version
                ),
            },
            Ok(_) => WebReply {
                status: "200 OK",
                body: crate::mcp_serve::document_sync_ok(state.version),
            },
            Err(e) if e.error_code().is_some() => collab_aware_error_reply(&e),
            Err(e) => WebReply {
                status: e.http_status(),
                body: crate::mcp_serve::rest_error_body(&format!("sync reset failed: {e}")),
            },
        },
        ("GET", "/api/mcp/selection") => {
            // TS `selection.get.ts` → `getSyncSelection()` shape:
            // `{selectedIds, activePageId}`. Read straight off the live
            // editor selection so MCP clients and the REST route agree.
            let ids: Vec<&str> = state
                .editor
                .selection
                .set
                .iter()
                .map(|id| id.as_str())
                .collect();
            let active_page_id = state
                .editor
                .doc
                .pages
                .as_ref()
                .and_then(|pages| pages.get(state.editor.ui.active_page_index))
                .map(|page| page.id.clone());
            let body = serde_json::json!({
                "selectedIds": ids,
                "activePageId": active_page_id,
            });
            WebReply {
                status: "200 OK",
                body: serde_json::to_string(&body)
                    .unwrap_or_else(|_| r#"{"selectedIds":[],"activePageId":null}"#.to_string()),
            }
        }
        ("POST", "/api/mcp/selection") => apply_selection_sync(body, state),
        // Both file routes touch the daemon host's filesystem, which in a
        // shared process means every account reading and writing through the
        // service account's paths. Refused before the handler, not inside it.
        ("POST", "/api/file/save" | "/api/file/open-recent")
            if !state.mode.allows_local_file_routes() =>
        {
            online_policy::refusal_reply(online_policy::OnlineRouteRefusal::LocalFileAccess)
        }
        // A save reply REPLACES `state.editor` (it installs the document it
        // just wrote, plus its metadata), so it is a whole-document swap and
        // has to clear the same gate `open-recent` does. Swapping the document
        // out from under a live session would leave the peers editing a
        // document this daemon no longer has.
        ("POST", "/api/file/save") => match state.gate_daemon_mutation(
            op_editor_core::CollabGateAction::ReplaceDocument,
            op_editor_core::CollabEditSource::ExternalSync,
        ) {
            Ok(()) => save_current_file(body, state),
            Err(refusal) => WebReply {
                status: refusal.http_status(),
                body: serde_json::json!({
                    "ok": false,
                    "error": refusal.code(),
                    "message": refusal.to_string(),
                })
                .to_string(),
            },
        },
        ("POST", "/api/file/open-recent") => open_recent_file(body, state),
        ("POST", "/api/file/new") => new_untitled_file(state),
        ("POST", "/api/export/pdf") => export_pdf_download(body, state),
        ("POST", "/api/export/raster") => export_raster_download(body, state),
        ("GET", "/api/ai/models") => WebReply {
            // JSON array of model ids the AI proxy can serve (the
            // configured built-in agents). The web bundle queries this
            // to populate its model picker without bundling a static
            // list or holding API keys. `POST /api/ai/stream` is a
            // streaming route handled in the connection loop, not here.
            status: "200 OK",
            body: crate::ai_proxy::models_json(&state.editor),
        },
        ("GET", "/api/settings/credential-policy") => WebReply {
            status: "200 OK",
            body: serde_json::json!({
                "serverPersistence": state.credential_persistence.server_persistence(),
            })
            .to_string(),
        },
        ("POST", "/api/settings/credentials") => {
            if !state.credential_persistence.server_persistence() {
                WebReply {
                    status: "403 Forbidden",
                    body: crate::mcp_serve::rest_error_body(
                        "server credential persistence is disabled",
                    ),
                }
            } else {
                match crate::web_credentials::apply_json(&mut state.editor, body) {
                    Ok(()) => WebReply {
                        status: "200 OK",
                        body: r#"{"ok":true}"#.into(),
                    },
                    // The oversize verdict now rides on the error itself
                    // (`is_payload_too_large`) instead of this route
                    // re-measuring the body against the cap — same 413/400
                    // split, one owner for the threshold.
                    Err(error) => WebReply {
                        status: if error.is_payload_too_large() {
                            "413 Payload Too Large"
                        } else {
                            "400 Bad Request"
                        },
                        body: crate::mcp_serve::rest_error_body(&error.to_string()),
                    },
                }
            }
        }
        // The account routes are NOT served here. They live in the online
        // accept loop's own anonymous prefix (`account_routes`), ahead of
        // identity resolution, because sign-in is the one request that must
        // work without a credential. What reaches this table is a daemon with
        // no account store at all — `--serve-web` and `--serve-web --managed`,
        // where the operator IS the deployment — and the two answers below are
        // the honest ones: this deployment has no accounts, and there is
        // nothing here to sign in to.
        //
        // The device-login proxy that used to live on `/api/auth/*` is gone
        // with the identity service it proxied. Its routes are simply not
        // found now, which is also what an online deployment answered for them
        // before — but for a different reason: online refused them because one
        // process-wide device session must not sign in every visitor, and here
        // they do not exist.
        ("GET", op_editor_core::auth_routes::STATUS) => WebReply {
            status: "200 OK",
            body: tenant_auth::anonymous_auth_status_json(false, false),
        },
        (_, path) if path.starts_with(op_editor_core::auth_routes::API_PREFIX) => not_found_reply(),
        // Collaboration: the runtime lives in this daemon, the panel in the
        // browser. See `web_canvas_server/collab_routes.rs`.
        ("GET", op_editor_core::collab_routes::STATE) => collab_routes::state(state),
        ("POST", op_editor_core::collab_routes::ACTION) => collab_routes::action(body, state),
        ("POST", op_editor_core::collab_routes::PRESENCE) => collab_routes::presence(body, state),
        // Every stored-document route reads and writes a directory that belongs
        // to the daemon process rather than to the caller. That used to be a
        // reason to refuse the whole family in a shared deployment (#20): with
        // no owner on a row, a role check could say who may edit without ever
        // saying whose file it is.
        //
        // The owner column answers it now. A create records the verified
        // caller as the owner, the list asks for that account's rows, and every
        // per-key route resolves the row and asks
        // `RequestAccess::reaches_stored_document` before it touches a file —
        // so the same shared directory serves every account without becoming a
        // way to read another one's documents. The draft slot is keyed per
        // workspace for the same reason (`document_store::recovery_path`).
        //
        // `/api/files*` carries a key in the path, so it is matched by prefix
        // rather than by the exact-path arms above (§ files_routes).
        _ if path.starts_with("/api/files") => {
            files_routes::handle(method, path, body, state, access)
        }
        // Analytics assets: the markdown a section is built from. Their own
        // family because an asset is reached by its own key rather than through
        // a document's — see `web_canvas_server/analytics_routes.rs`.
        _ if path.starts_with("/api/analytics") => {
            analytics_routes::handle(method, path, body, state, access)
        }
        // The unsaved-work draft: one slot, no key, dropped once restored.
        _ if path.starts_with("/api/recovery") => {
            recovery_routes::handle(method, path, body, state, access)
        }
        _ => not_found_reply(),
    }
}
