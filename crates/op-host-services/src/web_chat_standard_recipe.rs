//! The recipe a turn stands on, and the rules that read a modification reply.
//!
//! Pure code motion out of `web_chat_standard.rs` at the 800-line cap: the
//! functions below are byte-for-byte the ones that lived there, in the same
//! order, reasoning comments included. They reach the spine's helpers and the
//! shared imports through `use super::*`. The spine re-exports them, so
//! `crate::web_chat_standard::composes_new_screen` and the test modules' bare
//! names still resolve.

use super::*;

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
pub(super) fn recipe_base_to_place<'a>(
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
pub(super) fn kit_recipe(
    recipe_id: &str,
) -> Option<&'static op_editor_core::kit_manifest::KitRecipe> {
    op_editor_core::session_kit()
        .recipes
        .iter()
        .find(|recipe| recipe.id == recipe_id)
}

/// The `doc:recipe-base` rule: the recipe is on the page, it is what this turn
/// is based on, adapt it instead of composing it again.
pub(super) fn recipe_base_rule(
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
pub(super) fn reference_evidence(
    req: &WebStandardTurnRequest,
) -> op_editor_core::ReferenceEvidence {
    op_editor_core::ReferenceEvidence::of_attachments(req.attachments.iter().any(|a| a.is_image()))
}

/// Place the recipe this request asks for, before anything is classified.
///
/// Returns the placed root's id. The selection lands on it, so the turn that
/// follows is a *modification* of an existing screen rather than a request to
/// compose one — which is the difference between "adapt the recipe" as an
/// instruction the model may skip and as the shape of the turn itself.
pub(super) fn place_selected_recipe(
    user_message: &str,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> Option<(String, op_editor_core::NodeId)> {
    let recipe = op_editor_core::select_recipe(user_message, op_editor_core::session_kit())?;
    place_recipe_base(recipe, user_message, state, hub, write_barrier)
        .map(|node_id| (recipe.id.clone(), node_id))
}

/// Places `recipe` as the page's base and returns the root it created.
///
/// **Every** write this route makes goes through the collab gate, takes its own
/// instant of write admission and advances the version the browser polls — and
/// this function is the only place a recipe is cloned onto the page, so the
/// fallback placement in `stream_new_design_route` cannot skip any of the three
/// by re-implementing the write (issue #199). It re-implemented it: a bare
/// `instantiate_component` under the state lock, no gate, no admission, no
/// version bump — reachable exactly when the pre-classification placement
/// refused, which is the case where the write is least welcome.
pub(super) fn place_recipe_base(
    recipe: &op_editor_core::kit_manifest::KitRecipe,
    user_message: &str,
    state: &Mutex<WebCanvasState>,
    hub: &SseHub,
    write_barrier: Option<&crate::web_canvas_server::WriteBarrier>,
) -> Option<op_editor_core::NodeId> {
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
    Some(node_id)
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
pub(super) fn split_composed_screens(
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
pub(super) fn insert_composed_screens(
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
