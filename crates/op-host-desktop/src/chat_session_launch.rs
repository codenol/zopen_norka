//! Chat-turn launch + provider routing — `launch_if_pending` and the
//! per-provider transport builders, split out of `chat_session.rs` at
//! the 800-line cap. Declared as a `#[path]` child of `chat_session`
//! so the session type and external `chat_session::` paths stay put.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use op_ai::chat_history::{trim_chat_history, DEFAULT_MAX_CHARS, DEFAULT_MAX_MESSAGES};
use op_ai::chat_provider::{ChatDelta, ChatProvider, ChatRequest};
use op_editor_core::EditorState;
use op_host_native::WidgetHostNative;
use op_orchestrator::{classify_intent, AbortFlag, Intent};

use op_editor_host_core::design::DesignSession;
use op_host_services::chat_canvas_tools::chat_tool_channel;
use op_host_services::chat_provider_llm::ChatProviderLlmClient;
use op_host_services::chat_system_prompt::{
    build_agent_system_prompt, build_chat_system_prompt, chat_history_from_transcript,
};

use super::ChatSession;

#[path = "chat_design_request.rs"]
mod chat_design_request;
use chat_design_request::build_design_request;

// The turn's staged attachments, drained once by `launch_if_pending` and
// handed to every route — see the module docs (issue #64).
#[path = "chat_turn_attachments.rs"]
mod chat_turn_attachments;
use chat_turn_attachments::TurnAttachments;

/// Build the model-aware request while the live chat selection is still
/// attached, then take the narrowed worker snapshot.
///
/// `narrowed_snapshot` deliberately detaches `EditorState::chat`; reversing
/// these two operations therefore erases the selected builtin/ACP identity
/// and makes the orchestrator resolve `model=None` as Full tier.
///
/// `attachments` is the turn's already-drained set (see
/// [`TurnAttachments`]). This function must NOT drain the chat state itself:
/// doing so is what left the chat and modify routes of the same turn with an
/// empty list and lost the user's picture without a word (issue #64).
fn prepare_design_request_and_snapshot(
    host: &mut WidgetHostNative,
    prompt: String,
    append_context: Option<op_orchestrator::AppendContext>,
    attachments: &TurnAttachments,
) -> (op_orchestrator::DesignRequest, EditorState) {
    let mut request = build_design_request(
        prompt,
        host.editor_state(),
        append_context,
        attachments.for_design(),
    );
    // The reference channel is typed as an image, so a staged document cannot
    // ride it. Say so in the prompt rather than letting the planner describe a
    // file it never received.
    request.prompt.push_str(&attachments.design_omission_note());
    let initial_state =
        op_editor_core::request_snapshot::narrowed_snapshot(host.editor_state_mut());
    (request, initial_state)
}

/// Tell the person when the design turn about to run leaves staged files
/// behind. Only the routes this module picks synchronously can say it — the
/// same note rides the design prompt for the asynchronously classified CLI
/// route, where the UI thread does not yet know whether the design route is
/// the one that will run.
fn note_design_attachment_omissions(host: &mut WidgetHostNative, attachments: &TurnAttachments) {
    let Some(note) = attachments.design_omission_chat_note() else {
        return;
    };
    let chat = &mut host.editor_state_mut().chat;
    if let Some(msg) = chat
        .messages
        .iter_mut()
        .rev()
        .find(|m| m.role == op_editor_core::chat::ChatRole::User)
    {
        msg.content.push_str(&note);
        host.mark_editor_state_dirty();
    }
}

// Design-agent-loop helpers (flag gate + provider builder + turn launcher)
// split out at the 800-line cap; see module docs there.
#[path = "chat_session_launch_design.rs"]
pub(super) mod launch_design;
use launch_design::{launch_design_loop_turn, stamp_design_turn_scenario};

/// Drain `chat.pending_send` (raised by `ChatState::begin_send`) and
/// route it.
///
/// CLI (standard-mode) selections route through the TS three-way
/// classifier on a worker thread (`chat_intent::run_cli_turn`, GAP
/// #33): a `ChatSession` AND a `DesignSession` are parked up front,
/// and the worker drives whichever route the classification picks
/// (chat stream / DESIGN_MODIFY / new-design orchestrator), dropping
/// the other route's channels so its pump retires the unused session.
///
/// Builtin / ACP selections keep the established shell routing (TS
/// returns early for them too, into its agent loop): the keyword
/// intent gate (`op_orchestrator::classify_intent`) sends design
/// verbs to the orchestrator pipeline, builtin chat turns to the
/// tool-executing agent loop, and everything else to the plain
/// `ChatProvider` path. A send fired mid-turn replaces the in-flight
/// session — the old worker thread drains harmlessly once its channel
/// receiver drops.
///
/// Returns true when *any* turn was launched (caller redraws).
pub fn launch_if_pending(
    host: &mut WidgetHostNative,
    current_chat: &mut Option<ChatSession>,
    current_design: &mut Option<DesignSession>,
) -> bool {
    let Some(user_text) = host.editor_state_mut().chat.pending_send.take() else {
        return false;
    };
    // One drain per turn, before ANY route's request is built. The staged
    // attachments belong to the turn that is about to run, whichever route the
    // classifier picks, so every candidate request below reads this same value
    // and none of them may take from the chat state again (issue #64).
    let attachments = TurnAttachments::drain(host.editor_state_mut());
    host.mark_editor_state_dirty();
    let effective_user_text = resolve_turn_user_text(host.editor_state(), &user_text);
    // TS parity (ai-chat-handlers.ts:560-679): builtin / ACP entries
    // take their own early-return paths; ONLY external CLI providers
    // run the standard-mode classify → modify/new/chat pipeline.
    let is_builtin_or_acp = host
        .editor_state()
        .chat
        .selected_model_entry()
        .map(|entry| entry.builtin_provider_id.is_some() || entry.acp_agent_id().is_some())
        .unwrap_or(false);
    if !is_builtin_or_acp {
        if launch_cli_standard_turn(
            host,
            &effective_user_text,
            &attachments,
            current_chat,
            current_design,
        ) {
            return true;
        }
        // CLI transport construction failed — fall through to the
        // honest-error path below.
    } else if should_launch_direct_modify(host.editor_state(), &effective_user_text) {
        if launch_direct_modify_turn(
            host,
            &effective_user_text,
            &attachments,
            current_chat,
            current_design,
        ) {
            return true;
        }
    } else if !op_host_services::chat_intent::is_non_request_text(&effective_user_text)
        && matches!(classify_intent(&effective_user_text), Intent::Design)
    {
        // Record what this turn establishes the document to be BEFORE either
        // design route can clear the starter frame — once the page is empty
        // the "was it empty?" fact is gone. See `design_turn_scenario`.
        stamp_design_turn_scenario(host.editor_state_mut(), &effective_user_text);
        // Phase 2.3: When the design-agent-loop flag is ON and a built-in
        // provider is configured, run the agentic tool-loop with the 14-tool
        // design toolset instead of the orchestrator pipeline. Flag OFF falls
        // through to the orchestrator path below — byte-for-byte unchanged.
        if launch_design_loop_turn(
            host,
            effective_user_text.clone(),
            &attachments,
            current_chat,
            current_design,
        ) {
            return true;
        }
        // Orchestrator path — unchanged when flag is OFF or no built-in
        // design provider is available. The append context the TS tool path
        // detects (agent-tool-executor.ts:234) rides the request here.
        if let Some(provider) = provider_for_selected_model(host) {
            super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
            *current_chat = None;
            // Share one provider Arc between the design LLM and the
            // (flag-gated) Class-C vision validator so the real loop reuses
            // the user's selected auth/model.
            let provider_arc: Arc<dyn ChatProvider> = Arc::from(provider);
            let llm = ChatProviderLlmClient::new(provider_arc.clone())
                .with_model(selected_cli_model_id(host));
            if super::allow_ai_bulk_write(host)
                && clear_fresh_starter_frame_for_design(host.editor_state_mut())
            {
                host.mark_editor_state_dirty();
            }
            let append_context = op_host_services::chat_intent::detect_append_intent(
                host.editor_state(),
                &effective_user_text,
            );
            let (request, initial_state) = prepare_design_request_and_snapshot(
                host,
                effective_user_text.clone(),
                append_context,
                &attachments,
            );
            // The design route is the one that runs — say so when it had to
            // leave a staged file behind. (The asynchronously classified CLI
            // route cannot know yet, and only the model-facing note rides
            // there; see `note_design_attachment_omissions`.)
            note_design_attachment_omissions(host, &attachments);
            // Narrowed clone — this becomes the design worker's
            // `RemoteDocSink` mirror, which is only ever read through
            // `DocSink::state()` (`active_children` / `doc` / `components`).
            // See `op_editor_core::request_snapshot` for the field audit.
            // The request above must be built first because that snapshot
            // intentionally detaches chat/model-selection state.
            // Persist the request onto the turn's assistant bubble (already
            // pushed by `begin_send`) BEFORE it moves into the worker — the
            // manual per-subtask "Retry" button needs it to re-run a failed
            // section later (failed-subtask remediation, manual layer).
            stash_design_request_for_retry(host, &request);
            *current_design = Some(op_host_services::design_session::start(
                llm,
                request,
                initial_state,
                Some(provider_arc),
            ));
            return true;
        }
        // Design intent but the selected agent has no ChatProvider
        // bridge yet — fall through to the chat path so the unwired
        // agent error message lands in the assistant bubble.
    }
    // Taking the chat path — drop any in-flight design turn so its
    // worker's next `apply` returns false (channel dropped) and its
    // `Progress` deltas stop streaming into this turn's fresh bubble
    // (codex stop-gate: stale design session survived chat fallback,
    // kept overwriting the new bubble content + applying ack'd
    // EditorCommands long after the user moved on).
    *current_design = None;
    // Per-turn context (GAP #31): prior transcript turns, trimmed by
    // the TS sliding-window policy. `begin_send` already pushed this
    // turn's user message + empty assistant bubble — the transcript
    // mapper excludes both.
    let history = trim_chat_history(
        &chat_history_from_transcript(&host.editor_state().chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    // Builtin (API-key) providers run the tool-executing agent loop
    // (GAP #32): canvas tool defs ride the request and the UI thread
    // executes each call via the session's tool channel.
    if let Some((provider, tool_rx)) = builtin_provider_with_tools(host) {
        let system_prompt = build_agent_system_prompt(host.editor_state());
        // This builtin agent loop carries the full canvas toolset (`batch_design`
        // included), so a design request runs *here* for an API-key model like
        // glm-5.2 (experimental flag off → no design-agent loop, builtin → no CLI
        // provider). Reasoning models burn their whole budget on hidden `<think>`
        // and draw nothing with thinking left on (glm-5.2 measured thinking≈30k /
        // text=0 → empty Frame). Force it off for `thinking_disabled` models, same
        // as the design-agent loop. Resolved before the `&mut` borrow below.
        let thinking = launch_design::design_turn_thinking_mode(host);
        let chat = &mut host.editor_state_mut().chat;
        let effort = chat.effort_level;
        let req = ChatRequest {
            system_prompt,
            user_message: effective_user_text.clone(),
            history,
            max_output_tokens: 4096,
            thinking,
            effort,
            // The turn's staged attachments, drained once at the top of this
            // function. The tool-executing builtin loop cannot open a local
            // file, and says so in the prompt instead of accepting the picture
            // in silence (`chat_attachment::prompt_with_undelivered_attachments`).
            attachments: attachments.for_chat(),
            // Built-in entries carry their model inside the provider's
            // own config — see `selected_cli_model_id`.
            model: None,
        };
        super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
        *current_chat = Some(ChatSession::start_with_tools(provider, req, Some(tool_rx)));
        return true;
    }
    let Some(provider) = chat_provider_for_selected_model(host) else {
        // Selected agent has no `ChatProvider` bridge (all fixed CLI
        // agents are wired today, so this is a stale-index / not-ready
        // builtin/ACP guard). Surface that honestly in the assistant
        // bubble instead of silently running a different agent (codex
        // stop-gate: silent reroute to Claude misled the user about
        // which CLI answered).
        //
        // Drop any in-flight session FIRST — otherwise the next
        // `pump` keeps streaming the previous agent's deltas into
        // this fresh error bubble (codex stop-gate: stale session
        // overwrote the unwired-agent error text).
        super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
        *current_chat = None;
        let name = selected_provider_label(host);
        // The attachments are already out of `pending_attachments` (drained at
        // the top of this function), so this error is the only place left that
        // can tell the person they did not go anywhere (issue #64).
        let unsent = attachments.unsent_note();
        let chat = &mut host.editor_state_mut().chat;
        if let Some(msg) = chat.messages.last_mut() {
            msg.content = format!(
                "error: {name} chat is not available — no transport \
                 could be built for this selection. Pick another agent \
                 via the model chip.{unsent}"
            );
            // The turn is aborted — `begin_send` created this bubble
            // as `streaming`; clear it so the panel doesn't keep
            // animating a stream that will never arrive.
            msg.streaming = false;
        }
        host.mark_editor_state_dirty();
        // No session started; report the transcript change so the
        // caller repaints the error.
        return true;
    };
    // Thread the per-turn knobs the chat panel carries into the request;
    // the attachments were drained once at the top of this function.
    // Every turn now carries the context-rich chat system prompt (TS
    // buildChatSystemPrompt port) — CLI transports fold it (plus a
    // history digest) into their prompt string.
    let system_prompt = build_chat_system_prompt(host.editor_state(), &effective_user_text);
    let model = selected_cli_model_id(host);
    let chat = &mut host.editor_state_mut().chat;
    let thinking = chat.thinking_mode;
    let effort = chat.effort_level;
    let req = chat_turn_attachments::plain_chat_request(
        system_prompt,
        effective_user_text,
        history,
        thinking,
        effort,
        model,
        &attachments,
    );
    super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
    *current_chat = Some(ChatSession::start(provider, req));
    true
}

fn resolve_turn_user_text(state: &EditorState, user_text: &str) -> String {
    let history = trim_chat_history(
        &chat_history_from_transcript(&state.chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    op_host_services::chat_intent::resolve_retry_instruction(user_text, &history)
}

/// Persist `request` (JSON-encoded) onto the turn's assistant bubble —
/// `begin_send` already pushed the empty streaming bubble this write lands
/// on, at both design-turn launch sites (`launch_if_pending`'s builtin
/// branch, `launch_cli_standard_turn`'s CLI branch). Read back by
/// `design_session::launch_subtask_retry_if_pending` when the user clicks a
/// failed row's "Retry" icon (failed-subtask remediation, manual layer) —
/// see `ChatMessage::design_request_json_for_retry`.
///
/// `op-editor-core` cannot depend on `op-orchestrator`'s concrete
/// `DesignRequest` type (wrong dependency direction), hence the opaque JSON
/// string rather than a typed field. Serialization failure is silently
/// skipped: `DesignRequest` derives `Serialize` and always succeeds in
/// practice, and losing the retry affordance is strictly better than
/// panicking a design turn over it.
fn stash_design_request_for_retry(
    host: &mut WidgetHostNative,
    request: &op_orchestrator::DesignRequest,
) {
    let Ok(json) = serde_json::to_string(request) else {
        return;
    };
    if let Some(msg) = host.editor_state_mut().chat.messages.last_mut() {
        msg.design_request_json_for_retry = Some(json);
    }
}

fn should_launch_direct_modify(state: &EditorState, user_text: &str) -> bool {
    // A pristine "from-scratch" canvas holds only the blank starter frame:
    // there is nothing real to modify, so ANY design request on it is a NEW
    // design, never a modify — even when the prompt is a bare noun phrase the
    // new-screen gates don't recognize (measured: "Luxury webapp for managing
    // barbershop clients" on a fresh canvas fell into run_modify_turn → glm
    // flat-nodes → empty `"`). This is the desktop parity of
    // web_chat_standard's `page_children_empty => New`.
    if active_page_is_blank_starter_frame(state) {
        return false;
    }
    // A whole-screen draw request ("继续画一下 search 页面") must reach the
    // design pipeline's new-frame route, never get hijacked into editing the
    // existing frame in place — even when it also trips the modify classifier.
    if op_host_services::chat_intent::requests_new_whole_screen(user_text) {
        return false;
    }
    // Same intent, but section-add-blind: a full new-page spec that mentions
    // "section" ("Include a search section…") still reads as new, so a stray
    // selection can't drag it into modify (measured: a travel-app design with
    // a node selected fell into run_modify_turn → M3 flat-JSONL → empty).
    if op_host_services::chat_intent::has_new_screen_creation_signal(user_text) {
        return false;
    }
    let keyword_intent = op_host_services::chat_intent::classify_by_keywords(user_text);
    let selected_target_instruction = !state.selection.set.is_empty()
        && keyword_intent != op_host_services::chat_intent::DesignIntent::Chat;
    (keyword_intent == op_host_services::chat_intent::DesignIntent::Modify
        || selected_target_instruction)
        && op_host_services::chat_intent::build_modify_plan(state, user_text).is_some()
}

/// The selection-scoped `generateDesignModification` turn (builtin / ACP
/// selection with a modify-shaped prompt — the CLI selection routes the same
/// request through `launch_cli_standard_turn`'s plan instead).
///
/// `attachments` is the turn's already-drained set: an attached reference must
/// reach this turn as well, or "make it look like this picture" edits here
/// while the model sees nothing (issue #64). The transport reports the files
/// it cannot carry, and the plain-streaming built-in body inlines the rest.
fn launch_direct_modify_turn(
    host: &mut WidgetHostNative,
    user_text: &str,
    attachments: &TurnAttachments,
    current_chat: &mut Option<ChatSession>,
    current_design: &mut Option<DesignSession>,
) -> bool {
    let Some(provider) = provider_for_selected_model(host) else {
        return false;
    };
    let Some(plan) =
        op_host_services::chat_intent::build_modify_plan(host.editor_state(), user_text)
    else {
        return false;
    };
    let target_frame_ids = plan.target_frame_ids.clone();
    // Structured-JSON turn: reasoning models (MiniMax-M3, GLM-5.x) burn the
    // whole output budget inside <think> and emit zero nodes (measured: an M3
    // modify turn died in analysis prose). Same policy as the orchestrator's
    // design subtasks.
    let request = chat_turn_attachments::modify_request(
        plan,
        selected_cli_model_id(host),
        op_ai::chat_provider::ThinkingMode::Disabled,
        attachments,
    );
    let (chat_tx, chat_rx) = mpsc::channel::<ChatDelta>();
    let (executor, tool_rx) = chat_tool_channel();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *current_design = None;
    super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
    *current_chat = Some(ChatSession::from_channels_with_cancel(
        chat_rx,
        Some(tool_rx),
        Arc::clone(&cancel),
    ));
    let spawned = thread::Builder::new()
        .name("op-chat-modify".into())
        .spawn(move || {
            op_host_services::chat_intent::run_modify_turn_cancellable(
                provider.as_ref(),
                request,
                &chat_tx,
                &executor,
                target_frame_ids,
                cancel,
            );
        });
    if let Err(err) = spawned {
        // Worker never started — un-park the session and fall through
        // to the honest-error path instead of crashing the UI thread.
        eprintln!("openpencil-desktop: spawn op-chat-modify thread failed: {err}");
        *current_chat = None;
        return false;
    }
    true
}

/// Launch a CLI standard-mode turn (GAP #33): pre-build every route's
/// inputs + channels on the UI thread, park a `ChatSession` and a
/// `DesignSession` (mirroring `from_channels` docs), and hand the
/// route decision to `chat_intent::run_cli_turn` on a worker thread —
/// classification needs an LLM round-trip and must not block the UI.
///
/// Returns false when any transport fails to build (caller falls to
/// the honest-error path).
fn launch_cli_standard_turn(
    host: &mut WidgetHostNative,
    user_text: &str,
    attachments: &TurnAttachments,
    current_chat: &mut Option<ChatSession>,
    current_design: &mut Option<DesignSession>,
) -> bool {
    // All three transports up front: classification + design run without
    // transcript history (TS classify/generate calls never join the chat
    // conversation); the chat route receives only its owning tab's history.
    let (Some(classify_provider), Some(chat_provider), Some(design_provider)) = (
        provider_for_selected_model(host),
        chat_provider_for_selected_model(host),
        provider_for_selected_model(host),
    ) else {
        return false;
    };
    let model = selected_cli_model_id(host);

    // Rust-only starter handling: classification resolves async on
    // the worker, which cannot mutate the doc — so the pristine
    // starter sample clears eagerly when the keyword pre-gate already
    // reads design intent. A keyword-design turn the LLM later
    // classifies as chat loses only the untouched starter sample.
    if matches!(classify_intent(user_text), Intent::Design) {
        // Same ordering rule as the builtin route: the scenario fact is read
        // while the page still shows what the user had.
        stamp_design_turn_scenario(host.editor_state_mut(), user_text);
        if super::allow_ai_bulk_write(host)
            && clear_fresh_starter_frame_for_design(host.editor_state_mut())
        {
            host.mark_editor_state_dirty();
        }
    }

    // Route inputs, post-clear (TS reads the live doc at this point).
    let state = host.editor_state();
    let page_children_empty = state.active_children().is_empty();
    // TS `hasSelection` folds into build_modify_plan's target pick.
    let history = trim_chat_history(
        &chat_history_from_transcript(&state.chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    let system_prompt = build_chat_system_prompt(state, user_text);
    let modify_plan = op_host_services::chat_intent::build_modify_plan(state, user_text);
    let append_context = op_host_services::chat_intent::detect_append_intent(state, user_text);
    let (design_request, initial_state) = prepare_design_request_and_snapshot(
        host,
        user_text.to_string(),
        append_context,
        attachments,
    );
    // Narrowed clone — `CliTurnPlan::initial_state` ends up as the design
    // worker's `RemoteDocSink` mirror, read only through `DocSink::state()`.
    // See `op_editor_core::request_snapshot` for the field audit. Preparation
    // takes the mutable borrow, so it must come after the last read of `state`;
    // it builds the model-aware request before detaching chat for the snapshot.
    // Same stash as the builtin/design-intent path above — this turn may or
    // may not actually classify as `DesignIntent::New` on the worker (the
    // classifier runs async), but setting it unconditionally is harmless:
    // the manual "Retry" button only ever reads it back alongside a
    // `failed_subtasks` entry, which nothing populates on a Chat/Modify
    // turn. `design_request` is moved into `CliTurnPlan` below (which itself
    // moves into the worker thread), so this must run BEFORE that move.
    stash_design_request_for_retry(host, &design_request);

    let chat = &mut host.editor_state_mut().chat;
    let thinking = chat.thinking_mode;
    let effort = chat.effort_level;
    // Every route in the plan below carries the turn's staged attachments (see
    // `plain_chat_request` / `modify_request` / `prepare_design_request_and_
    // snapshot`): the classifier picks the route on the worker, AFTER these
    // requests are built, so a route left without them can only answer about a
    // picture it never received (issue #64).
    let chat_request = chat_turn_attachments::plain_chat_request(
        system_prompt,
        user_text.to_string(),
        history,
        thinking,
        effort,
        model.clone(),
        attachments,
    );
    let modify_request = modify_plan.map(|plan| {
        chat_turn_attachments::modify_request(
            plan,
            model.clone(),
            op_ai::chat_provider::ThinkingMode::Adaptive,
            attachments,
        )
    });

    // Channels for all three routes; the worker drops the unused
    // ones, whose pumps then retire their sessions.
    let (chat_tx, chat_rx) = mpsc::channel::<ChatDelta>();
    let (executor, tool_rx) = chat_tool_channel();
    let (delta_tx, delta_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let indicator_epoch = op_editor_core::agent_indicators::begin();
    let design_abort = AbortFlag::new();
    super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
    *current_chat = Some(ChatSession::from_channels_with_cancel(
        chat_rx,
        Some(tool_rx),
        design_abort.shared_atomic(),
    ));
    *current_design = Some(DesignSession::from_channels_with_epoch_and_abort(
        delta_rx,
        cmd_rx,
        indicator_epoch,
        design_abort.clone(),
    ));

    let plan = op_host_services::chat_intent::CliTurnPlan {
        user_text: user_text.to_string(),
        page_children_empty,
        classify_provider,
        chat_provider,
        design_provider,
        chat_request,
        modify_request,
        design_request,
        initial_state,
        indicator_epoch,
        abort: design_abort,
        model,
    };
    let spawned = thread::Builder::new()
        .name("op-chat-intent".into())
        .spawn(move || {
            op_host_services::chat_intent::run_cli_turn(plan, chat_tx, executor, delta_tx, cmd_tx);
        });
    if let Err(err) = spawned {
        // Worker never started — un-park both sessions, end the epoch
        // begun above, and fall to the honest-error path instead of
        // crashing the UI thread.
        eprintln!("openpencil-desktop: spawn op-chat-intent thread failed: {err}");
        *current_chat = None;
        *current_design = None;
        op_editor_core::agent_indicators::end_if_epoch(indicator_epoch);
        return false;
    }
    true
}

/// Drain a New Chat request raised by the widget layer.
///
/// The widget handler ([`crate::widget_host`] `AIChatHit::NewChat`) already
/// pushed the fresh tab via `ChatSessions::new_tab` — the active tab keeps its
/// history intact while the new tab starts blank. This drain only does the
/// host-side worker cleanup the widget layer cannot reach: drop any in-flight
/// workers (so stale deltas can't repopulate the previous tab's transcript)
/// and clear any provider-local chat state retained by older adapters.
///
/// Returns the index of the tab a still-running turn was bound to, if any, so
/// the caller can clear its `chat_running_tab` field (the run we just aborted
/// must no longer target any tab).
pub fn drain_new_chat_request(
    host: &mut WidgetHostNative,
    current_chat: &mut Option<ChatSession>,
    current_design: &mut Option<DesignSession>,
) -> bool {
    if !std::mem::take(&mut host.editor_state_mut().chat.pending_new_chat) {
        return false;
    }
    // Finalize-lifecycle invariant (0718-1-k3-1 postmortem): New Chat can
    // discard an in-flight, still-unfinalized design loop before `pump`'s
    // own poll-backstop ever gets a chance to see it (`app_handler.rs`
    // drains this BEFORE `chat_session::pump` each frame).
    super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
    // The fresh tab was already opened by the widget handler — do NOT push a
    // second one here (one "+" click == one new tab).
    *current_chat = None;
    *current_design = None;
    if let Some(epoch) = op_editor_core::agent_indicators::active_epoch() {
        op_editor_core::agent_indicators::end_if_epoch(epoch);
    }
    // Keep the provider reset fanout for adapter compatibility. Claude Code
    // and Copilot currently retain no process-global session: each new tab's
    // request-local history already starts the provider conversation cleanly.
    op_host_services::chat_claude::reset_claude_chat_session();
    op_host_services::chat_copilot::reset_copilot_chat_session();
    host.mark_editor_state_dirty();
    true
}

/// Drain a Stop request raised by the widget layer. The transcript
/// has already had its streaming flags cleared; this only drops the
/// in-flight workers so stale deltas cannot append after cancellation.
pub fn drain_stop_request(
    host: &mut WidgetHostNative,
    current_chat: &mut Option<ChatSession>,
    current_design: &mut Option<DesignSession>,
    running_tab: Option<usize>,
) -> bool {
    if !std::mem::take(&mut host.editor_state_mut().chat.pending_stop_chat) {
        return false;
    }
    // Finalize-lifecycle invariant (0718-1-k3-1 postmortem) — see
    // `drain_new_chat_request`'s matching comment above.
    super::finalize_design_session_if_needed(host, current_chat, "teardown-backstop");
    if let Some(session) = current_design.as_ref() {
        session.abort();
        crate::design_session::stop_design_transcript(host, running_tab);
    }
    *current_chat = None;
    *current_design = None;
    if let Some(epoch) = op_editor_core::agent_indicators::active_epoch() {
        op_editor_core::agent_indicators::end_if_epoch(epoch);
    }
    host.mark_editor_state_dirty();
    true
}

pub(crate) fn clear_fresh_starter_frame_for_design(state: &mut EditorState) -> bool {
    if !active_page_is_blank_starter_frame(state) {
        return false;
    }
    // Keep a visual ghost of the starter at its exact rect: the document
    // node is gone (the pipeline must see an empty canvas), but the canvas
    // keeps painting the frame until the generated design's sized root
    // lands — sending a prompt never flashes an empty artboard.
    if let Some(only) = state.active_children().first() {
        use op_editor_core::PenNodeExt;
        let base = only.base();
        let (w, h) = (
            only.width_px().unwrap_or(1200.0),
            only.height_px().unwrap_or(800.0),
        );
        state.editor_ui.starter_ghost = Some([
            base.x.unwrap_or(0.0) as f32,
            base.y.unwrap_or(0.0) as f32,
            w as f32,
            h as f32,
        ]);
    }
    state.active_children_mut().clear();
    state.clear_selection();
    // Raw `active_children_mut()` bypasses the command/history path, so
    // bump the document revision explicitly. Without it the layer-panel
    // row cache (keyed on `document_revision()`) keeps painting the
    // now-deleted starter "Frame" row, and save-dirty tracking stays wrong.
    state.mark_document_changed();
    true
}

/// Drop the starter ghost once it has served its purpose: the generated
/// design's root landed (any top-level node exists again), or the turn is
/// over with nothing produced. Returns true when the ghost was cleared.
pub(crate) fn reconcile_starter_ghost(state: &mut EditorState, any_session_running: bool) -> bool {
    if state.editor_ui.starter_ghost.is_none() {
        return false;
    }
    if !state.active_children().is_empty() || !any_session_running {
        state.editor_ui.starter_ghost = None;
        return true;
    }
    false
}

/// The Asset Center asks the same question before deciding whether bringing
/// a template in should replace the document, so the predicate itself lives
/// in `op_editor_core::blank_starter`.
pub(crate) fn active_page_is_blank_starter_frame(state: &EditorState) -> bool {
    op_editor_core::blank_starter::active_page_is_blank_starter(state)
}

#[path = "chat_session_launch_providers.rs"]
mod providers;
pub(crate) use providers::{
    builtin_provider_with_tools, provider_for_selected_model, selected_cli_model_id,
};
use providers::{chat_provider_for_selected_model, selected_provider_label};

#[cfg(test)]
#[path = "chat_session_launch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "chat_turn_attachment_wire_tests.rs"]
mod attachment_wire_tests;

#[cfg(test)]
#[path = "chat_session_launch_selection_tests.rs"]
mod selection_tests;
