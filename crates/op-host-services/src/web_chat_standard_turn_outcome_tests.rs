//! What a design turn *reported*, measured at the route (issues #182, #183,
//! #184, #189).
//!
//! These are the outcomes the baseline could not tell apart: a turn that
//! rewrote the screen the user asked for and a turn that only edited the
//! template the host had placed, both of which ended `done` with
//! `<!-- APPLIED -->` and an untouched kit screen on the canvas (#182); a
//! placement that changed 470 nodes without moving the version every poller
//! reads (#183); a third, empty screen left beside a two-screen design (#184);
//! and a New-route turn that cloned the recipe base a second time (#189).
//!
//! The assertions are about what the reply *says* and how many recipe roots the
//! document holds — "some nodes were applied" is what the defect already
//! satisfied.

use super::recipe_reference_tests::{
    daemon_session_document, design_turn, recipe_master_name, RECIPE_PROMPT,
};
use super::tests::ScriptedProvider;
use super::*;

/// The reply the #182 measurement caught, verbatim: the turn placed the ops
/// recipe, reported `done` + `<!-- APPLIED -->`, and emitted **no** root-level
/// statement — all six ops insert inside the placed tree, into table cells the
/// kit already owns. The user got the kit master back.
const TEMPLATE_ONLY_REPLY: &str = r#"<step title="Checking guidelines">Analyzing modification request...</step>
I("n267", {type:"frame", name:"Table/Cell/Status/Alternative", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/alternative"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/warning/background/default"}]},{type:"text", name:"label", content:"Обслуживание", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});
I("n396", {type:"frame", name:"Table/Cell/Status/Default", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/default"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/success/background/default"}]},{type:"text", name:"label", content:"В работе", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});
I("n439", {type:"frame", name:"Table/Cell/Status/Alternative", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/alternative"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/unavailable/background/default"}]},{type:"text", name:"label", content:"Недоступен", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});
I("n224", {type:"frame", name:"Table/Cell/Status/Default", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/default"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/success/background/default"}]},{type:"text", name:"label", content:"В работе", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});
I("n353", {type:"frame", name:"Table/Cell/Status/Alternative", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/alternative"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/warning/background/default"}]},{type:"text", name:"label", content:"Обслуживание", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});
I("n310", {type:"frame", name:"Table/Cell/Status/Default", width:125, height:44, layout:"none", clipContent:false, fill:[{type:"solid", color:"$table/row/background/default"}], children:[{type:"frame", name:"content", x:0, y:0, width:125, height:44, layout:"horizontal", gap:8, padding:[0,8], justifyContent:"start", alignItems:"center", clipContent:true, children:[{type:"frame", name:"indicator", width:8, height:8, layout:"none", cornerRadius:4, fill:[{type:"solid", color:"$indicator/success/background/default"}]},{type:"text", name:"label", content:"В работе", fontFamily:"Roboto", fontSize:13, fontWeight:400, lineHeight:1.1429, textAlign:"left", fill:[{type:"solid", color:"$table/text/primary"}]}]},{type:"rectangle", name:"rule", x:0, y:43, width:125, height:1, fill:[{type:"solid", color:"$table/row/border/default"}]}]});

<!-- APPLIED -->"#;

/// The control: one root-level statement that adds a node the document does not
/// hold, so the reply composes content of its own instead of retitling the
/// template.
const COMPOSING_REPLY: &str = r#"I(null, {id:"n900", type:"frame", name:"Server table", x:0, y:0, width:1200, height:600, children:[]});"#;

/// A canvas holding one placed recipe root whose cells the recorded reply
/// reaches into — the six parent ids it names are on it, so the ops really do
/// apply (`applied > 0` is what puts `<!-- APPLIED -->` on the wire).
fn placed_recipe_state() -> Mutex<WebCanvasState> {
    let cell = |id: &str, name: &str| {
        serde_json::json!({
            "id": id, "type": "frame", "name": name,
            "x": 0, "y": 0, "width": 125, "height": 44, "children": [],
        })
    };
    let doc_json = serde_json::json!({
        "version": "1.0.0",
        "children": [{
            "id": "n100", "type": "frame", "name": "Recipe/Ops servers screen",
            "x": 0, "y": 0, "width": 1440, "height": 850,
            "children": [
                cell("n267", "Table/Cell/Status/Alternative"),
                cell("n396", "Table/Cell/Status/Default"),
                cell("n439", "Table/Cell/Status/Alternative"),
                cell("n224", "Table/Cell/Status/Default"),
                cell("n353", "Table/Cell/Status/Alternative"),
                cell("n310", "Table/Cell/Status/Default"),
            ],
        }],
    })
    .to_string();
    let loaded = op_pen_loader::load_canonical(&doc_json).expect("fixture document loads");
    Mutex::new(WebCanvasState::new(
        EditorState::from_document(loaded.value),
        3100,
    ))
}

/// The plan a recipe turn runs with: the base was placed *for this turn*, and
/// the immutable write scope is that placed root.
fn placed_recipe_plan() -> crate::chat_intent::ModifyPlan {
    crate::chat_intent::ModifyPlan {
        rewrites_a_placed_recipe: true,
        user_message: "fill in the servers table".into(),
        system_prompt: String::new(),
        target_frame_ids: vec!["n100".to_string()],
    }
}

fn nodes_of(reply: &str) -> Vec<crate::chat_canvas_tools::DesignModificationOp> {
    let nodes = crate::chat_intent::parse_modify_nodes(reply);
    assert!(!nodes.is_empty(), "the fixture reply must parse: {reply}");
    nodes
}

fn target() -> Vec<String> {
    vec!["n100".to_string()]
}

fn sibling_frames(state: &EditorState) -> usize {
    state
        .active_children()
        .iter()
        .filter(|node| {
            use op_editor_core::PenNodeExt as _;
            node.base().name.as_deref() == Some("Recipe/Ops servers screen")
        })
        .count()
}

fn root_count(state: &EditorState) -> usize {
    state.active_children().len()
}

// ── the definition ──────────────────────────────────────────────────────────

#[test]
fn a_reply_that_only_reaches_inside_the_placed_recipe_is_not_a_new_screen() {
    let state = placed_recipe_state();
    let live = state.lock().unwrap_or_else(|p| p.into_inner());

    assert!(
        !composes_new_screen(&live.editor, &nodes_of(TEMPLATE_ONLY_REPLY), &target()),
        "six inserts into the recipe's own table cells rewrite nothing: the \
         screen the user asked for is not composed by them"
    );
}

#[test]
fn a_root_level_insert_is_a_screen_of_its_own() {
    let state = placed_recipe_state();
    let live = state.lock().unwrap_or_else(|p| p.into_inner());

    assert!(composes_new_screen(
        &live.editor,
        &nodes_of(COMPOSING_REPLY),
        &target()
    ));
}

#[test]
fn replacing_the_placed_root_is_a_screen_of_its_own() {
    // Measured on this route: the model answers "rewrite this screen" by
    // emitting the target root's own id at root level, which the applier
    // substitutes for the whole placed screen.
    let state = placed_recipe_state();
    let live = state.lock().unwrap_or_else(|p| p.into_inner());
    let reply = r#"I(null, {id:"n100", type:"frame", name:"Recipe/Ops servers screen", x:0, y:0, width:1440, height:850, children:[]});"#;

    assert!(composes_new_screen(
        &live.editor,
        &nodes_of(reply),
        &target()
    ));
}

#[test]
fn replacing_an_inner_node_is_not_a_screen_of_its_own() {
    let state = placed_recipe_state();
    let live = state.lock().unwrap_or_else(|p| p.into_inner());
    let reply = r#"I(null, {id:"n267", type:"frame", name:"Table/Cell/Status/Default", x:0, y:0, width:125, height:44, children:[]});"#;

    assert!(!composes_new_screen(
        &live.editor,
        &nodes_of(reply),
        &target()
    ));
}

#[test]
fn an_id_less_root_op_needs_the_single_target_the_applier_needs() {
    // `apply_design_modification` can only place an op that names no parent
    // when exactly one target frame was captured; with several it does nothing
    // at all, so it composed nothing either.
    let state = placed_recipe_state();
    let live = state.lock().unwrap_or_else(|p| p.into_inner());
    let reply =
        r#"I(null, {type:"text", name:"label", content:"Имя", x:0, y:0, width:40, height:16});"#;
    let nodes = nodes_of(reply);

    assert!(composes_new_screen(&live.editor, &nodes, &target()));
    assert!(!composes_new_screen(
        &live.editor,
        &nodes,
        &["n100".to_string(), "n101".to_string()]
    ));
}

// ── the report ──────────────────────────────────────────────────────────────

#[test]
fn a_recipe_turn_that_only_edited_the_template_says_so() {
    let state = placed_recipe_state();
    let provider = ScriptedProvider {
        response: TEMPLATE_ONLY_REPLY.to_string(),
    };
    let mut out = Vec::new();

    stream_modify_route(
        &mut out,
        placed_recipe_plan(),
        &provider,
        None,
        &state,
        &SseHub::default(),
        None,
    )
    .expect("the modify turn answers");

    let streamed = String::from_utf8(out).expect("utf8 sse");
    assert!(
        streamed.contains("No new screen was composed"),
        "a recipe turn that composed nothing must say so: {streamed}"
    );
    assert!(
        streamed.contains("edited the template the host placed"),
        "the sentence has to name what did happen: {streamed}"
    );
    assert!(
        streamed.contains("<!-- APPLIED -->"),
        "the marker stays true — nodes really were written: {streamed}"
    );
    let live = state.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        sibling_frames(&live.editor),
        1,
        "the placed base is still the only recipe root"
    );
    assert_eq!(root_count(&live.editor), 1, "and nothing new is beside it");
}

#[test]
fn a_recipe_turn_that_composed_a_screen_is_not_reported_as_template_only() {
    let state = placed_recipe_state();
    let provider = ScriptedProvider {
        response: COMPOSING_REPLY.to_string(),
    };
    let mut out = Vec::new();

    stream_modify_route(
        &mut out,
        placed_recipe_plan(),
        &provider,
        None,
        &state,
        &SseHub::default(),
        None,
    )
    .expect("the modify turn answers");

    let streamed = String::from_utf8(out).expect("utf8 sse");
    assert!(
        !streamed.contains("No new screen was composed"),
        "a turn that did compose must not be accused of not having: {streamed}"
    );
    assert!(streamed.contains("<!-- APPLIED -->"), "{streamed}");
    let live = state.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        root_count(&live.editor),
        2,
        "the composed screen stands beside the base it adapted"
    );
}

#[test]
fn an_ordinary_edit_turn_never_gets_the_recipe_notice() {
    // The sentence is about a base placed *for this turn*; an ordinary edit of
    // an existing screen owes the user no such claim.
    let state = placed_recipe_state();
    let provider = ScriptedProvider {
        response: TEMPLATE_ONLY_REPLY.to_string(),
    };
    let mut plan = placed_recipe_plan();
    plan.rewrites_a_placed_recipe = false;
    let mut out = Vec::new();

    stream_modify_route(
        &mut out,
        plan,
        &provider,
        None,
        &state,
        &SseHub::default(),
        None,
    )
    .expect("the modify turn answers");

    let streamed = String::from_utf8(out).expect("utf8 sse");
    assert!(
        !streamed.contains("No new screen was composed"),
        "an edit turn owes no recipe report: {streamed}"
    );
    assert!(streamed.contains("<!-- APPLIED -->"), "{streamed}");
}

// ── #189: one base, not two ─────────────────────────────────────────────────

#[test]
fn a_new_route_turn_whose_base_is_already_placed_ends_with_one_recipe_root() {
    let state = Mutex::new(WebCanvasState::new(daemon_session_document(), 3100));
    let hub = SseHub::default();
    let master_name = recipe_master_name(&state.lock().unwrap_or_else(|p| p.into_inner()).editor);

    // The pre-classification placement `stream_standard_turn` performs before
    // it classifies anything.
    let placed = place_selected_recipe(RECIPE_PROMPT, &state, &hub, None)
        .expect("the kit ships the ops recipe for this prompt");
    assert_eq!(
        recipe_roots(&state, &master_name),
        1,
        "one base, placed once"
    );

    let snapshot = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .editor
        .clone();
    let mut out = Vec::new();
    stream_new_design_route(
        &mut out,
        design_turn(RECIPE_PROMPT, Vec::new()),
        snapshot,
        Box::new(ScriptedProvider {
            response: "ok".into(),
        }),
        Some("vision-model".into()),
        CanvasWriteTarget {
            state: &state,
            hub: &hub,
            write_barrier: None,
        },
        op_editor_core::ReferenceEvidence::NoImage,
        Some(placed),
    )
    .expect("the new-design route answers");

    assert_eq!(
        recipe_roots(&state, &master_name),
        1,
        "a turn that already carries its base must not clone it again: {:?}",
        root_names(&state)
    );
}

fn recipe_roots(state: &Mutex<WebCanvasState>, master_name: &str) -> usize {
    root_names(state)
        .iter()
        .filter(|name| name.as_str() == master_name)
        .count()
}

fn root_names(state: &Mutex<WebCanvasState>) -> Vec<String> {
    use op_editor_core::PenNodeExt as _;
    state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .editor
        .active_children()
        .iter()
        .map(|node| node.base().name.clone().unwrap_or_default())
        .collect()
}

// ── #184: the blank starter frame is not a screen ───────────────────────────

#[test]
fn the_daemons_own_fresh_document_still_reads_as_the_blank_starter() {
    // The regression itself: the clear used to compare whole documents, and
    // `/api/file/new` merges the Skala library into the starter, so on this
    // daemon the comparison could never hold and the clear never ran.
    let mut state = daemon_session_document();
    assert!(
        state
            .doc
            .pages
            .as_ref()
            .is_some_and(|pages| pages.len() > 1),
        "the fixture is the daemon's document: starter + kit pages"
    );

    assert!(clear_fresh_starter_frame_for_design(&mut state));
    assert!(state.active_children().is_empty());
}

#[test]
fn a_page_with_content_is_left_alone() {
    let mut state = EditorState::starter();
    state.apply(EditorCommand::InsertNode {
        kind: "rect".into(),
        name: "Drawn".into(),
        x: 10,
        y: 10,
        width: 40,
        height: 40,
        fill_hex: Some("#ff0000".into()),
        target_parent: NodeId::NONE,
        page_id: None,
    });

    assert!(
        !clear_fresh_starter_frame_for_design(&mut state),
        "a page the user has drawn on is not a fresh starter"
    );
    assert_eq!(state.active_children().len(), 2);
}

#[test]
fn a_design_turn_clears_the_starter_frame_it_was_launched_with() {
    let state = Mutex::new(WebCanvasState::new(daemon_session_document(), 3100));
    let hub = SseHub::default();
    let mut snapshot = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .editor
        .clone();
    assert_eq!(snapshot.active_children().len(), 1, "the starter frame");

    clear_starter_frame_for_design(&mut snapshot, &state, &hub, None);

    assert!(
        snapshot.active_children().is_empty(),
        "the snapshot follows"
    );
    assert!(
        state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .editor
            .active_children()
            .is_empty(),
        "and so does the live document the browser syncs"
    );
}

// ── #183: a placement is a document change the pollers can see ──────────────

#[test]
fn placing_a_recipe_advances_the_document_version() {
    let state = Mutex::new(WebCanvasState::new(daemon_session_document(), 3100));
    let before = state.lock().unwrap_or_else(|p| p.into_inner()).version;

    let placed = place_selected_recipe(RECIPE_PROMPT, &state, &SseHub::default(), None)
        .expect("the kit ships the ops recipe for this prompt");

    let after = state.lock().unwrap_or_else(|p| p.into_inner()).version;
    assert!(
        after > before,
        "a placement changes the document ({before} -> {after}); \
         `/api/mcp/version` is what the browser polls for that"
    );
    assert!(!placed.1.as_str().is_empty());
}

// ── #199: the fallback placement goes through the same doors ────────────────

/// The `None` arm of `stream_new_design_route`'s base match places the recipe
/// itself, and it reached that write by calling `instantiate_component` alone:
/// no collab gate, no write admission, no version bump. It is reachable exactly
/// when the pre-classification placement stood down — the case where the write
/// is least welcome — and a closed write barrier is how "no admission" shows:
/// the document must be left as the flush found it (issue #199).
#[test]
fn a_fallback_placement_is_refused_by_a_closed_write_barrier() {
    use crate::web_canvas_server::WriteBarrier;

    let barrier = WriteBarrier::default();
    barrier.close();
    let state = Mutex::new(WebCanvasState::new(daemon_session_document(), 3100));
    let hub = SseHub::default();
    let master_name = recipe_master_name(&state.lock().unwrap_or_else(|p| p.into_inner()).editor);
    let before = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .document_version_for_test();
    let snapshot = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .editor
        .clone();
    let mut out = Vec::new();

    stream_new_design_route(
        &mut out,
        design_turn(RECIPE_PROMPT, Vec::new()),
        snapshot,
        Box::new(ScriptedProvider {
            response: "ok".into(),
        }),
        Some("vision-model".into()),
        CanvasWriteTarget {
            state: &state,
            hub: &hub,
            write_barrier: Some(&barrier),
        },
        op_editor_core::ReferenceEvidence::NoImage,
        // No base placed before this route ran: this is the fallback arm.
        None,
    )
    .expect("the new-design route answers");

    assert_eq!(
        recipe_roots(&state, &master_name),
        0,
        "a closed barrier must refuse the placement: {:?}",
        root_names(&state)
    );
    assert_eq!(
        state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .document_version_for_test(),
        before,
        "a refused write advances nothing"
    );
}
