//! The three route bodies a standard turn dispatches to, and the rule that picks
//! between them.
//!
//! Pure code motion out of `web_chat_standard.rs` at the 800-line cap: the
//! functions below are byte-for-byte the ones that lived there, in the same
//! order, reasoning comments included. They reach the spine's helpers, the
//! shared imports and the events module through `use super::*`, so every call
//! site reads exactly as it did before the move. The spine re-exports them, so
//! `crate::web_chat_standard::stream_modify_route` and the test modules' bare
//! names still resolve.

use super::*;
use crate::web_chat_standard::recipe::{
    kit_recipe, place_recipe_base, recipe_base_to_place, truncation_of, Truncation,
};

/// What the modify route tells the user when the reply it was about to apply
/// ran out before it finished (issue #205).
///
/// Refused rather than applied-and-annotated, and the applier is why: this route
/// replaces whole subtrees of the captured frames, so a tree parsed out of half
/// a statement is not a smaller version of the request — it is a *wrong* screen.
/// The measured fragment put the table's rows beside the table instead of inside
/// it (54-node `Table/Default` whose only child is its `Header`, with the 60
/// `Rows` nodes as its sibling). The canvas is left alone and the user is told
/// what happened and what to do about it.
fn cut_short_refusal(truncation: Truncation) -> String {
    let why = match truncation {
        Truncation::OutputBudget => {
            "The model ran out of output budget before it finished the screen's last statement, \
             so half a statement was all there was."
        }
        Truncation::UnterminatedStatement => {
            "The model's reply stops mid-statement — its last statement never closes."
        }
    };
    format!(
        "\n\n⚠ Nothing was applied: {why} A partial tree is not a smaller version of the screen \
         you asked for — it lands the pieces in the wrong places — so the canvas is exactly as \
         you left it rather than holding a broken screen. Ask for the screen in smaller parts, \
         or narrow what this turn has to rewrite."
    )
}

pub(super) fn resolve_standard_route(
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

pub(super) fn stream_chat_route<W: Write>(
    out: &mut W,
    req: &WebStandardTurnRequest,
    state: &EditorState,
    provider: &dyn ChatProvider,
    model: Option<String>,
    target: CanvasWriteTarget<'_>,
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
    // The reply is kept as it streams so the terminal `Done` can be read against
    // it. This route writes nothing to the document, so a cut reply costs no
    // nodes — but it still ends a turn that says `done` with an answer that
    // stops mid-sentence, and the three chat-route replies of the #205 corpus
    // are exactly that: 16 384 deltas, the request's whole output budget, ending
    // on `"width": "fill_container",` or `"fontWeight`. The user is told.
    let mut reply = String::new();
    let mut stop_reason_seen: Option<StopReason> = None;
    for delta in provider.send(chat_req) {
        if let ChatDelta::TextDelta(text) = &delta {
            reply.push_str(text);
        }
        if let ChatDelta::Done { stop_reason } = &delta {
            let stop_reason = *stop_reason;
            stop_reason_seen = Some(stop_reason);
            if let Some(truncation) = truncation_of(&reply, Some(stop_reason)) {
                write_delta_event(out, &chat_cut_short_notice(truncation))?;
            }
            out.write_all(
                crate::ai_proxy::delta_to_sse(&ChatDelta::Done { stop_reason }).as_bytes(),
            )?;
            out.flush()?;
            break;
        }
        out.write_all(crate::ai_proxy::delta_to_sse(&delta).as_bytes())?;
        out.flush()?;
        if matches!(delta, ChatDelta::Error(_)) {
            break;
        }
    }
    // A route that talks can still be answered with a SCREEN: the classifier
    // picks "chat" when its own model call times out or reads the prompt as
    // conversation, and the model then answers the design request anyway (issue
    // #215: 2 of 24 corpus turns — a complete payload, streamed as text, `done`
    // reported, the document untouched). A reply that composes screens is
    // applied here, whatever route it arrived on.
    apply_composed_reply_from_chat(out, state, &reply, stop_reason_seen, target)?;
    Ok(())
}

/// Apply the screens a talking route's reply composed (issue #215).
///
/// Deliberately narrow: only ops that are a screen of their own — a root-level
/// node with an id the document does not have ([`split_composed_screens`]) — are
/// applied. A conversational answer that happens to quote a JSON snippet stays
/// a conversational answer.
fn apply_composed_reply_from_chat<W: Write>(
    out: &mut W,
    snapshot: &EditorState,
    reply: &str,
    stop_reason: Option<StopReason>,
    target: CanvasWriteTarget<'_>,
) -> std::io::Result<()> {
    // A cut reply is not a screen (#205): the same refusal the modify route
    // makes, so a half-written tree cannot become canvas content just because it
    // arrived on the talking route.
    if truncation_of(reply, stop_reason).is_some() {
        return Ok(());
    }
    let nodes = crate::chat_intent::parse_modify_nodes(reply);
    if nodes.is_empty() {
        return Ok(());
    }
    let (screens, _) = split_composed_screens(snapshot, nodes, &[]);
    if screens.is_empty() {
        return Ok(());
    }

    let (applied, tick, starter_children) = {
        let mut guard = target
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let refused = guard
            .gate_daemon_mutation(
                op_editor_core::CollabGateAction::Document(
                    op_editor_core::CollabDocumentMutation::NodePropertyBatch,
                ),
                op_editor_core::CollabEditSource::Ai,
            )
            .is_err()
            || admit_document_write(target.write_barrier).is_err();
        if refused {
            // Nothing is applied, so nothing is taken away either.
            (0, None, None)
        } else {
            // The blank starter frame goes only now that there IS a screen to
            // put beside it — clearing it up front is what left an empty page
            // when the turn then drew nothing (#216). Taken first so the frame
            // can be handed back if the insert does not land.
            let starter_children = super::starter::blank_starter_children(&guard);
            if starter_children.is_some() {
                super::clear_fresh_starter_frame_for_design(&mut guard.editor);
            }
            let (applied, mutated) = insert_composed_screens(&mut guard.editor, &screens);
            let tick = if mutated {
                guard.version += 1;
                guard.note_daemon_draw();
                Some(guard.sse_tick())
            } else {
                None
            };
            (applied, tick, starter_children)
        }
    };
    if let Some(tick) = tick {
        target.hub.broadcast(tick);
    }
    if applied == 0 {
        // The insert did not land: give back the frame the clear took.
        super::starter::restore_starter_frame_if_page_empty(
            target.state,
            target.hub,
            starter_children.as_deref(),
        );
        return Ok(());
    }
    write_delta_event(out, "\n\n<!-- APPLIED -->")?;
    Ok(())
}

/// What the chat route appends when the answer it just streamed was cut off
/// (issue #205). The turn is not refused — nothing was applied either way — but
/// it may not pass a half sentence off as the whole answer.
fn chat_cut_short_notice(truncation: Truncation) -> String {
    let why = match truncation {
        Truncation::OutputBudget => "at the model's output budget",
        Truncation::UnterminatedStatement => "mid-statement",
    };
    format!(
        "\n\n⚠ This reply was cut off {why}: what is above is not the whole answer. Ask again, \
         or narrow the request so the answer fits."
    )
}

pub(super) fn stream_modify_route<W: Write>(
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
    // The provider's own verdict on why it stopped. Taken here because it is the
    // only place the route can see it, and because the decision below — apply
    // this tree, or refuse it — is not one a truncated reply may win (issue
    // #205).
    let mut stop_reason: Option<StopReason> = None;
    for delta in provider.send(request) {
        match delta {
            ChatDelta::TextDelta(s) => full_response.push_str(&s),
            ChatDelta::Thinking(_) | ChatDelta::ToolUse { .. } => {}
            ChatDelta::Error(msg) => {
                stream_error = Some(msg);
                break;
            }
            ChatDelta::Done {
                stop_reason: reason,
            } => {
                stop_reason = Some(reason);
                break;
            }
        }
    }

    let nodes = crate::chat_intent::parse_modify_nodes(&full_response);
    if !nodes.is_empty() {
        // #205: a partial tree used to be applied exactly like a whole one —
        // `<!-- APPLIED -->`, `done`, no hint that the screen was half written.
        // The reply is read before it is applied now, and an incomplete one is
        // refused with the reason, not applied as a screen.
        if let Some(truncation) = truncation_of(&full_response, stop_reason) {
            return write_error_event(out, &cut_short_refusal(truncation));
        }
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
                    // The daemon applied a composed/modified screen by itself:
                    // arm the guard and put the result in its file, since this
                    // is the end of the turn (issues #247/#248). The whole-doc
                    // path that also runs a tool loop arms it per command and
                    // writes the file below, after the turn.
                    guard.note_daemon_draw();
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

pub(super) fn stream_new_design_route<W: Write>(
    out: &mut W,
    req: WebStandardTurnRequest,
    snapshot: EditorState,
    provider: Box<dyn ChatProvider>,
    model: Option<String>,
    target: CanvasWriteTarget<'_>,
    base: PlacedBase,
) -> std::io::Result<()> {
    // Read before the fields move into the request below.
    let reference = base.reference;
    let placed_recipe = base.placed_recipe;
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
        // The pre-classification placement stood down (or never ran), so this
        // arm does the placement — through `place_recipe_base`, which is the
        // same door every other write on this route goes through: the collab
        // gate, its own instant of write admission, and the version bump the
        // browser polls. This arm used to call `instantiate_component` under
        // the state lock and nothing else, so the one placement that happens
        // when the first one refused was also the one write on this route that
        // bypassed all three (issue #199).
        None => recipe_base_to_place(&req.ai.user, reference, false).and_then(|recipe| {
            place_recipe_base(
                recipe,
                &req.ai.user,
                target.state,
                target.hub,
                target.write_barrier,
            )
            .map(|node_id| (recipe, node_id))
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
    // REAL providers by default: `vision_validation_enabled` reads
    // `OPENPENCIL_VISION_VALIDATION`, and an unset variable means ON
    // (`vision_validation_requested(None) == true`). The variable is the way to
    // turn the pipeline OFF, with `0` / `false` / `no` / `off` — so the no-op
    // stubs below are the opt-out path, not the default one. Read this before
    // assuming a turn ran without a vision round.
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
    // The turn is over. Its commands armed the turn-result guard as they were
    // applied; what is left is putting the result in the document's own file,
    // once, instead of on every command (issues #247/#248 — a file left holding
    // the starter is what "the AI does not build anything" looks like from
    // outside). Best effort: a brand-new account has no key yet.
    {
        let mut guard = target.state.lock().unwrap_or_else(|p| p.into_inner());
        guard.note_daemon_draw();
    }
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
