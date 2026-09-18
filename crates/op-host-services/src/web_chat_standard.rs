//! Standard chat/design turn for the Rust web shell.
//!
//! The browser owns the immediate UI. This endpoint accepts an optional
//! request-scoped built-in credential and mirrors the desktop "standard mode"
//! route on the daemon side: classify the user's turn, then dispatch to plain
//! chat, design modification, or the orchestrator-backed new-design pipeline.
//! Host CLI and ACP providers are intentionally unavailable on the web route.
//!
//! This file is the spine: the request shape and its parsing, the turn itself,
//! the document sink it commits through, the `#[path]` test modules, and the
//! re-exports that keep every path into the moved parts where it was. The route
//! bodies live in `web_chat_standard_routes.rs`, the recipe and reply rules in
//! `web_chat_standard_recipe.rs`, the snapshot and starter-frame bookkeeping in
//! `web_chat_standard_starter.rs`.

use std::io::Write;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use op_ai::chat_provider::ThinkingMode;
use op_ai::chat_provider::{
    ChatAttachment, ChatDelta, ChatHistoryRole, ChatProvider, ChatRequest, StopReason,
};
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

#[path = "web_chat_standard_recipe.rs"]
mod recipe;
pub(crate) use recipe::composes_new_screen;
// `kit_recipe` + `recipe_base_rule` are read outside this module as well: the
// modify plan renders the rule a placement contributes into the prompt of the
// route a recipe turn actually takes, and it builds it from this one builder
// rather than keeping a second copy of the wording (issue #207). The rest of
// these stay module-private — `routes` reaches them through `use super::*`.
use recipe::{
    insert_composed_screens, place_selected_recipe, recipe_base_to_place, reference_evidence,
    split_composed_screens,
};
pub(crate) use recipe::{kit_recipe, recipe_base_rule};

#[path = "web_chat_standard_routes.rs"]
mod routes;
use routes::{
    resolve_standard_route, stream_chat_route, stream_modify_route, stream_new_design_route,
};

#[path = "web_chat_standard_starter.rs"]
mod starter;
pub(crate) use starter::clear_fresh_starter_frame_for_design;
use starter::{apply_request_snapshot, clear_starter_frame_for_design, inject_transient_builtin};
// Read only by the test modules below — the only reader the old path had. The
// clear itself is called from `starter`, so nothing else needs the name.
#[cfg(test)]
use starter::clear_live_starter_frame_for_design;

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

/// What the stage *before* routing decided, carried into the route that draws.
///
/// Both answers come out of that earlier stage and are read together by
/// `stream_new_design_route`: whether the request carries a reference picture
/// (no recipe base may be laid over it, issue #65) and the recipe base the
/// pre-classification placement already put on the page (the route must not
/// clone a second copy of it, issue #189). Bundled rather than passed one by
/// one because the route's parameter list had reached its arity limit, and
/// because the two facts are the same decision's two halves.
pub(crate) struct PlacedBase {
    pub(crate) reference: op_editor_core::ReferenceEvidence,
    pub(crate) placed_recipe: Option<(String, op_editor_core::NodeId)>,
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
    // ── The starter frame, and the one rule about it (issue #202) ───────────
    //
    // The blank `Frame` the daemon opens a document with is not part of a design
    // request, so a turn that draws drops it (issue #184). The deletion is a
    // document mutation like any other: it takes the collab gate, write
    // admission and a version bump. **The clear may only happen on a turn that
    // will draw** — a turn that answers in words must leave the page exactly as
    // it found it.
    //
    // So it happens in two steps, and the order is the whole fix:
    //
    //   1. here, before routing, a *probe* clears the frame in this turn's own
    //      snapshot and nothing else. Routing reads that snapshot (page empty →
    //      a classified `Modify` becomes `New`), so the route decision is
    //      bit-for-bit the one this endpoint made when the clear was live.
    //   2. below, once the route is known, the live clear runs for every route
    //      except `Chat`.
    //
    // Before that split, the clear ran here against the *live* document, on the
    // keyword verdict of a different classifier than the one that routes. An
    // English "Design a settings page…" is a `Design` keyword for
    // `op_orchestrator::classify_intent` and a conversation for the second
    // classifier, so the document lost its only frame and the chat route then
    // drew nothing: `pages[0]` went from 1 node to 0, the turn reported `done`,
    // and the single version bump of the turn *was* the deletion.
    let design_keyword = matches!(
        op_orchestrator::classify_intent(&req.ai.user),
        op_orchestrator::Intent::Design
    );
    let starter_frame_probed_away =
        design_keyword && clear_fresh_starter_frame_for_design(&mut snapshot);
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

    // Step 2 of the starter-frame rule above: the route is known, so the
    // deletion the probe only previewed can now be committed — and it is
    // committed for the two routes that draw, never for the one that talks.
    //
    // The `design_keyword` half of the condition is what keeps the drawing
    // routes exactly as they were: a turn that did not match the design
    // keywords never had its starter frame cleared, and a modify turn against a
    // blank starter is the turn that rewrites that frame in place.
    if design_keyword && !matches!(intent, crate::chat_intent::DesignIntent::Chat) {
        clear_starter_frame_for_design(&mut snapshot, state, hub, write_barrier);
    }

    match intent {
        crate::chat_intent::DesignIntent::Chat => {
            // The chat route writes nothing to the document, so the frame the
            // probe dropped from this turn's snapshot must still be on the
            // canvas — and the reply has to describe the canvas the user
            // actually has (issue #202).
            if starter_frame_probed_away {
                snapshot = {
                    let guard = state.lock().unwrap_or_else(|p| p.into_inner());
                    guard.editor.clone()
                };
            }
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
            // The starter frame was already cleared above, by the rule that
            // makes the clear belong to a drawing route — the route that
            // actually draws the screens decides it, not the keyword classifier
            // (a Russian "сделай два экрана" is no `classify_intent` design word,
            // and the starter frame it left behind was the third screen in the
            // measurement, issue #184).
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
                // What the pre-classification stage decided: whether this is a
                // reference turn, and the base this turn already stands on — if
                // the placement put one there. Carried in so the route below
                // cannot place a second copy of it (issue #189).
                PlacedBase {
                    reference,
                    placed_recipe,
                },
            )
        }
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
                // This is the daemon drawing on its own, and it is THE path a
                // design turn takes (the MCP connection is not involved), so the
                // turn-result guard has to be armed here: an autosave from a tab
                // that never took the screen must be refused rather than allowed
                // to replace it mid-turn (issues #247/#248, measured — the guard
                // armed only on the MCP path left a real turn unprotected). The
                // file write is not per command; the route does it once at the
                // end of the turn.
                guard.note_turn_result();
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

#[cfg(test)]
#[path = "web_chat_standard_starter_clear_tests.rs"]
mod starter_clear_tests;

#[cfg(test)]
#[path = "web_chat_standard_truncation_tests.rs"]
mod truncation_tests;
