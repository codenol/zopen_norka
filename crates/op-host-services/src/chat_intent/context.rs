//! Document-derived turn context: append-target detection, the design-md
//! auto-generation gate, the design-variable context block, and the
//! selection-scoped modify plan. Split out of `chat_intent.rs` to keep the
//! spine under the 800-line cap.

use super::*;

pub(super) type PenNode = jian_ops_schema::node::PenNode;

pub(super) fn is_frame(node: &PenNode) -> bool {
    matches!(node, PenNode::Frame(_))
}

pub(super) fn is_status_bar_like(node: &PenNode) -> bool {
    let name = node.base().name.as_deref().unwrap_or("");
    is_status_bar_like_text(&format!("{name} {}", node.id_str()))
}

pub(super) fn node_label(node: &PenNode) -> String {
    node.base()
        .name
        .clone()
        .unwrap_or_else(|| node.id_str().to_string())
}

/// TS `pickContentRoot` — prefer a child frame named like a content
/// root, else the page frame itself.
pub(super) fn pick_content_root(page: &PenNode) -> (&PenNode, Vec<String>) {
    let children: &[PenNode] = page.children().map(Vec::as_slice).unwrap_or(&[]);
    let content_frames: Vec<&PenNode> = children
        .iter()
        .filter(|n| is_frame(n) && !is_status_bar_like(n))
        .collect();

    const CONTENT_NAME: &[&str] = &["content", "main", "body", "root"];
    let candidate = content_frames.iter().find(|f| {
        let name = f.base().name.as_deref().unwrap_or("").to_lowercase();
        matches_any_word_phrase(&name, CONTENT_NAME)
    });
    if let Some(candidate) = candidate {
        let grand = candidate.children();
        let labels = grand
            .map(|kids| {
                kids.iter()
                    .filter(|n| is_frame(n) && !is_status_bar_like(n))
                    .map(node_label)
                    .collect()
            })
            .unwrap_or_default();
        return (candidate, labels);
    }

    (page, content_frames.iter().map(|n| node_label(n)).collect())
}

/// Detect append intent against the live editor state. Append is a targeted
/// mutation, so continuation wording alone never chooses an arbitrary canvas
/// frame. Exactly one selected Frame is required; callers otherwise stay on
/// the new-design/chat route.
pub fn detect_append_intent(state: &EditorState, prompt: &str) -> Option<AppendContext> {
    if prompt.trim().is_empty() {
        return None;
    }
    let lower = prompt.to_lowercase();
    let has_append = matches_any_word_phrase(&lower, APPEND_EN)
        || APPEND_CJK.iter().any(|k| prompt.contains(k))
        || matches_cjk_add_section(prompt);
    if !has_append {
        return None;
    }
    if is_new_screen_veto(prompt) {
        return None;
    }

    let [selected_id] = state.selection.set.as_slice() else {
        return None;
    };
    let page_frame = op_editor_core::walkers::find_node(state.active_children(), selected_id)?;
    if !is_frame(page_frame) {
        return None;
    }
    let page_has_content = page_frame
        .children()
        .is_some_and(|kids| kids.iter().any(|c| is_frame(c) && !is_status_bar_like(c)));
    if !page_has_content {
        return None;
    }

    let (target, section_labels) = pick_content_root(page_frame);
    let width = page_frame.width_px().unwrap_or(375.0);

    Some(AppendContext {
        target_parent_id: target.id_str().to_string(),
        target_width: target.width_px().unwrap_or(width),
        existing_section_labels: section_labels,
        is_mobile: width <= 480.0,
    })
}

/// Design generation should ask the selected LLM to extract a design.md from
/// the current canvas for named follow-on pages (Discover / Orders / Profile,
/// etc.). A document-bound design.md wins, and append mode keeps its
/// append-specific context instead of creating a new sibling screen.
pub fn should_auto_generate_design_md(
    state: &EditorState,
    prompt: &str,
    append_context: Option<&AppendContext>,
) -> bool {
    state.doc.design_md.is_none()
        && append_context.is_none()
        && !state.active_children().is_empty()
        && (is_named_follow_on_screen(prompt) || requests_new_whole_screen(prompt))
}

// ---------------------------------------------------------------------------
// Modification plan — port of generateDesignModification's inputs
// ---------------------------------------------------------------------------

/// TS `buildVariableContext` (design-generator.ts:43-72). `None` when
/// the document has no variables. BTreeMap iteration is sorted where
/// TS uses insertion order — content is identical, ordering may not be.
pub fn build_variable_context(state: &EditorState) -> Option<String> {
    let vars = state.doc.variables.as_ref().filter(|v| !v.is_empty())?;
    let mut lines: Vec<String> = vec![
        "DOCUMENT VARIABLES (use \"$name\" to reference, e.g. fill color \"$color-1\"):".into(),
    ];
    for (name, def) in vars {
        let kind = variable_kind_label(&def.kind);
        match &def.value {
            jian_ops_schema::variable::VariableValue::Themed(values) => {
                let default_val = values
                    .first()
                    .map(|v| scalar_display(&v.value))
                    .unwrap_or_else(|| "?".into());
                lines.push(format!("  - {name} ({kind}): {default_val} [themed]"));
            }
            jian_ops_schema::variable::VariableValue::Scalar(value) => {
                lines.push(format!("  - {name} ({kind}): {}", scalar_display(value)));
            }
        }
    }
    if let Some(themes) = state.doc.themes.as_ref().filter(|t| !t.is_empty()) {
        let summary = themes
            .iter()
            .map(|(axis, values)| format!("{axis}: [{}]", values.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!("Themes: {summary}"));
    }
    Some(lines.join("\n"))
}

pub(super) fn variable_kind_label(kind: &jian_ops_schema::variable::VariableKind) -> &'static str {
    use jian_ops_schema::variable::VariableKind;
    match kind {
        VariableKind::Color => "color",
        VariableKind::Number => "number",
        VariableKind::String => "string",
        VariableKind::Boolean => "boolean",
    }
}

/// JS template-literal rendering of a variable scalar.
pub(super) fn scalar_display(value: &jian_ops_schema::variable::VariableScalar) -> String {
    use jian_ops_schema::variable::VariableScalar;
    match value {
        VariableScalar::Bool(b) => b.to_string(),
        VariableScalar::Str(s) => s.clone(),
        VariableScalar::Num(n) => {
            if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
    }
}

/// Pre-built `generateDesignModification` request inputs.
/// What the host placed before this turn, for the prompt's framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeBaseHint {
    pub recipe_id: String,
    pub name: String,
}

pub struct ModifyPlan {
    /// `CONTEXT NODES + INSTRUCTION (+ variable context)` user message.
    pub user_message: String,
    /// Maintenance skills (+ design-md style policy) system prompt.
    pub system_prompt: String,
    /// Immutable write scope captured when the turn starts.
    pub target_frame_ids: Vec<String>,
    /// True when the host just placed a recipe and this turn rewrites its
    /// sample content, which needs more reply room than a small edit.
    pub rewrites_a_placed_recipe: bool,
}

pub(super) fn strip_base64_data_uris(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) if s.starts_with("data:") && s.contains(";base64,") => {
            *s = "<image>".to_string();
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_base64_data_uris(item);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                strip_base64_data_uris(item);
            }
        }
        _ => {}
    }
}

pub(super) struct ModifyNodeParse {
    pub(super) nodes: Vec<crate::chat_canvas_tools::DesignModificationOp>,
    pub(super) diagnostic: Option<String>,
}

pub(super) fn parse_modify_response(full_response: &str) -> ModifyNodeParse {
    if full_response.trim().is_empty() {
        return ModifyNodeParse {
            nodes: Vec::new(),
            diagnostic: Some("the model returned no text".into()),
        };
    }

    let script = op_mcp::script_runner::run_script_to_program(full_response);
    let nodes: Vec<crate::chat_canvas_tools::DesignModificationOp> = script
        .as_ref()
        .ok()
        .map(|program| {
            op_mcp::parse_program_objects(program)
                .into_iter()
                .map(|(parent, mut node)| {
                    // This route extracts the script's node objects itself, so
                    // it owes them the same shape normalization the program-DSL
                    // path runs before deserializing: a name the catalogue
                    // documents as a ROLE in the `type` slot (`divider`,
                    // issue #206) is translated here. Left as authored, the
                    // unknown variant travels through the scope checks and into
                    // the applied-json delta the transcript echoes; the insert
                    // tool would repair it later (`insert_node_data` runs the
                    // same normalizer), but nothing downstream of the parse
                    // should have to.
                    op_mcp::normalize_generated_node_shape(&mut node);
                    op_orchestrator::parse::normalize_generated_node_json(&mut node);
                    (parent, node)
                })
                .collect()
        })
        .unwrap_or_default();
    if !nodes.is_empty() {
        return ModifyNodeParse {
            nodes,
            diagnostic: None,
        };
    }

    match op_orchestrator::parse::parse_nodes(full_response) {
        Ok(nodes) if !nodes.is_empty() => ModifyNodeParse {
            nodes: nodes
                .into_iter()
                .map(|node| {
                    (
                        "null".to_string(),
                        serde_json::to_value(node).unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect(),
            diagnostic: None,
        },
        parsed => {
            let script_detail = match script {
                Ok(_) => "response contained no I(...) operations".to_string(),
                Err(_) => "response was not valid modification JavaScript".to_string(),
            };
            let node_detail = match parsed {
                Ok(_) => "node response contained no nodes".to_string(),
                Err(_) => "response was not valid node JSON".to_string(),
            };
            ModifyNodeParse {
                nodes: Vec::new(),
                diagnostic: Some(format!("{script_detail}; {node_detail}")),
            }
        }
    }
}

pub(crate) fn parse_modify_nodes(
    full_response: &str,
) -> Vec<crate::chat_canvas_tools::DesignModificationOp> {
    parse_modify_response(full_response).nodes
}

/// Build a modification plan for explicitly selected Frames. Direct mutation
/// is never inferred from the last canvas node: when selection is empty,
/// stale, or includes a non-Frame, the caller must degrade to a non-modifying
/// route.
pub(super) fn selected_frame_ids(state: &EditorState) -> Option<Vec<String>> {
    let children = state.active_children();
    if state.selection.set.is_empty() {
        return None;
    }
    let mut ids = Vec::with_capacity(state.selection.set.len());
    for id in &state.selection.set {
        let node = op_editor_core::walkers::find_node(children, id)?;
        if !is_frame(node) {
            return None;
        }
        ids.push(node.id_str().to_string());
    }
    Some(ids)
}

pub fn build_modify_plan(state: &EditorState, instruction: &str) -> Option<ModifyPlan> {
    build_modify_plan_with(state, instruction, None)
}

/// As [`build_modify_plan`], but told which recipe the host just placed.
///
/// A freshly placed recipe is full of the kit's placeholder content, and the
/// user's message ("список коммутаторов") reads as a request to *build* that
/// screen rather than to fill it in. Naming the base and the placeholder duty
/// explicitly is what makes the turn a rewrite of the sample data instead of
/// a coin flip on whether the model notices the columns say something else.
pub fn build_modify_plan_with(
    state: &EditorState,
    instruction: &str,
    recipe_base: Option<&RecipeBaseHint>,
) -> Option<ModifyPlan> {
    let children = state.active_children();
    let target_frame_ids = selected_frame_ids(state)?;
    let targets = target_frame_ids
        .iter()
        .map(|id| op_editor_core::walkers::find_node(children, &op_editor_core::NodeId::new(id)))
        .collect::<Option<Vec<_>>>()?;

    let mut context = serde_json::to_value(&targets).ok()?;
    strip_base64_data_uris(&mut context);
    let context_json = serde_json::to_string(&context).ok()?;
    let mut user_message = String::new();
    if let Some(base) = recipe_base {
        // The placed screen, by id. Naming it is the point of this paragraph: a
        // root-level statement carrying *this* id is the one reply shape that
        // both composes a screen and leaves a single screen on the page, so the
        // turn has to ask for that shape by name (issue #197).
        let screen_id = target_frame_ids
            .first()
            .map(String::as_str)
            .unwrap_or("the selected screen");
        user_message.push_str(&format!(
            "THIS SCREEN WAS JUST PLACED FROM RECIPE `{}` ({}) AND IS SELECTED ABOVE AS `{}`.\n\
             Everything it shows is the kit's placeholder sample, not this product's content: \
             the columns name something else and the table repeats one sample row.\n\
             WHAT TO RETURN: the whole screen, as one root-level statement carrying its id —\n\
             I(null, {{id:\"{}\", type:\"frame\", name:\"…\", width:1440, height:850, children:[…]}})\n\
             That statement is this turn's deliverable: the complete screen the request \
             describes — retitled for this product, with a distinct realistic row for every row \
             this list needs, and the optional blocks (filter strip, pagination strip, \
             column-settings button) kept or dropped as the request asks.\n\
             WRITE IT SMALL. This reply has room for a screen of roughly a hundred nodes; a \
             longer one is cut off mid-sentence and lands as a fragment, which is worse than a \
             modest screen that is whole. So do NOT re-emit the sample's node tree from CONTEXT \
             NODES — those nodes show you which blocks and kit components exist, not what to \
             write. Write the screen itself: the shell frame, the blocks this request names, and \
             the rows.\n\
             SMALL is about the sample ROWS, not the shell. Every placed shell block survives \
             in your reply exactly as placed unless the request names it for removal: the \
             sidebar's minibar icon rail, the menu (with the Active variant on the item for \
             this screen's section, Default on the rest), the breadcrumbs, the info message, \
             the table chrome, and the footer's pagination INCLUDING its page-size selector. \
             Dropping one of them is a wrong screen, not a smaller one.\n\
             WHAT NOT TO RETURN: statements naming a node *inside* the screen. \
             I(null, {{id:\"<an id below {}\", type:\"frame\", children:[…]}}) replaces that one \
             node and leaves the rest of the sample standing, so a reply built only of those has \
             not answered the request. Do not re-emit the sample rows, and do not add a second \
             screen beside this one.\n\n",
            base.recipe_id, base.name, screen_id, screen_id, screen_id
        ));
    }
    user_message.push_str(&format!(
        "CONTEXT NODES:\n{context_json}\n\nINSTRUCTION:\n{instruction}"
    ));
    if let Some(var_context) = build_variable_context(state) {
        user_message.push_str("\n\n");
        user_message.push_str(&var_context);
    }

    // Maintenance-phase skills (TS resolveSkills('maintenance', …)).
    let has_variables = state
        .doc
        .variables
        .as_ref()
        .is_some_and(|vars| !vars.is_empty());
    let mut options = op_ai_skills::ResolveOptions::default();
    options
        .flags
        .insert("hasVariables".to_string(), has_variables);
    options
        .flags
        .insert("hasDesignMd".to_string(), state.doc.design_md.is_some());
    let ctx = op_ai_skills::resolve_skills(op_ai_skills::Phase::Maintenance, instruction, &options);
    let mut system_prompt = ctx
        .skills
        .iter()
        .map(|s| s.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    // The turn's own rule list: the document's saved rules, with the placement
    // rule at the head when the host just placed a recipe (issue #207).
    //
    // A recipe turn is *this* route, not the new-design one: the placement runs
    // before classification, it selects the base, and a selected Frame is what
    // forces `DesignIntent::Modify`. The `doc:recipe-base` rule used to be
    // built for such a turn and inserted only on the new-design route, so the
    // one instruction that says "this screen is already on the page, adapt it,
    // do not rebuild its shell" never reached the prompt the turn sends —
    // measured absent from the captured request, 0 occurrences.
    //
    // Built by the recipe module, not copied here: one rule, one wording, and
    // the new-design route inserts the same value into its `DesignRequest`
    // (issue #207). Both routes insert at index 0, so the order the model reads
    // is the same on either.
    let mut rules: Vec<jian_ops_schema::DesignRule> =
        op_editor_core::effective_design_rules(state.doc.design_md.as_ref())
            .into_iter()
            .map(|entry| entry.rule)
            .collect();
    if let Some(base) = recipe_base {
        // The placed root is the selection — `place_recipe_base` sets a single
        // selection on it — so the head of `target_frame_ids` is the node the
        // rule must name, and the node the user message above already names.
        let placed = target_frame_ids.first().and_then(|node_id| {
            crate::web_chat_standard::kit_recipe(&base.recipe_id)
                .map(|recipe| (recipe, op_editor_core::NodeId::new(node_id)))
        });
        if let Some((recipe, node_id)) = placed {
            rules.insert(
                0,
                crate::web_chat_standard::recipe_base_rule(recipe, &node_id),
            );
        }
    }
    let rules_policy = op_editor_core::build_design_rules_policy(&rules);
    if !rules_policy.is_empty() {
        system_prompt.push_str("\n\nSESSION RULES (follow these EXACTLY):\n");
        system_prompt.push_str(&rules_policy);
    }

    Some(ModifyPlan {
        user_message,
        system_prompt,
        target_frame_ids,
        rewrites_a_placed_recipe: recipe_base.is_some(),
    })
}
