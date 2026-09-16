//! Standard chat/design turn for the Rust web shell.
//!
//! The browser owns the immediate UI. This endpoint accepts an optional
//! request-scoped built-in credential and mirrors the desktop "standard mode"
//! route on the daemon side: classify the user's turn, then dispatch to plain
//! chat, design modification, or the orchestrator-backed new-design pipeline.
//! Host CLI and ACP providers are intentionally unavailable on the web route.

use std::io::Write;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
#[cfg(test)]
use op_ai::chat_provider::StopReason;
use op_ai::chat_provider::ThinkingMode;
use op_ai::chat_provider::{ChatAttachment, ChatDelta, ChatHistoryRole, ChatProvider, ChatRequest};
use op_editor_core::chat::MAX_ATTACHMENT_BYTES;
use op_editor_core::{BuiltinAgentConfig, EditorCommand, EditorState, NodeId};
use op_orchestrator::{
    AbortFlag, DesignRequest, DocSink, Orchestrator, Progress, SkippedScreenshotProvider,
    SkippedVisionLlmClient, ValidationProviders,
};
use serde_json::Value;

use crate::ai_proxy::AiStreamRequest;
use crate::chat_provider_llm::ChatProviderLlmClient;
use crate::pre_validator::LintPreValidator;
use crate::web_canvas_server::{SseHub, WebCanvasState};

#[path = "web_chat_standard_error.rs"]
mod error;
use error::WebChatStandardError;

#[path = "web_chat_standard_events.rs"]
mod events;
use events::{
    progress_label, web_identity_seed, write_agent_identity_event, write_delta_event,
    write_done_event, write_error_event, write_thinking_event,
};

#[path = "web_chat_standard_model_selection.rs"]
mod model_selection;
use model_selection::selected_model_id;

const STANDARD_MODIFY_STEP: &str =
    r#"<step title="Checking guidelines">Analyzing modification request...</step>"#;

/// What a recipe turn gets when the reply only edited the placed template and
/// composed no screen of its own (issue #182).
///
/// It says what happened instead of apologising, and it is the only sentence on
/// this route that tells the user the canvas does not hold the screen they
/// asked for. `<!-- APPLIED -->` stays: nodes really were written.
const EDIT_WITHOUT_COMPOSE_NOTICE: &str =
    "\n\n⚠ No new screen was composed: this turn only edited the template the host placed as \
     the base for it, and that template's own content still stands. What the request described \
     is not on the canvas — ask for it as a new screen, or keep editing this one until it is \
     the screen you want.";

pub struct WebStandardTurnRequest {
    pub ai: AiStreamRequest,
    document_json: Option<String>,
    editor_meta: Option<op_pen_loader::EditorMeta>,
    selected_ids: Vec<String>,
    active_page_id: Option<String>,
    agent_team_size: Option<u32>,
    history: Vec<(ChatHistoryRole, String)>,
    attachments: Vec<ChatAttachment>,
    transient_builtin: Option<BuiltinAgentConfig>,
}

pub fn parse_standard_turn_body(body: &str) -> Option<WebStandardTurnRequest> {
    let ai = crate::ai_proxy::parse_ai_stream_body(body)?;
    let value: Value = serde_json::from_str(body).ok()?;
    let obj = value.as_object()?;
    let document_json = obj.get("document").map(Value::to_string);
    let editor_meta = obj
        .get("editorMeta")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let selected_ids = obj
        .get("selectedIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let active_page_id = obj
        .get("activePageId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let agent_team_size = obj
        .get("agent_team_size")
        .and_then(Value::as_u64)
        .map(|n| (n as u32).clamp(1, 6));
    let history = parse_chat_history(obj.get("history"));
    let attachments = parse_chat_attachments(obj.get("attachments"));
    let transient_builtin = match obj.get("credential") {
        None | Some(Value::Null) => None,
        Some(value) => Some(crate::web_credentials::parse_transient_builtin(value)?),
    };
    Some(WebStandardTurnRequest {
        ai,
        document_json,
        editor_meta,
        selected_ids,
        active_page_id,
        agent_team_size,
        history,
        attachments,
        transient_builtin,
    })
}

fn parse_chat_history(value: Option<&Value>) -> Vec<(ChatHistoryRole, String)> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let role = match obj.get("role").and_then(Value::as_str) {
                Some("user") => ChatHistoryRole::User,
                Some("assistant") => ChatHistoryRole::Assistant,
                _ => return None,
            };
            let content = obj.get("content").and_then(Value::as_str)?.to_string();
            if content.trim().is_empty() {
                return None;
            }
            Some((role, content))
        })
        .collect()
}

fn parse_chat_attachments(value: Option<&Value>) -> Vec<ChatAttachment> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())?
                .to_string();
            let media_type = obj
                .get("media_type")
                .or_else(|| obj.get("mediaType"))
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())?
                .to_string();
            let encoded = obj
                .get("data_base64")
                .or_else(|| obj.get("dataBase64"))
                .and_then(Value::as_str)?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()?;
            if data.len() > MAX_ATTACHMENT_BYTES {
                return None;
            }
            Some(ChatAttachment {
                name,
                media_type,
                data,
            })
        })
        .collect()
}

/// Everything an AI turn needs to commit to the canvas: the document
/// authority, the stream that announces a change, and the shutdown admission
/// that decides whether a commit may happen at all.
///
/// Bundled because the three always travel together — and separating them is
/// how `/api/ai/standard` came to have commit points with no admission.
#[derive(Clone, Copy)]
pub(crate) struct CanvasWriteTarget<'a> {
    pub(crate) state: &'a Mutex<WebCanvasState>,
    pub(crate) hub: &'a SseHub,
    pub(crate) write_barrier: Option<&'a crate::web_canvas_server::WriteBarrier>,
}

/// Admission for one document commit on the AI path.
///
/// The conversation itself never holds the shutdown barrier — a model turn can
/// run for minutes and would block every stop. The pass is taken only for the
/// instant a commit touches the document, and refused once shutdown has closed
/// the barrier: the reply still streams back, but the document is left alone
/// and the caller is told the turn was not applied.
fn admit_document_write(
    barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> Result<Option<crate::web_canvas_server::WritePass<'_>>, WebChatStandardError> {
    let Some(barrier) = barrier else {
        return Ok(None); // local/managed: no flush to protect
    };
    barrier
        .enter()
        .map(Some)
        .ok_or(WebChatStandardError::ShuttingDown)
}

pub fn stream_standard_turn<W: Write>(
    out: &mut W,
    req: WebStandardTurnRequest,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
    cors_origin: Option<&str>,
) -> std::io::Result<()> {
    crate::ai_proxy::write_sse_headers(out, cors_origin)?;

    let mut snapshot = match apply_request_snapshot(&req, state, hub, write_barrier) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            // `write_error_event` feeds `op-ai`'s `ChatDelta::Error(String)`
            // SSE frame; render the typed failure at that boundary only.
            return write_error_event(out, &error.to_string());
        }
    };

    let model = selected_model_id(&req.ai, &snapshot);
    if matches!(
        op_orchestrator::classify_intent(&req.ai.user),
        op_orchestrator::Intent::Design
    ) {
        clear_starter_frame_for_design(&mut snapshot, state, hub, write_barrier);
    }
    inject_transient_builtin(&mut snapshot, req.transient_builtin.as_ref());

    let credential_persistence = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .credential_persistence;
    let providers = (|| -> Result<_, WebChatStandardError> {
        let resolve = |chat_session| {
            crate::ai_proxy::proxy_provider_for_request_with_chat_session(
                &snapshot,
                &req.ai,
                chat_session,
                credential_persistence,
            )
            // `ProxyProviderError` is transparent, so the sentence the SSE
            // `error` event carries is unchanged; this variant is flat
            // because the resolve step reports one sentence to the browser.
            .map_err(|error| WebChatStandardError::ProviderResolve(error.to_string()))?
            .ok_or(WebChatStandardError::NoModelConfigured)
        };
        Ok((resolve(false)?, resolve(true)?, resolve(false)?))
    })();
    let (classify_provider, chat_provider, design_provider) = match providers {
        Ok(providers) => providers,
        Err(error) => return write_error_event(out, &error.to_string()),
    };

    // The recipe goes in before the turn is classified: with the base on the
    // page and selected, this is a modification of a real screen instead of a
    // request to invent one.
    //
    // A reference image changes that: "make it like this picture" is a request
    // to follow the picture, and routing it into a recipe rewrite throws the
    // picture away (the modify path never sees attachments). So the reference
    // wins and the recipe stays out of the way.
    //
    // The decision is taken on this route's own attachment list, which arrives
    // on the wire body: whether the turn carries a picture is known exactly
    // here, so nothing below guesses it from the prompt's words (issue #65).
    let reference = reference_evidence(&req);
    let placed_recipe = if recipe_base_to_place(&req.ai.user, reference, false).is_some() {
        place_selected_recipe(&req.ai.user, state, hub, write_barrier)
    } else {
        None
    };
    if placed_recipe.is_some() {
        snapshot = {
            let guard = state.lock().unwrap_or_else(|p| p.into_inner());
            guard.editor.clone()
        };
    }

    let classified = crate::chat_intent::classify_intent_for_standard_route(
        classify_provider.as_ref(),
        &snapshot,
        &req.ai.user,
        model.clone(),
    );
    // When the host placed a recipe, the turn is a rewrite of that screen's
    // placeholder content — say so, instead of leaving the model to infer it
    // from a request that reads like "build me a switches screen".
    let recipe_hint =
        placed_recipe
            .as_ref()
            .map(|(recipe_id, _)| crate::chat_intent::RecipeBaseHint {
                recipe_id: recipe_id.clone(),
                name: kit_recipe(recipe_id)
                    .map(|recipe| recipe.name.clone())
                    .unwrap_or_else(|| recipe_id.clone()),
            });
    let modify_plan =
        crate::chat_intent::build_modify_plan_with(&snapshot, &req.ai.user, recipe_hint.as_ref());
    let page_children_empty = snapshot.active_children().is_empty();
    let intent = if reference == op_editor_core::ReferenceEvidence::Attached {
        // "Make it like this picture" with the picture attached is a build
        // request by construction: the reference brief is what the turn is
        // for. Letting the classifier read it as conversation answered a
        // question the user did not ask.
        crate::chat_intent::DesignIntent::New
    } else if placed_recipe.is_some() && modify_plan.is_some() {
        crate::chat_intent::DesignIntent::Modify
    } else {
        resolve_standard_route(classified, page_children_empty, modify_plan.is_some())
    };

    match intent {
        crate::chat_intent::DesignIntent::Chat => {
            stream_chat_route(out, &req, &snapshot, chat_provider.as_ref(), model)
        }
        crate::chat_intent::DesignIntent::Modify => {
            let plan = modify_plan.expect("route checked has_modify_plan");
            stream_modify_route(
                out,
                plan,
                design_provider.as_ref(),
                model.as_deref(),
                state,
                hub,
                write_barrier,
            )
        }
        crate::chat_intent::DesignIntent::New => {
            // The route that actually draws the screens decides this, not the
            // keyword classifier above: a Russian "сделай два экрана" is no
            // `classify_intent` design word, and the starter frame it left
            // behind was the third screen in the measurement (issue #184).
            clear_starter_frame_for_design(&mut snapshot, state, hub, write_barrier);
            stream_new_design_route(
                out,
                req,
                snapshot,
                design_provider,
                model,
                CanvasWriteTarget {
                    state,
                    hub,
                    write_barrier,
                },
                reference,
                // The base this turn already stands on, if the pre-classification
                // placement put one there. Carried in so the route below cannot
                // place a second copy of it (issue #189).
                placed_recipe,
            )
        }
    }
}

/// The recipe this turn still has to place, or `None` when one must not be
/// placed — or when the base is already on the page.
///
/// The one gate both placements read: the pre-classification placement in
/// [`stream_standard_turn`], and the placement inside
/// [`stream_new_design_route`] for a turn that resolved to `New` anyway. A
/// reference turn is not a recipe turn (issue #65), and a turn whose base is
/// already placed must not clone it again — `instantiate_component` does not
/// dedupe by master, so a second placement leaves two recipe roots, one of them
/// ~20px offset and described by no rule (issue #189).
fn recipe_base_to_place<'a>(
    prompt: &str,
    reference: op_editor_core::ReferenceEvidence,
    base_already_placed: bool,
) -> Option<&'a op_editor_core::kit_manifest::KitRecipe> {
    if base_already_placed {
        return None;
    }
    op_editor_core::recipe_to_place(prompt, reference, op_editor_core::session_kit())
}

/// The kit's recipe with this id — a placement returns the id, and everything
/// that describes the placed base needs the name that goes with it.
fn kit_recipe(recipe_id: &str) -> Option<&'static op_editor_core::kit_manifest::KitRecipe> {
    op_editor_core::session_kit()
        .recipes
        .iter()
        .find(|recipe| recipe.id == recipe_id)
}

/// The `doc:recipe-base` rule: the recipe is on the page, it is what this turn
/// is based on, adapt it instead of composing it again.
fn recipe_base_rule(
    recipe: &op_editor_core::kit_manifest::KitRecipe,
    node_id: &op_editor_core::NodeId,
) -> jian_ops_schema::DesignRule {
    jian_ops_schema::DesignRule {
        id: "doc:recipe-base".into(),
        title: format!("Recipe already placed: {}", recipe.name),
        instruction: format!(
            "The product already placed recipe `{}` as node `{}`. It is the base for \
             this turn: keep its shell, table chrome and pagination, and adapt what \
             it provides — retitle it for this product, replace the sample column \
             data, delete or hide the blocks the request does not need. Do not \
             compose this screen again and do not rebuild its structure.",
            recipe.id,
            node_id.as_str()
        ),
        kind: jian_ops_schema::DesignRuleKind::Require,
        scope: jian_ops_schema::DesignRuleScope::Global,
        condition: None,
        priority: i32::MIN + 1,
        enabled: true,
        overrides: None,
    }
}

/// Whether this turn carries a reference picture, from the attachment list the
/// route itself holds.
///
/// `req.attachments` arrives on the wire body, so this is knowledge rather than
/// inference: a picture attached with no words about it is a reference turn,
/// and "like on the screenshot" with nothing attached is not. Everything on
/// this route that must stand down for a reference turn — the recipe
/// placement, the `doc:recipe-base` rule, the recipe rules handed to the
/// orchestrator — reads this one value, and nothing reads the prompt's words
/// (issue #65).
fn reference_evidence(req: &WebStandardTurnRequest) -> op_editor_core::ReferenceEvidence {
    op_editor_core::ReferenceEvidence::of_attachments(req.attachments.iter().any(|a| a.is_image()))
}

fn apply_request_snapshot(
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

fn inject_transient_builtin(state: &mut EditorState, transient: Option<&BuiltinAgentConfig>) {
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
fn clear_starter_frame_for_design(
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

fn clear_live_starter_frame_for_design(state: &mut WebCanvasState) -> Option<u64> {
    if !clear_fresh_starter_frame_for_design(&mut state.editor) {
        return None;
    }
    state.version += 1;
    Some(state.version)
}

/// Place the recipe this request asks for, before anything is classified.
///
/// Returns the placed root's id. The selection lands on it, so the turn that
/// follows is a *modification* of an existing screen rather than a request to
/// compose one — which is the difference between "adapt the recipe" as an
/// instruction the model may skip and as the shape of the turn itself.
fn place_selected_recipe(
    user_message: &str,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> Option<(String, op_editor_core::NodeId)> {
    let recipe = op_editor_core::select_recipe(user_message, op_editor_core::session_kit())?;
    let master = op_editor_core::NodeId::new(recipe.template.clone());
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let gated = guard
        .gate_daemon_mutation(
            op_editor_core::CollabGateAction::Document(
                op_editor_core::CollabDocumentMutation::BasicNodeInsert,
            ),
            op_editor_core::CollabEditSource::Ai,
        )
        .is_ok();
    if !gated {
        return None;
    }
    let _pass = admit_document_write(write_barrier).ok()?;
    // Clear the starter BEFORE placing: the clear only recognises an
    // untouched starter document, and placing the recipe already touched it.
    clear_fresh_starter_frame_for_design(&mut guard.editor);
    let node_id = guard.editor.instantiate_component(&master)?;
    // The request may name blocks it does not want. That is a product
    // decision like the recipe choice itself, so it happens here rather
    // than in a prompt the model may or may not honour.
    let hidden = op_editor_core::hide_blocks_in_subtree(
        &mut guard.editor,
        &node_id,
        &op_editor_core::requested_hidden_blocks(user_message, recipe),
    );
    if hidden > 0 {
        guard.editor.mark_document_changed();
    }
    // An empty root beside the placed screen is noise the user has to delete
    // (it is either the untouched starter or a root the model opened and left
    // blank). The recipe root is the page now.
    let keep = node_id.as_str().to_string();
    let pruned = {
        use op_editor_core::PenNodeExt as _;
        let before = guard.editor.active_children().len();
        guard.editor.active_children_mut().retain(|child| {
            child.id_str() == keep || child.children().is_some_and(|kids| !kids.is_empty())
        });
        guard.editor.active_children().len() != before
    };
    guard.editor.set_single_selection(node_id.clone());
    // Every mutation above is a document change, and `version` is the key the
    // browser's live-sync loop polls (`wants_version`): a placement that leaves
    // it where it was is a 470-node change no poller is told about (issue
    // #183). Counted, one bump each: the clone of the master onto the page, the
    // optional blocks the request asked to hide, and the empty-root prune. Not
    // counted: the selection — turn state, not document content — and the
    // starter clear, which was already gone under the prune.
    guard.version += 1 + u64::from(hidden > 0) + u64::from(pruned);
    let tick = guard.sse_tick();
    drop(guard);
    hub.broadcast(tick);
    Some((recipe.id.clone(), node_id))
}

fn resolve_standard_route(
    classified: crate::chat_intent::DesignIntent,
    page_children_empty: bool,
    has_modify_plan: bool,
) -> crate::chat_intent::DesignIntent {
    match classified {
        crate::chat_intent::DesignIntent::Modify if page_children_empty => {
            crate::chat_intent::DesignIntent::New
        }
        crate::chat_intent::DesignIntent::Modify if !has_modify_plan => {
            crate::chat_intent::DesignIntent::New
        }
        other => other,
    }
}

fn stream_chat_route<W: Write>(
    out: &mut W,
    req: &WebStandardTurnRequest,
    state: &EditorState,
    provider: &dyn ChatProvider,
    model: Option<String>,
) -> std::io::Result<()> {
    let chat_req = ChatRequest {
        system_prompt: crate::chat_system_prompt::build_chat_system_prompt(state, &req.ai.user),
        user_message: req.ai.user.clone(),
        history: req.history.clone(),
        max_output_tokens: req.ai.max_output_tokens,
        thinking: req.ai.thinking,
        effort: req.ai.effort,
        attachments: req.attachments.clone(),
        model,
    };
    for delta in provider.send(chat_req) {
        out.write_all(crate::ai_proxy::delta_to_sse(&delta).as_bytes())?;
        out.flush()?;
        if matches!(delta, ChatDelta::Done { .. } | ChatDelta::Error(_)) {
            break;
        }
    }
    Ok(())
}

/// Whether a MODIFY reply composed a screen of its own, or only edited the
/// recipe the host had already placed.
///
/// The route hands the model a recipe the kit placed as this turn's base, and
/// asks it to rewrite that screen (`ModifyPlan::rewrites_a_placed_recipe`). The
/// reply arrives as `(parent, node)` ops and can come back in two shapes that
/// the wire reported identically before this existed (issue #182): ops that
/// produce screen content the user asked for, and ops that only reach inside
/// the placed tree and retitle the kit's template. Both end in `done` with
/// `<!-- APPLIED -->`, because nodes genuinely were written either way — the
/// turn that shipped the untouched template counted 500 nodes, the most in the
/// corpus, while delivering the least of what was asked.
///
/// A *screen-level statement* is a root-level op (`parent == "null"`) that
/// [`crate::chat_canvas_tools::apply_design_modification`] will apply to the
/// screen rather than to a node inside it:
///
/// * it carries **no id**, or an id the document does not already hold, so
///   there is nothing to replace and the op is inserted as new content into the
///   captured target frame; or
/// * its id **is** one of the captured target frames, so the op replaces the
///   placed root — the whole screen — with the model's own version of it
///   (measured on this route: the model rewrites a 470-node base this way and
///   lands a 222-node screen of its own).
///
/// Everything else stays inside the placed tree: an op naming an id that exists
/// *below* the target frames replaces that node, and an op with an explicit
/// parent inserts under it. That is the shape the #182 measurement caught —
/// "root-level DSL statements in the delta: 0, every statement targeted an
/// existing id" — and it is why the applier's own branch is the definition: a
/// replacement of an inner node and an insert into the placed tree both leave
/// the kit's screen standing.
///
/// The id-less case keeps the applier's precondition: an op that names no
/// parent reaches the document only when exactly one target frame was captured,
/// so with several targets it is not a screen-level statement either.
pub(crate) fn composes_new_screen(
    state: &EditorState,
    nodes: &[crate::chat_canvas_tools::DesignModificationOp],
    target_frame_ids: &[String],
) -> bool {
    nodes
        .iter()
        .any(|(parent, node)| is_screen_level_statement(state, parent, node, target_frame_ids))
}

fn is_screen_level_statement(
    state: &EditorState,
    parent: &str,
    node: &Value,
    target_frame_ids: &[String],
) -> bool {
    if parent != "null" {
        return false;
    }
    match node.get("id").and_then(Value::as_str) {
        Some(id) => target_frame_ids.iter().any(|frame| frame == id) || !node_exists(state, id),
        None => target_frame_ids.len() == 1,
    }
}

fn node_exists(state: &EditorState, id: &str) -> bool {
    op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(id)).is_some()
}

/// Split a reply into the statements that compose a screen of the model's own
/// and the statements that edit inside the frames the turn captured.
///
/// The two are applied differently, which is the whole point of telling them
/// apart: an edit belongs inside the screen it edits, while a composed screen
/// belongs *beside* the base this turn stands on. The modify applier cannot make
/// that distinction — a root-level op naming an id the document does not hold
/// takes the implicit parent like an id-less one does, so it is nested into the
/// captured frame. On a recipe turn that is where the screen the model composed
/// disappears: the user gets the kit template with a stranger's frame inside it,
/// and because the reply *did* compose something, `composes_new_screen` reports
/// success and the notice that would have said so is withheld (issue #182).
///
/// Only a *root-level op that carries an id of its own* counts as a composed
/// screen. An op naming no id keeps the applier's documented contract — with
/// exactly one captured frame it is new content *for* that screen, which is how
/// a label or a row gets added to the screen the user selected.
fn split_composed_screens(
    state: &EditorState,
    nodes: Vec<crate::chat_canvas_tools::DesignModificationOp>,
    target_frame_ids: &[String],
) -> (
    Vec<crate::chat_canvas_tools::DesignModificationOp>,
    Vec<crate::chat_canvas_tools::DesignModificationOp>,
) {
    nodes
        .into_iter()
        .partition(|(parent, node)| is_screen_of_its_own(state, parent, node, target_frame_ids))
}

fn is_screen_of_its_own(
    state: &EditorState,
    parent: &str,
    node: &Value,
    target_frame_ids: &[String],
) -> bool {
    parent == "null"
        && node.get("id").and_then(Value::as_str).is_some_and(|id| {
            !target_frame_ids.iter().any(|frame| frame == id) && !node_exists(state, id)
        })
}

/// Place the screens the model composed at the page root.
///
/// Through the same chat tool the modify applier inserts with, and the same
/// JSON shape — `{"data": node}` — with the parent left out: the tool's own
/// contract reads a missing parent as the page root, which is what puts the
/// composed screen beside the placed base instead of inside it.
fn insert_composed_screens(
    state: &mut EditorState,
    screens: &[crate::chat_canvas_tools::DesignModificationOp],
) -> (usize, bool) {
    let mut applied = 0usize;
    let mut mutated = false;
    for (_, node) in screens {
        let args = serde_json::json!({ "data": node }).to_string();
        let (result, did_mutate) =
            crate::chat_canvas_tools::execute_chat_tool(state, "insert_node", &args);
        if !result.is_error {
            applied += 1;
            mutated |= did_mutate;
        }
    }
    (applied, mutated)
}

fn stream_modify_route<W: Write>(
    out: &mut W,
    plan: crate::chat_intent::ModifyPlan,
    provider: &dyn ChatProvider,
    model: Option<&str>,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> std::io::Result<()> {
    write_delta_event(out, STANDARD_MODIFY_STEP)?;
    let target_frame_ids = plan.target_frame_ids;
    // Read before the fields move: this is the flag that says the base was
    // placed *for this turn*, so the reply owes the user a rewritten screen.
    let rewrites_a_placed_recipe = plan.rewrites_a_placed_recipe;
    // Rewriting a whole placed screen — every column header and every sample
    // row — does not fit in the default reply budget, and a reply cut short
    // is what "it changed the headers but not the data" looks like.
    let max_output_tokens = if plan.rewrites_a_placed_recipe {
        16384
    } else {
        8192
    };
    // What the screen's JSON and the model's hidden reasoning share. This was
    // the one design entry point that never applied the design-turn policy:
    // desktop, mobile and the orchestrator all force thinking off for a model
    // whose profile says it burns its budget inside `<think>` (glm-5.2
    // measured at thinking≈30k / text=0, DeepSeek V4 at 19 s of reasoning for
    // 0 characters of answer, #179), and this route left it on — which is what
    // "it changed the headers but not the data" is made of.
    let thinking = if op_orchestrator::design_turn_disables_thinking(model) {
        ThinkingMode::Disabled
    } else {
        ThinkingMode::Adaptive
    };
    let request = ChatRequest {
        system_prompt: plan.system_prompt,
        user_message: plan.user_message,
        max_output_tokens,
        thinking,
        ..Default::default()
    };
    let mut full_response = String::new();
    let mut stream_error: Option<String> = None;
    for delta in provider.send(request) {
        match delta {
            ChatDelta::TextDelta(s) => full_response.push_str(&s),
            ChatDelta::Thinking(_) | ChatDelta::ToolUse { .. } => {}
            ChatDelta::Error(msg) => {
                stream_error = Some(msg);
                break;
            }
            ChatDelta::Done { .. } => break,
        }
    }

    let nodes = crate::chat_intent::parse_modify_nodes(&full_response);
    if !nodes.is_empty() {
        write_delta_event(out, &format!("\n{full_response}"))?;
        let (applied, composed, tick) = {
            let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
            // `apply_design_modification` writes a batch straight into the
            // editor, so the gate runs before it rather than per command.
            if guard
                .gate_daemon_mutation(
                    op_editor_core::CollabGateAction::Document(
                        op_editor_core::CollabDocumentMutation::NodePropertyBatch,
                    ),
                    op_editor_core::CollabEditSource::Ai,
                )
                .is_err()
            {
                (0, false, None)
            } else {
                // Shutting down: the reply still streams, the document is left
                // exactly as the flush will find it.
                let admitted = admit_document_write(write_barrier).ok();
                let (count, composed, mutated) = if admitted.is_none() {
                    (0, false, false)
                } else {
                    // Read the reply against the document BEFORE it is applied:
                    // afterwards every id it inserted exists, and an insert is
                    // indistinguishable from a replacement.
                    let composed = composes_new_screen(&guard.editor, &nodes, &target_frame_ids);
                    // A recipe turn is the one turn whose reply can compose a
                    // screen *of its own* while a placed base stands on the
                    // page; that screen goes beside the base, not inside it
                    // (issue #182).
                    let (screens, edits) = if rewrites_a_placed_recipe {
                        split_composed_screens(&guard.editor, nodes, &target_frame_ids)
                    } else {
                        (Vec::new(), nodes)
                    };
                    let (edited, edited_mutated) =
                        crate::chat_canvas_tools::apply_design_modification(
                            &mut guard.editor,
                            &edits,
                            &target_frame_ids,
                        );
                    let (screens_applied, screens_mutated) =
                        insert_composed_screens(&mut guard.editor, &screens);
                    (
                        edited + screens_applied,
                        composed,
                        edited_mutated || screens_mutated,
                    )
                };
                let tick = if mutated {
                    guard.version += 1;
                    Some(guard.sse_tick())
                } else {
                    None
                };
                (count, composed, tick)
            }
        };
        if let Some(tick) = tick {
            hub.broadcast(tick);
        }
        if applied > 0 {
            // A recipe turn that never left the placed tree owes the user a
            // sentence: the canvas changed, but not into the screen the request
            // described (issue #182). The marker follows so a reader that waits
            // for `<!-- APPLIED -->` still finds it last.
            if rewrites_a_placed_recipe && !composed {
                write_delta_event(out, EDIT_WITHOUT_COMPOSE_NOTICE)?;
            }
            write_delta_event(out, "\n\n<!-- APPLIED -->")?;
        }
        return write_done_event(out);
    }

    let message = if let Some(err) = stream_error {
        err
    } else {
        let trimmed = full_response.trim();
        let hint = if trimmed.is_empty() {
            "The model returned an empty response.".to_string()
        } else {
            let preview: String = trimmed.chars().take(150).collect();
            let ellipsis = if full_response.chars().count() > 150 {
                "…"
            } else {
                ""
            };
            format!("Model output: \"{preview}{ellipsis}\"")
        };
        format!("Could not parse design nodes from model response. {hint}")
    };
    write_error_event(out, &message)
}

fn stream_new_design_route<W: Write>(
    out: &mut W,
    req: WebStandardTurnRequest,
    snapshot: EditorState,
    provider: Box<dyn ChatProvider>,
    model: Option<String>,
    target: CanvasWriteTarget<'_>,
    reference: op_editor_core::ReferenceEvidence,
    placed_recipe: Option<(String, op_editor_core::NodeId)>,
) -> std::io::Result<()> {
    let append_context = crate::chat_intent::detect_append_intent(&snapshot, &req.ai.user);
    let reference_attachments = req
        .attachments
        .iter()
        .filter(|a| a.is_image())
        .map(|a| op_orchestrator::ReferenceAttachment {
            name: a.name.clone(),
            media_type: a.media_type.clone(),
            data: a.data.clone(),
        })
        .collect();
    // The recipe base this route stands on is picked by the same rule as the
    // pre-classification placement, and only when that placement did not run.
    // "The model should pick the recipe" is a hope; matching the request
    // against the kit's own words is a decision. Once the base is on the page,
    // the turn is framed as adaptation of a node that already exists, which is
    // what stops the model composing the same screen from scratch.
    //
    // Not on a reference turn, though — and that is the decision this route
    // used to skip: it placed the recipe here whatever the attachments said,
    // so a screenshot attached with no word about it still got a library
    // screen laid over it AND the `doc:recipe-base` Require rule below, which
    // told the model to keep it (issue #65).
    let mut rules: Vec<jian_ops_schema::DesignRule> =
        op_editor_core::effective_design_rules(snapshot.doc.design_md.as_ref())
            .into_iter()
            .map(|entry| entry.rule)
            .collect();
    // The base this turn stands on. Placement is the pre-classification step's
    // job; when it already happened, this route describes that copy instead of
    // cloning it again — `instantiate_component` does not dedupe by master, so
    // the second clone used to land ~20px off the first and only the clone was
    // named in `doc:recipe-base` (issue #189).
    let recipe_base = match placed_recipe {
        Some((recipe_id, node_id)) => kit_recipe(&recipe_id).map(|recipe| (recipe, node_id)),
        None => recipe_base_to_place(&req.ai.user, reference, false).and_then(|recipe| {
            // Placed through the daemon's own lock, like every other write on
            // this path, so the browser sees the base on its next sync.
            let node_id = {
                let mut guard = target.state.lock().unwrap_or_else(|p| p.into_inner());
                guard
                    .editor
                    .instantiate_component(&op_editor_core::NodeId::new(recipe.template.clone()))
            }?;
            Some((recipe, node_id))
        }),
    };
    if let Some((recipe, node_id)) = recipe_base {
        rules.insert(0, recipe_base_rule(recipe, &node_id));
    }
    let mut request = DesignRequest {
        prompt: req.ai.user,
        model: model.clone(),
        provider: None,
        // The AI reads the session's resolved rules — never the document's
        // markdown brief, which the editor no longer maintains.
        rules,
        continuation_context: None,
        append_context,
        concurrency: req
            .agent_team_size
            .unwrap_or(snapshot.chat.agent_team_size)
            .clamp(1, 6),
        validation_enabled: true,
        visual_ref_enabled: false,
        pinned_style_guide: snapshot.editor_ui.pinned_style_guide.clone(),
        reference_attachments,
        reference_brief: None,
    };
    // Share one provider Arc between the design LLM and vision brief /
    // (optionally) the vision validator.
    let provider_arc: Arc<dyn ChatProvider> = Arc::from(provider);
    let llm = ChatProviderLlmClient::new(provider_arc.clone()).with_model(model.clone());
    let mut sink = WebDesignDocSink::new(target.state, target.hub, target.write_barrier, snapshot);
    let abort = AbortFlag::new();
    let pre_validator = LintPreValidator;

    // Always use a real multimodal client for reference briefs when the user
    // attached images. Post-gen validation stays behind OPENPENCIL_VISION_VALIDATION.
    let brief_vision = crate::validation_providers::ChatVisionLlmClient::new(provider_arc.clone())
        .with_model(model.clone());
    if !request.reference_attachments.is_empty() {
        op_orchestrator::reference_brief::enrich_request_with_reference_brief(
            &mut request,
            &brief_vision,
        );
    }

    // ── Class-C vision-validation provider selection (Track-1 Step 3) ──────────
    // REAL providers only when `OPENPENCIL_VISION_VALIDATION=1` (defaults OFF);
    // otherwise the no-op stubs keep `run_post_generation_validation` a
    // guaranteed short-circuit, so the default path is byte-for-byte unchanged.
    let use_real_vision = crate::validation_providers::vision_validation_enabled();
    let stub_screenshot = SkippedScreenshotProvider;
    let stub_vision = SkippedVisionLlmClient;
    let real_screenshot = crate::validation_providers::RealScreenshotProvider;
    let real_vision = crate::validation_providers::ChatVisionLlmClient::new(provider_arc.clone())
        .with_model(model.clone());
    let (screenshot, vision, system_prompt): (
        &dyn op_orchestrator::ScreenshotProvider,
        &dyn op_orchestrator::VisionLlmClient,
        String,
    ) = if use_real_vision {
        (
            &real_screenshot,
            &real_vision,
            crate::validation_providers::validation_system_prompt(),
        )
    } else {
        (&stub_screenshot, &stub_vision, String::new())
    };
    let providers = ValidationProviders {
        pre_validator: &pre_validator,
        screenshot,
        vision,
        system_prompt,
    };
    let identity =
        op_orchestrator::agent_identity::assign_agent_identities_seeded(1, web_identity_seed())
            .into_iter()
            .next()
            .expect("one requested agent identity");
    // The browser transcript learns the persona first. The daemon relay then
    // confirms that exact same identity, so the canvas cursor cannot appear
    // under a different name or colour than the visible assistant bubble.
    write_agent_identity_event(out, &identity)?;
    let epoch = op_editor_core::agent_indicators::begin();
    op_editor_core::agent_indicators::confirm_cursor_agent(epoch, &identity.color, &identity.name);
    let summary = {
        let out_ref = &mut *out;
        let mut on_progress = move |p: Progress| {
            let _ = write_thinking_event(out_ref, &format!("\n{}", progress_label(&p)));
        };
        crate::chat_runtime::block_on_anywhere(Orchestrator::new().with_indicator_epoch(epoch).run(
            request,
            &mut sink,
            &llm,
            &mut on_progress,
            &abort,
            &providers,
        ))
    };
    // Natural completion drains the queued reveals gracefully; an
    // aborted turn tears the overlay down at once.
    if abort.is_set() {
        op_editor_core::agent_indicators::end_if_epoch(epoch);
    } else {
        op_editor_core::agent_indicators::finish_if_epoch(epoch);
    }
    match summary {
        Ok(summary) => {
            let ok = summary
                .subtasks
                .iter()
                .filter(|o| o.error.is_none())
                .count();
            let failed = summary.subtasks.len() - ok;
            write_delta_event(
                out,
                &format!(
                    "\n\nDone — {} subtask(s) succeeded, {} failed, {} paintable node(s) \
                     ({} forest root(s)).",
                    ok, failed, summary.paintable_nodes, summary.total_nodes
                ),
            )?;
            write_done_event(out)
        }
        Err(e) => write_error_event(out, &e.to_string()),
    }
}

struct WebDesignDocSink<'a> {
    state: &'a Mutex<WebCanvasState>,
    hub: &'a SseHub,
    /// Admission for each generated command's commit. `None` for the local
    /// and managed daemons, which have no flush to protect.
    write_barrier: Option<&'a crate::web_canvas_server::WriteBarrier>,
    mirror: EditorState,
}

impl<'a> WebDesignDocSink<'a> {
    fn new(
        state: &'a Mutex<WebCanvasState>,
        hub: &'a SseHub,
        write_barrier: Option<&'a crate::web_canvas_server::WriteBarrier>,
        mirror: EditorState,
    ) -> Self {
        Self {
            state,
            hub,
            write_barrier,
            mirror,
        }
    }
}

impl DocSink for WebDesignDocSink<'_> {
    fn state(&self) -> &EditorState {
        &self.mirror
    }

    fn apply(&mut self, cmd: EditorCommand) -> bool {
        // Each generated command is its own document commit, so each needs its
        // own instant of admission. A closed barrier acks `false`, which the
        // generator already treats as "not applied".
        let Ok(_write_pass) = admit_document_write(self.write_barrier) else {
            return false;
        };
        let (applied, tick, snapshot) = {
            let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
            // A refusal and a no-op both ack `false` to the generator, which is
            // the existing contract; the difference is visible in the session
            // notice the gate raises, not in this return value.
            let applied = guard
                .apply_gated(cmd, op_editor_core::CollabEditSource::Ai)
                .unwrap_or(false);
            let tick = if applied {
                crate::design_session::fit_design_viewport_to_content(
                    &mut guard.editor,
                    1440.0,
                    900.0,
                );
                guard.version += 1;
                Some(guard.sse_tick())
            } else {
                None
            };
            (applied, tick, guard.editor.clone())
        };
        self.mirror = snapshot;
        if let Some(tick) = tick {
            self.hub.broadcast(tick);
        }
        applied
    }

    fn begin_undo_batch(&mut self) {}

    fn end_undo_batch(&mut self) {}
}

#[cfg(test)]
#[path = "web_chat_standard_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "web_chat_standard_model_tests.rs"]
mod model_tests;

#[cfg(test)]
#[path = "web_chat_standard_reference_image_tests.rs"]
mod reference_image_tests;

#[cfg(test)]
#[path = "web_chat_standard_recipe_reference_tests.rs"]
mod recipe_reference_tests;

#[cfg(test)]
#[path = "web_chat_standard_turn_outcome_tests.rs"]
mod turn_outcome_tests;
