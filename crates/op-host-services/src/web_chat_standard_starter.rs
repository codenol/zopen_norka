//! The editor state a turn reasons from, and the blank starter frame it clears.
//!
//! Pure code motion out of `web_chat_standard.rs` at the 800-line cap: the
//! functions below are byte-for-byte the ones that lived there, in the same
//! order, reasoning comments included. They reach the spine's helpers and the
//! shared imports through `use super::*`. The spine re-exports them, so
//! `crate::web_chat_standard::clear_fresh_starter_frame_for_design` and the
//! test modules' bare names still resolve.

use super::*;

pub(super) fn apply_request_snapshot(
    req: &WebStandardTurnRequest,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> Result<EditorState, WebChatStandardError> {
    let mut broadcast_tick = None;
    let mut snapshot = {
        let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(agent) = req.transient_builtin.as_ref() {
            if !agent.has_model(req.ai.model.trim()) {
                return Err(WebChatStandardError::TransientModelMismatch);
            }
            // `web_credentials` is outside this pass; carry its verdict text.
            crate::web_credentials::validate_web_provider_base_url(&agent.base_url)
                .map_err(|error| WebChatStandardError::EndpointRejected(error.to_string()))?;
            if !crate::web_credentials::public_demo_transient_endpoint_allowed(agent) {
                return Err(WebChatStandardError::EndpointNotAllowlisted);
            }
        }
        if let Some(doc_json) = req.document_json.as_deref() {
            let loaded = op_pen_loader::load_canonical(doc_json)
                .map_err(|e| WebChatStandardError::Document(e.to_string()))?;
            if guard.editor.doc != loaded.value {
                // A whole-document swap during a live session is exactly what
                // the collaboration protocol cannot sequence, so the gateway
                // refuses it here rather than letting the AI route silently
                // replace what peers are editing.
                guard
                    .gate_daemon_mutation(
                        op_editor_core::CollabGateAction::ReplaceDocument,
                        op_editor_core::CollabEditSource::Ai,
                    )
                    .map_err(WebChatStandardError::CollabRefused)?;
                // The one instant this turn touches the document; refused
                // outright once shutdown has closed the barrier.
                let _write_pass = admit_document_write(write_barrier)?;
                guard.replace_document(loaded.value);
                broadcast_tick = Some(guard.sse_tick());
            }
        }
        // `apply_editor_meta` and the active-page switch below both write
        // state that `EditorMeta::from_state` serialises into the tenant's
        // persisted snapshot (`active_page_index`, `preserve_authored_
        // geometry`). They are therefore document writes for admission
        // purposes even when the document itself is unchanged, and a closed
        // barrier must skip them — otherwise a turn arriving during shutdown
        // moves the active page after the flush snapshotted it.
        //
        // Skipped, not refused: the metadata is incidental to the turn, so a
        // plain chat reply still streams back rather than erroring.
        let metadata_pass = admit_document_write(write_barrier).ok();
        if metadata_pass.is_some() {
            if let Some(meta) = req.editor_meta.clone() {
                op_pen_loader::apply_editor_meta(&mut guard.editor, meta);
            }
        }
        if let Some(size) = req.agent_team_size {
            guard.editor.chat.agent_team_size = size.clamp(1, 6);
        }
        guard.editor.selection.set = req.selected_ids.iter().map(NodeId::new).collect::<Vec<_>>();
        guard.editor.selection.anchor = guard
            .editor
            .selection
            .set
            .last()
            .cloned()
            .unwrap_or(NodeId::NONE);
        if metadata_pass.is_some() {
            if let Some(page_id) = req.active_page_id.as_deref() {
                if let Some(index) = guard
                    .editor
                    .doc
                    .pages
                    .as_ref()
                    .and_then(|pages| pages.iter().position(|p| p.id == page_id))
                {
                    let _ = guard.editor.set_active_page(index);
                }
            }
        }
        guard.editor.clone()
    };
    if let Some(tick) = broadcast_tick {
        hub.broadcast(tick);
    }
    inject_transient_builtin(&mut snapshot, req.transient_builtin.as_ref());
    Ok(snapshot)
}

pub(super) fn inject_transient_builtin(
    state: &mut EditorState,
    transient: Option<&BuiltinAgentConfig>,
) {
    let Some(transient) = transient else {
        return;
    };
    let agents = &mut state.editor_ui.agent_settings.builtin_agents;
    agents.retain(|agent| agent.id != transient.id);
    agents.insert(0, transient.clone());
    state.rebuild_chat_models();
}

/// Drop the untouched starter frame from the live canvas and from the snapshot
/// this turn reasons from, when the active page still holds nothing but that
/// frame.
///
/// A design turn draws its own screens, and the blank `Frame` the daemon opened
/// the document with is not part of the request: left beside them it hands the
/// user a screen they did not ask for, inflates every screen count in the
/// audit, and makes a two-screen design read as three (issue #184).
///
/// The snapshot is refreshed only when the live clear went through, so routing
/// and the orchestrator see the same page the browser will.
pub(super) fn clear_starter_frame_for_design(
    snapshot: &mut EditorState,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) {
    if !op_editor_core::blank_starter::active_page_is_blank_starter(snapshot) {
        return;
    }
    let tick = {
        let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
        // Through the gateway like every other daemon write: during a live
        // session this housekeeping edit would be an unsequenced AI write.
        // Skipping it only means the starter frame stays, which is strictly
        // better than forking the shared document.
        let gated = guard
            .gate_daemon_mutation(
                op_editor_core::CollabGateAction::Document(
                    op_editor_core::CollabDocumentMutation::NodeDelete,
                ),
                op_editor_core::CollabEditSource::Ai,
            )
            .is_ok();
        // Also a document commit, so it needs the same instant of admission; a
        // closed barrier simply skips the clear.
        let starter_clear_pass = admit_document_write(write_barrier).ok();
        if gated
            && starter_clear_pass.is_some()
            && clear_live_starter_frame_for_design(&mut guard).is_some()
        {
            *snapshot = guard.editor.clone();
            Some(guard.sse_tick())
        } else {
            None
        }
    };
    if let Some(tick) = tick {
        hub.broadcast(tick);
    }
}

pub(crate) fn clear_fresh_starter_frame_for_design(state: &mut EditorState) -> bool {
    // Not the document comparison this used to make (`state.doc ==
    // EditorState::starter().doc`): on this daemon that can never hold.
    // `/api/file/new` builds the starter and merges the Skala library into it,
    // so `pages` carries the kit's component pages and the two documents differ
    // by construction — which left this clear dead on the route and the blank
    // starter frame on the canvas beside the screens a design turn drew (issue
    // #184). The frame is what the clear is about, so it asks about the frame,
    // through the predicate the desktop host already uses.
    if !op_editor_core::blank_starter::active_page_is_blank_starter(state) {
        return false;
    }
    state.active_children_mut().clear();
    state.clear_selection();
    // Raw `active_children_mut()` bypasses the command/history path, so it
    // must advance the content revision explicitly. Save acknowledgements
    // use that revision to avoid marking newer edits as saved.
    state.mark_document_changed();
    true
}

pub(super) fn clear_live_starter_frame_for_design(state: &mut WebCanvasState) -> Option<u64> {
    if !clear_fresh_starter_frame_for_design(&mut state.editor) {
        return None;
    }
    state.version += 1;
    Some(state.version)
}
