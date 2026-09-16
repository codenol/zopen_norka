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

pub(super) fn stream_new_design_route<W: Write>(
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
