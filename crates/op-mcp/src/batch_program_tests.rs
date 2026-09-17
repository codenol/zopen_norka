//! Tests for the multi-op `batch_design` DSL program executor
//! (`batch_program.rs`) — mixed programs, bindings, slash paths, and
//! TS `runBatchDesignDsl` error semantics, exercised through the
//! public `BatchDesign` tool exactly as the MCP host drives it.

use std::collections::BTreeMap;

use op_editor_core::{EditorCommand, EditorState, NodeId, PenNodeExt};
use serde_json::Value;

use super::batch_design_snapshot;
use super::batch_program_test_support::{
    binding_id, call_operations, call_operations_best_effort, contains_merge_app_state,
    subtree_contains_text,
};
use super::test_fixtures::{frame, sample, state_with, text};
use super::{McpTool, ToolOutcome};

#[test]
fn bare_document_and_root_parent_aliases_insert_at_document_root() {
    let mut state = op_editor_core::EditorState::new();
    state.active_children_mut().clear();
    let program = r#"root=I(document, {"type":"frame","name":"Search","width":390,"height":844,"layout":"vertical"})
I(root, {"type":"text","name":"Title","content":"Find your sound"})
second=I(root, {"type":"frame","name":"Nested"})
screen=I(root, {"type":"frame","name":"Nested Again"})"#;

    let (envelope, cmd) = call_operations(&state, program);

    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(state.apply(cmd.expect("root-alias program emits a command")));
    assert_eq!(state.active_children().len(), 1);
    let children = state.active_children()[0]
        .children()
        .expect("root frame children");
    assert_eq!(children.len(), 3);

    let (second_envelope, second_cmd) = call_operations(
        &state,
        r#"other=I(root, {"type":"frame","name":"Second Root","width":390,"height":844})"#,
    );
    assert!(second_envelope.get("errors").is_none(), "{second_envelope}");
    assert!(state.apply(second_cmd.expect("bare root alias emits a command")));
    assert_eq!(state.active_children().len(), 2);
}

#[test]
fn quoted_root_remains_a_literal_existing_node_id() {
    let mut state = state_with(vec![frame(
        "root",
        "Literal Root",
        0.0,
        0.0,
        320.0,
        240.0,
        vec![],
    )]);

    let (envelope, cmd) = call_operations(
        &state,
        r#"child=I("root", {"type":"text","name":"Child","content":"Nested"})"#,
    );

    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(state.apply(cmd.expect("literal-parent insert emits a command")));
    assert_eq!(state.active_children().len(), 1);
    assert_eq!(
        state.active_children()[0]
            .children()
            .map(|children| children.len()),
        Some(1)
    );
}

#[test]
fn three_value_padding_update_expands_css_shorthand() {
    let mut state = sample();

    let (envelope, cmd) = call_operations(
        &state,
        "U(\"n10\", {\"x\":48})\nU(\"n10\", {\"padding\":[1,2,3]})",
    );

    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(state.apply(cmd.expect("padding update emits a command")));
    let root = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("n10"))
        .expect("updated frame");
    let json = serde_json::to_value(root).expect("frame json");
    assert_eq!(json["padding"], serde_json::json!([1.0, 2.0, 3.0, 2.0]));
}

#[test]
fn mixed_program_executes_all_ops_with_shared_bindings_and_slash_paths() {
    let mut state = sample();
    let program = r##"card=I("n10", {"type":"frame","name":"Card","width":200,"height":120,"children":[{"type":"text","id":"title","name":"Title","content":"Hi","width":100,"height":24}]})
U(card+"/title", {"content":"Hello"})
copy=C(card, null, {"name":"Card Copy"})
D("n14")"##;

    let (envelope, cmd) = call_operations(&state, program);

    // TS envelope: results for I + C (U/D push nothing), no errors key.
    let results = envelope["results"].as_array().expect("results");
    assert_eq!(results.len(), 2, "{envelope}");
    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(envelope["nodeCount"].as_u64().is_some(), "{envelope}");
    let card_id = binding_id(&envelope, "card");
    let copy_id = binding_id(&envelope, "copy");
    assert_ne!(card_id, copy_id);

    // The four surviving ops ride home as ONE atomic Batch command.
    let cmd = cmd.expect("program with successful lines must emit a command");
    let EditorCommand::Batch { ref commands } = cmd else {
        panic!("expected Batch, got {cmd:?}");
    };
    assert_eq!(commands.len(), 4, "{commands:?}");

    // Apply against the SAME doc the snapshot was taken from — the
    // predicted binding ids must be the ids that actually land.
    assert!(state.apply(cmd));
    let children = state.active_children();
    let card = op_editor_core::walkers::find_node(children, &NodeId::new(&card_id)).expect("card");
    assert_eq!(card.base().name.as_deref(), Some("Card"));
    // U(card+"/title") resolved the AUTHORED child id through the
    // alias table and patched the final node.
    let title = &card.children().expect("card children")[0];
    let title_json = serde_json::to_value(title).unwrap();
    assert_eq!(title_json["content"], "Hello", "{title_json}");
    // C cloned the card (with its child) under the page root.
    let copy = op_editor_core::walkers::find_node(children, &NodeId::new(&copy_id)).expect("copy");
    assert_eq!(copy.base().name.as_deref(), Some("Card Copy"));
    assert_eq!(copy.children().expect("copy children").len(), 1);
    // D removed n14.
    assert!(op_editor_core::walkers::find_node(children, &NodeId::new("n14")).is_none());
}

#[test]
fn kit_op_instantiates_shadcn_component_under_parent() {
    let mut state = op_editor_core::EditorState::new();
    let program = r##"root=I(null, {"type":"frame","name":"Root","width":320,"height":240})
button=K("shadcn/btn-primary", root)"##;
    let (envelope, cmd) = call_operations(&state, program);

    assert!(envelope.get("errors").is_none(), "{envelope}");
    let root_id = binding_id(&envelope, "root");
    let button_id = binding_id(&envelope, "button");
    assert_ne!(root_id, button_id);

    assert!(state.apply(cmd.expect("K program emits a command")));
    let root = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(&root_id))
        .expect("root frame");
    let root_children = root.children().expect("root children");
    let button = op_editor_core::walkers::find_node(root_children, &NodeId::new(&button_id))
        .expect("button under root");
    assert_eq!(button.base().name.as_deref(), Some("Primary Button"));
    assert!(
        button.children().map(|c| !c.is_empty()).unwrap_or(false),
        "kit instance must carry the shadcn button subtree"
    );
    assert!(
        subtree_contains_text(button, "Button"),
        "shadcn primary button label should survive"
    );
}

#[test]
fn kit_op_applies_descendant_text_overrides_before_remapping_ids() {
    let mut state = op_editor_core::EditorState::new();
    let program = r##"root=I(null, {"type":"frame","name":"Root","width":320,"height":240})
button=K("shadcn/btn-primary", root, {"descendants":{"shadcn-btn-primary-label":{"content":"Book now"}}})"##;
    let (envelope, cmd) = call_operations(&state, program);

    assert!(envelope.get("errors").is_none(), "{envelope}");
    let root_id = binding_id(&envelope, "root");
    let button_id = binding_id(&envelope, "button");

    assert!(state.apply(cmd.expect("K override program emits a command")));
    let root = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(&root_id))
        .expect("root frame");
    let button = op_editor_core::walkers::find_node(
        root.children().expect("root children"),
        &NodeId::new(&button_id),
    )
    .expect("button under root");
    assert!(
        subtree_contains_text(button, "Book now"),
        "descendants override must land on the original shadcn label before ids are remapped"
    );
}

#[test]
fn failing_line_rolls_back_the_whole_batch() {
    // Transactional contract (Pencil parity): a failing line means NO
    // line of the batch applies — no command ships, the envelope carries
    // errors[] + applied:false + a resend hint, and bindings/results are
    // dropped (their ids never land, so reporting them would invite the
    // model to reference phantom nodes).
    let state = sample();
    let before = serde_json::to_string(state.active_children()).expect("snapshot");
    let program = r##"a=I("n10", {"type":"rectangle","name":"A","width":10,"height":10})
U("missing-node", {"x":5})
b=I("n10", {"type":"rectangle","name":"B","width":10,"height":10})"##;

    let (envelope, cmd) = call_operations(&state, program);
    assert!(cmd.is_none(), "a failing line must roll back every command");
    let errors = envelope["errors"].as_array().expect("errors present");
    assert_eq!(errors.len(), 1, "{envelope}");
    assert_eq!(errors[0]["error"], "Update target not found: missing-node");
    assert_eq!(
        errors[0]["line"], r##"U("missing-node", {"x":5})"##,
        "{envelope}"
    );
    assert_eq!(envelope["applied"], Value::Bool(false), "{envelope}");
    assert_eq!(envelope["results"], serde_json::json!([]), "{envelope}");
    let hint = envelope["hint"].as_str().expect("hint present");
    assert!(hint.contains("rolled back"), "{hint}");
    assert!(hint.contains("resend"), "{hint}");
    // No command was handed to the host, so the live doc is untouched.
    assert_eq!(
        serde_json::to_string(state.active_children()).expect("snapshot"),
        before,
        "document must be unchanged after a rolled-back batch"
    );
}

#[test]
fn failed_rail_reconstruction_keeps_all_four_populated_cards_byte_identical() {
    let cards: Vec<jian_ops_schema::node::PenNode> = (1..=4)
        .map(|index| {
            serde_json::from_value(serde_json::json!({
                "type": "frame",
                "id": format!("card-{index}"),
                "name": format!("Event Card {index}"),
                "layout": "vertical",
                "width": 168,
                "height": 220,
                "children": [
                    {
                        "type": "image",
                        "id": format!("card-{index}-image"),
                        "name": format!("Event {index} Photo"),
                        "src": format!("https://example.invalid/event-{index}.jpg"),
                        "objectFit": "crop",
                        "width": "fill_container",
                        "height": 112
                    },
                    {
                        "type": "frame",
                        "id": format!("card-{index}-details"),
                        "name": format!("Event {index} Details"),
                        "layout": "vertical",
                        "width": "fill_container",
                        "height": "fit_content",
                        "children": [
                            {
                                "type": "text",
                                "id": format!("card-{index}-title"),
                                "name": "Title",
                                "content": format!("Popular Event {index}"),
                                "width": "fill_container",
                                "height": 24
                            },
                            {
                                "type": "text",
                                "id": format!("card-{index}-venue"),
                                "name": "Venue",
                                "content": format!("Venue {index}"),
                                "width": "fill_container",
                                "height": 18
                            }
                        ]
                    }
                ]
            }))
            .expect("populated event card")
        })
        .collect();
    let rail: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame",
        "id": "event-rail",
        "name": "Popular Near You",
        "layout": "horizontal",
        "width": 390,
        "height": 220,
        "children": cards
    }))
    .expect("populated rail");
    let state = state_with(vec![rail]);
    let before = serde_json::to_vec(state.active_children()).expect("pre-transaction bytes");

    // This mirrors the destructive-redraft failure mode: four valid cards are
    // deleted in the simulated batch, then reconstruction targets a bad id.
    // Transactional execution must ship no partial delete commands.
    let program = r##"D("card-1")
D("card-2")
D("card-3")
D("card-4")
replacement=I("missing-rail-parent", {"type":"frame","name":"Rebuilt Card","width":168,"height":220,"children":[{"type":"text","content":"replacement","width":120,"height":24}]})"##;
    let (envelope, command) = call_operations(&state, program);

    assert!(
        command.is_none(),
        "failed reconstruction must not emit deletes"
    );
    assert_eq!(envelope["applied"], Value::Bool(false), "{envelope}");
    assert_eq!(envelope["results"], serde_json::json!([]), "{envelope}");
    assert!(envelope["errors"][0]["error"]
        .as_str()
        .is_some_and(|message| message.contains("missing-rail-parent")));
    assert_eq!(
        serde_json::to_vec(state.active_children()).expect("post-transaction bytes"),
        before,
        "the original populated rail must remain byte-identical"
    );

    let rail =
        op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("event-rail"))
            .expect("original rail");
    let cards = rail.children().expect("original cards");
    assert_eq!(cards.len(), 4, "all four cards must remain");
    for (index, card) in cards.iter().enumerate() {
        let children = card.children().expect("populated card subtree");
        assert_eq!(children.len(), 2, "card {} lost content", index + 1);
        assert!(matches!(
            children.first(),
            Some(jian_ops_schema::node::PenNode::Image(image)) if !image.src.as_str().is_empty()
        ));
        assert!(children
            .get(1)
            .and_then(jian_ops_schema::node::PenNode::children)
            .is_some_and(|details| details.len() == 2));
    }
}

#[test]
fn best_effort_policy_keeps_ts_survivor_semantics_for_internal_callers() {
    // The orchestrator's script-gen path (`program_gen.rs`) opts back
    // into TS `runBatchDesignDsl` best-effort: the thrown line lands in
    // errors[] and the remaining lines still execute.
    let mut state = sample();
    let program = r##"a=I("n10", {"type":"rectangle","name":"A","width":10,"height":10})
U("missing-node", {"x":5})
b=I("n10", {"type":"rectangle","name":"B","width":10,"height":10})"##;

    let (envelope, cmd) = call_operations_best_effort(&state, program);
    let errors = envelope["errors"].as_array().expect("errors present");
    assert_eq!(errors.len(), 1, "{envelope}");
    let results = envelope["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);

    assert!(state.apply(cmd.expect("surviving lines emit a command")));
    let frame = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("n10"))
        .expect("n10");
    let names: Vec<_> = frame
        .children()
        .expect("children")
        .iter()
        .filter_map(|c| c.base().name.as_deref())
        .collect();
    assert!(names.contains(&"A") && names.contains(&"B"), "{names:?}");
}

#[test]
fn js_style_line_separators_are_noise_not_syntax_errors() {
    // A model that reaches for a list separator writes it on EVERY line, so
    // rejecting it fails the whole batch and rolls the transaction back —
    // measured 2026-07-31 with five `G(...)` image fills, after which the
    // model mis-diagnosed the error as an ARGUMENT separator problem and
    // spent the rest of the run demolishing subtrees it had already built.
    let state = sample();
    let comma_separated = "a=I(null, {\"type\":\"frame\",\"name\":\"A\"}),\nb=I(null, {\"type\":\"frame\",\"name\":\"B\"}),";
    let (envelope, cmd) = call_operations(&state, comma_separated);
    // A clean run reports no `errors` key at all — a present one means the
    // transaction rolled back.
    assert!(
        envelope.get("errors").is_none(),
        "trailing commas must not fail the batch: {envelope}"
    );
    assert!(cmd.is_some(), "both operations survive");
    assert_eq!(
        envelope["results"].as_array().map(Vec::len),
        Some(2),
        "both bindings land: {envelope}"
    );
    // Semicolons were already tolerated; keep them that way.
    let (semi, semi_cmd) = call_operations(
        &state,
        "a=I(null, {\"type\":\"frame\",\"name\":\"A\"});\nb=I(null, {\"type\":\"frame\",\"name\":\"B\"});",
    );
    assert!(semi.get("errors").is_none(), "{semi}");
    assert!(semi_cmd.is_some());
}

#[test]
fn unparseable_program_returns_ts_errors_without_a_command() {
    let state = sample();
    let program = "X(1)\nfoo bar";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(cmd.is_none(), "no surviving line, no command");
    assert_eq!(envelope["results"], serde_json::json!([]));
    let errors = envelope["errors"].as_array().expect("errors");
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["error"], "Cannot parse operation: X(1)");
    assert_eq!(errors[1]["error"], "Cannot parse operation: foo bar");
}

#[test]
fn lenient_json_accepts_unquoted_keys_single_quotes_and_trailing_commas() {
    // TS `parseJsonArg` fallback pipeline.
    let mut state = sample();
    let program = "a=I(null, {type:'frame', name:'Lenient', width:100, height:50,})\nD(\"ghost\")";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    let id = binding_id(&envelope, "a");
    assert!(state.apply(cmd.expect("command")));
    let node = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(&id))
        .expect("lenient node");
    assert_eq!(node.base().name.as_deref(), Some("Lenient"));
}

#[test]
fn delete_of_unknown_id_is_a_silent_noop_like_ts_remove_node_from_tree() {
    let state = sample();
    // Two lines so the program executor (not the single-op path) runs.
    let program = "D(\"ghost\")\nD(\"phantom\")";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(cmd.is_none(), "nothing to apply");
    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert_eq!(envelope["results"], serde_json::json!([]));
}

#[test]
fn root_frame_insert_replaces_the_first_empty_frame_and_inherits_position() {
    // TS auto-replace: `I(null, frame)` swaps in for an empty root
    // frame instead of creating a sibling. Lenient JSON keeps this
    // program on the executor path. The placeholder carries the shipped
    // starter's name, which is what makes it replaceable.
    let mut state = state_with(vec![frame("f1", "Frame", 30.0, 40.0, 100.0, 100.0, vec![])]);
    let program = "page=I(null, {type:'frame', name:'Page', width:320, height:240})";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    let page_id = binding_id(&envelope, "page");

    assert!(state.apply(cmd.expect("command")));
    let children = state.active_children();
    assert!(
        op_editor_core::walkers::find_node(children, &NodeId::new("f1")).is_none(),
        "the empty frame must be replaced"
    );
    let page = op_editor_core::walkers::find_node(children, &NodeId::new(&page_id))
        .expect("inserted page");
    assert_eq!(page.base().x, Some(30.0), "inherits the empty frame's x");
    assert_eq!(page.base().y, Some(40.0), "inherits the empty frame's y");
}

#[test]
fn root_frame_batch_replaces_only_preexisting_empty_starters() {
    let mut state = state_with(vec![frame("f1", "Frame", 30.0, 40.0, 100.0, 100.0, vec![])]);
    let program = "a=I(null, {type:'frame', name:'A'})\nb=I(null, {type:'frame', name:'B'})\nc=I(null, {type:'frame', name:'C'})";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");

    assert!(state.apply(cmd.expect("three-screen batch")));
    let roots = state.active_children();
    assert_eq!(roots.len(), 3, "all authored screen shells must survive");
    assert_eq!(
        roots
            .iter()
            .filter_map(|root| root.base().name.as_deref())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert_eq!(
        (roots[0].base().x, roots[0].base().y),
        (Some(30.0), Some(40.0))
    );
}

/// The pre-program snapshot is retaken every `batch_design` call, so shells a
/// previous batch inserted looked like fresh starter placeholders to the next
/// one: batch 1 built A and B, batch 2 built C, and A was silently deleted.
/// An authored name is what tells the two apart.
#[test]
fn a_later_batch_does_not_consume_the_previous_batchs_empty_screen_shells() {
    let mut state = state_with(vec![frame(
        "home",
        "Home",
        0.0,
        0.0,
        390.0,
        844.0,
        vec![text("home-title", "Title", 0.0, 0.0, 100.0, 20.0, "Home")],
    )]);

    let program = "a=I(null, {type:'frame', name:'A'})\nb=I(null, {type:'frame', name:'B'})";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(state.apply(cmd.expect("first batch")));
    assert_eq!(root_names(&state), ["Home", "A", "B"]);

    // A and B are still empty — the model fills them in later batches.
    let program = "c=I(null, {type:'frame', name:'C'})\nd=I(null, {type:'frame', name:'D'})";
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    assert!(state.apply(cmd.expect("second batch")));
    assert_eq!(
        root_names(&state),
        ["Home", "A", "B", "C", "D"],
        "an earlier batch's empty shells must survive the next batch"
    );
}

fn root_names(state: &EditorState) -> Vec<&str> {
    state
        .active_children()
        .iter()
        .filter_map(|root| root.base().name.as_deref())
        .collect()
}

#[test]
fn bound_move_records_the_binding_and_honors_the_index() {
    let mut state = sample();
    let program = r##"box=I("n10", {"type":"rectangle","name":"Box","width":10,"height":10})
moved=M(box, "n12", 0)"##;
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    let box_id = binding_id(&envelope, "box");
    assert_eq!(binding_id(&envelope, "moved"), box_id);

    assert!(state.apply(cmd.expect("command")));
    let group = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("n12"))
        .expect("group");
    let first = &group.children().expect("children")[0];
    assert_eq!(first.id_str(), box_id, "M(.., 0) inserts at the front");
}

#[test]
fn move_of_unknown_target_reports_ts_error_message() {
    let state = sample();
    let program = "M(\"nope\", null)\nD(\"ghost\")";
    let (envelope, _) = call_operations(&state, program);
    let errors = envelope["errors"].as_array().expect("errors");
    assert_eq!(errors[0]["error"], "Move target not found: nope");
}

#[test]
fn replace_swaps_the_node_in_place_and_binds_the_fresh_id() {
    let mut state = sample();
    let program = r##"swap=R("n13", {"type":"text","name":"Swapped","content":"New","width":50,"height":20})
D("ghost")"##;
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    let new_id = binding_id(&envelope, "swap");
    assert_ne!(new_id, "n13");

    assert!(state.apply(cmd.expect("command")));
    let children = state.active_children();
    assert!(op_editor_core::walkers::find_node(children, &NodeId::new("n13")).is_none());
    let group = op_editor_core::walkers::find_node(children, &NodeId::new("n12")).expect("n12");
    // Same slot: the replacement sits where n13 (first child) was.
    let first = &group.children().expect("children")[0];
    assert_eq!(first.id_str(), new_id, "predicted id must land in place");
    assert_eq!(first.base().name.as_deref(), Some("Swapped"));
}

#[test]
fn replace_program_hoists_node_state() {
    let state = sample();
    let program = r##"swap=R("n13", {"type":"frame","name":"Counter","width":120,"height":24,"state":{"n":{"type":"int","default":0}}})
D("ghost")"##;
    let (envelope, cmd) = call_operations(&state, program);
    assert!(envelope.get("errors").is_none(), "{envelope}");
    match cmd.expect("command") {
        EditorCommand::Batch { commands } => {
            assert!(
                matches!(&commands[0], EditorCommand::MergeAppState { plan_idx, state }
                if *plan_idx == usize::MAX && state.contains_key("n"))
            );
            let replacement = commands
                .iter()
                .find_map(|c| match c {
                    EditorCommand::ReplaceSubtree { node, .. } => Some(node),
                    _ => None,
                })
                .expect("ReplaceSubtree in batch");
            let v = serde_json::to_value(replacement.as_ref()).expect("json");
            assert!(
                v.get("state").is_none(),
                "replacement state must be stripped"
            );
        }
        other => panic!("expected Batch, got {other:?}"),
    }
}

#[test]
fn failed_insert_line_does_not_leak_merge_app_state() {
    // A stateful `I()` line whose parent doesn't resolve to a container
    // fails at the `InsertAuthoredSubtree` emit — AFTER the node's
    // `state` would already have been hoisted. The line's error is
    // collected and the line dropped (TS best-effort semantics), so its
    // hoisted `MergeAppState` must never survive into the surviving
    // command. `U(...)` (rather than a second `I()`) is the second line
    // so `parse_operations` can't take the single-shot Insert-only fast
    // path and this program is guaranteed to run through the mixed-DSL
    // executor (`run_batch_design_program`) this fix targets.
    // Best-effort policy: survivors only ship on the internal script-gen
    // path now (the agent surface rolls the whole batch back instead).
    let mut state = sample();
    let program = r##"a=I("nonexistent-parent", {"type":"frame","name":"Ghost","width":100,"height":50,"state":{"n":{"type":"int","default":0}}})
U("n11", {"name":"Renamed"})"##;
    let (envelope, cmd) = call_operations_best_effort(&state, program);
    let errors = envelope["errors"].as_array().expect("errors present");
    assert_eq!(errors.len(), 1, "{envelope}");
    assert!(
        errors[0]["error"]
            .as_str()
            .unwrap()
            .starts_with("Insert parent not found or not a container"),
        "{envelope}"
    );
    let cmd = cmd.expect("the surviving U() line still emits a command");
    assert!(
        !contains_merge_app_state(&cmd),
        "failed insert line must not leak orphan MergeAppState: {cmd:?}"
    );
    assert!(
        state.apply(cmd),
        "surviving command must still apply cleanly"
    );
}

#[test]
fn failed_replace_line_does_not_leak_merge_app_state() {
    // The insert case above fails at the emit (parent-not-container is
    // only checked inside `InsertAuthoredSubtree`'s apply). A plain
    // `R("no-such-id", ...)` instead fails EARLIER, in `find_node_by_path`
    // — before `hoist_generation_state` ever runs — so it can't exercise
    // the vulnerable emit-ordering window this fix closes. To reach that
    // window for replace, force the `ReplaceSubtree` emit ITSELF to fail
    // post-hoist: a `pageId` that resolves to no page. The program's
    // initial page-pin silently no-ops on the same unresolvable id
    // (leaving the sim on its real, default active page), so
    // `find_node_by_path` still finds "n13" and the node's `state` is
    // hoisted — but the ReplaceSubtree command's own page stamp is then
    // rejected at apply, reproducing exactly the failure shape codex
    // flagged for `I()`.
    let tool = batch_design_snapshot(&sample());
    let mut args = BTreeMap::new();
    args.insert("pageId".into(), "does-not-exist".into());
    // Best-effort keeps the survivor window open (the agent surface
    // would roll the whole batch back and never ship ANY command).
    args.insert("_line_policy".into(), "best_effort".into());
    args.insert(
        "operations".into(),
        r##"swap=R("n13", {"type":"frame","name":"Ghost","width":100,"height":50,"state":{"n":{"type":"int","default":0}}})
D("ghost")"##
            .into(),
    );
    let (envelope, cmd): (Value, Option<EditorCommand>) = match tool.call(&args) {
        ToolOutcome::OkJson(json) => (serde_json::from_str(&json).expect("json"), None),
        ToolOutcome::OkJsonWithCommand(json, cmd) => {
            (serde_json::from_str(&json).expect("json"), Some(cmd))
        }
        other => panic!("expected a TS result envelope, got {other:?}"),
    };
    let errors = envelope["errors"].as_array().expect("errors present");
    assert_eq!(errors.len(), 1, "{envelope}");
    assert!(
        errors[0]["error"]
            .as_str()
            .unwrap()
            .starts_with("Replace failed for:"),
        "{envelope}"
    );
    // `D("ghost")` is a silent no-op (unknown id), so nothing else could
    // legitimately contribute a command here — any surviving command
    // must NOT be (or contain) the orphaned MergeAppState.
    assert!(
        cmd.as_ref()
            .map(|c| !contains_merge_app_state(c))
            .unwrap_or(true),
        "failed replace line must not leak orphan MergeAppState: {cmd:?}"
    );
}

#[path = "batch_program_image_tests.rs"]
mod image_tests;

#[path = "batch_program_interactivity_tests.rs"]
mod interactivity_tests;

/// Issue #206, end to end: the measured line from the two-screen prompt.
///
/// The model wrote `divider` as a node TYPE although the skills catalogue
/// documents it as a ROLE. Serde rejected the unknown `PenNode` variant, the
/// executor dropped the line, and the turn still reported success — the
/// person was told the screen was drawn while the card's hairline was
/// missing. The requested element must become real.
#[test]
fn divider_typed_line_lands_as_a_visible_node() {
    let mut state = op_editor_core::EditorState::new();
    state.active_children_mut().clear();
    // Verbatim from the daemon log of the re-run of prompt 7, minus the
    // binding numbers that only this turn's program knows.
    let program = r##"card=I(document, {"type":"frame","name":"Card","width":320,"height":200,"layout":"vertical"})
b46=I(card, {"type":"divider","name":"card-divider","width":"fill_container","height":1,"fill":[{"type":"solid","color":"#E6EBF3"}]})"##;

    let (envelope, cmd) = call_operations(&state, program);

    assert!(
        envelope.get("errors").is_none(),
        "the divider line must not be dropped: {envelope}"
    );
    assert!(state.apply(cmd.expect("the program emits a command")));
    let card = &state.active_children()[0];
    let children = card.children().expect("card children");
    assert_eq!(children.len(), 1, "the divider must be inside the card");
    let value = serde_json::to_value(&children[0]).expect("node json");
    assert_eq!(
        value["type"].as_str(),
        Some("rectangle"),
        "the alias must become a real node: {value}"
    );
    assert_eq!(
        value["role"].as_str(),
        Some("divider"),
        "the role the model meant must survive: {value}"
    );
    assert_eq!(value["height"].as_f64(), Some(1.0), "{value}");
    assert_eq!(value["width"].as_str(), Some("fill_container"), "{value}");
    assert!(
        value["fill"]
            .as_array()
            .is_some_and(|fill| !fill.is_empty()),
        "a divider with no fill paints nothing: {value}"
    );
}
