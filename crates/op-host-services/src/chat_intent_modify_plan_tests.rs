//! Design-variable context + modify-plan + apply-modification tests. Split
//! out of `chat_intent_tests.rs` at the 800-line cap; nested under that
//! module so `use super::*` still reaches its node/state helpers.

use super::*;

fn seed_variables(state: &mut EditorState) {
    use jian_ops_schema::variable::{
        ThemedValue, VariableDefinition, VariableKind, VariableScalar, VariableValue,
    };
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(
        "color-1".to_string(),
        VariableDefinition {
            kind: VariableKind::Color,
            value: VariableValue::Themed(vec![
                ThemedValue {
                    value: VariableScalar::Str("#112233".into()),
                    theme: None,
                },
                ThemedValue {
                    value: VariableScalar::Str("#aabbcc".into()),
                    theme: None,
                },
            ]),
        },
    );
    vars.insert(
        "spacing-1".to_string(),
        VariableDefinition {
            kind: VariableKind::Number,
            value: VariableValue::Scalar(VariableScalar::Num(8.0)),
        },
    );
    state.doc.variables = Some(vars);
    let mut themes = std::collections::BTreeMap::new();
    themes.insert("Theme-1".to_string(), vec!["Light".into(), "Dark".into()]);
    state.doc.themes = Some(themes);
}

#[test]
fn variable_context_matches_ts_format() {
    let mut state = EditorState::new();
    assert!(build_variable_context(&state).is_none());
    seed_variables(&mut state);
    let ctx = build_variable_context(&state).expect("variables present");
    assert!(ctx.starts_with(
        "DOCUMENT VARIABLES (use \"$name\" to reference, e.g. fill color \"$color-1\"):"
    ));
    assert!(ctx.contains("  - color-1 (color): #112233 [themed]"));
    assert!(ctx.contains("  - spacing-1 (number): 8"));
    assert!(ctx.contains("Themes: Theme-1: [Light, Dark]"));
}

#[test]
fn modify_plan_targets_selection_when_present() {
    let mut state = state_with_page();
    state.selection.set = vec![op_editor_core::NodeId::new("page-1")];
    state.selection.anchor = op_editor_core::NodeId::new("page-1");
    let plan = build_modify_plan(&state, "make it red").expect("plan");
    assert_eq!(plan.target_frame_ids, vec!["page-1".to_string()]);
    assert!(plan.user_message.starts_with("CONTEXT NODES:\n"));
    assert!(plan.user_message.contains("\"id\":\"page-1\""));
    assert!(plan.user_message.contains("\n\nINSTRUCTION:\nmake it red"));
}

#[test]
fn modify_plan_strips_base64_data_uris_from_context_nodes() {
    let image_data_uri = "data:image/png;base64,AAAABBBBCCCC";
    let fill_data_uri = "data:image/jpeg;base64,DDDDEEEEFFFF";
    let mut image_fill_rect: PenNode = serde_json::from_value(serde_json::json!({
        "type": "rectangle",
        "id": "fill-card",
        "name": "Image Fill Card",
        "x": 0.0,
        "y": 100.0,
        "width": 120.0,
        "height": 80.0,
        "fill": [
            { "type": "image", "url": fill_data_uri, "mode": "crop" },
            { "type": "solid", "color": "$color-1" }
        ],
    }))
    .expect("valid rectangle with image fill json");
    image_fill_rect.base_mut().explain = Some("keep metadata".into());

    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.active_children_mut().push(frame(
        "page-1",
        "Home",
        375.0,
        vec![
            image("hero-photo", "Hero Photo", image_data_uri),
            image_fill_rect,
        ],
    ));
    state.set_single_selection(op_editor_core::NodeId::new("page-1"));

    let plan = build_modify_plan(&state, "make it warmer").expect("plan");
    let context_json = plan
        .user_message
        .strip_prefix("CONTEXT NODES:\n")
        .and_then(|rest| rest.split_once("\n\nINSTRUCTION:\n").map(|(ctx, _)| ctx))
        .expect("context section");
    let context: serde_json::Value =
        serde_json::from_str(context_json).expect("valid context json");

    assert_eq!(
        context
            .pointer("/0/children/0/src")
            .and_then(|v| v.as_str()),
        Some("<image>")
    );
    assert_eq!(
        context
            .pointer("/0/children/1/fill/0/url")
            .and_then(|v| v.as_str()),
        Some("<image>")
    );
    assert_eq!(
        context
            .pointer("/0/children/1/fill/1/color")
            .and_then(|v| v.as_str()),
        Some("$color-1")
    );
    assert_eq!(
        context
            .pointer("/0/children/1/explain")
            .and_then(|v| v.as_str()),
        Some("keep metadata")
    );
    assert!(
        !plan.user_message.contains("AAAABBBBCCCC") && !plan.user_message.contains("DDDDEEEEFFFF"),
        "base64 blobs must not be sent to the model: {}",
        plan.user_message
    );
}

#[test]
fn modify_plan_requires_a_selected_frame() {
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state
        .active_children_mut()
        .push(frame("f1", "One", 375.0, vec![]));
    state
        .active_children_mut()
        .push(frame("f2", "Two", 375.0, vec![]));
    state.active_children_mut().push(rect("r1", "Loose"));

    assert!(
        build_modify_plan(&state, "tweak").is_none(),
        "no selection must never fall back to an arbitrary existing node"
    );

    state.set_single_selection(op_editor_core::NodeId::new("r1"));
    assert!(
        build_modify_plan(&state, "tweak").is_none(),
        "a non-frame selection must not authorize direct modification"
    );

    state.set_single_selection(op_editor_core::NodeId::new("f2"));
    let plan = build_modify_plan(&state, "tweak").expect("selected frame plan");
    assert_eq!(plan.target_frame_ids, vec!["f2".to_string()]);
    assert!(plan.user_message.contains("\"id\":\"f2\""));
    assert!(!plan.user_message.contains("\"id\":\"f1\""));
}

#[test]
fn modify_plan_appends_variable_context() {
    let mut state = state_with_page();
    state.set_single_selection(op_editor_core::NodeId::new("page-1"));
    seed_variables(&mut state);
    let plan = build_modify_plan(&state, "recolor with variables").expect("plan");
    assert!(plan.user_message.contains("DOCUMENT VARIABLES"));
    assert!(
        !plan.system_prompt.is_empty(),
        "maintenance skills resolve into the system prompt"
    );
}

#[test]
fn modify_plan_is_none_for_an_empty_page() {
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    assert!(build_modify_plan(&state, "tweak").is_none());
}

// ---------------------------------------------------------------------------
// #207 — the recipe-base rule reaches the route a recipe turn takes
// ---------------------------------------------------------------------------

/// The recipe the kit ships for a screen prompt, as the placement names it.
fn a_placed_recipe() -> (String, String) {
    let recipe = op_editor_core::session_kit()
        .recipes
        .first()
        .expect("the kit ships recipes");
    (recipe.id.clone(), recipe.name.clone())
}

/// The prompt contract of issue #207, at the level the plan controls: a turn
/// told that the host just placed a recipe carries the `doc:recipe-base`
/// instruction in the system prompt it builds, and an ordinary edit turn does
/// not — so the assertion below is about the flag and not about a constant
/// that is always there.
///
/// This is the unit half of the check. The wire half lives in
/// `web_chat_standard::recipe_reference_tests`, which captures the request the
/// route really sends.
#[test]
fn a_recipe_turn_carries_the_recipe_base_rule_in_its_system_prompt() {
    let mut state = state_with_page();
    state.set_single_selection(op_editor_core::NodeId::new("page-1"));
    let (recipe_id, recipe_name) = a_placed_recipe();

    let plain = build_modify_plan(&state, "make it red").expect("plan");
    assert!(
        !plain.system_prompt.contains("Recipe already placed"),
        "an ordinary edit turn stands on no placement and must not be told it does"
    );

    let hint = super::RecipeBaseHint {
        recipe_id,
        name: recipe_name.clone(),
    };
    let plan =
        build_modify_plan_with(&state, "make it red", Some(&hint)).expect("recipe turn plan");

    assert!(
        plan.system_prompt.contains("Recipe already placed"),
        "the route a recipe turn takes must carry the rule the code builds for \
         it; system prompt was: {}",
        plan.system_prompt
    );
    assert!(
        plan.system_prompt
            .contains("Do not compose this screen again and do not rebuild its structure"),
        "the rule's instruction, not only its title, has to reach the model"
    );
    assert!(
        plan.system_prompt.contains(&recipe_name),
        "the rule names the recipe it is about"
    );
    assert!(
        plan.system_prompt.contains("page-1"),
        "…and the node the host placed it as"
    );
    assert!(
        plan.system_prompt.contains("SESSION RULES"),
        "the placement rule rides in the same rules block as the document's own"
    );
    assert!(
        plan.rewrites_a_placed_recipe,
        "the whole-screen reply the preamble asks for is tied to this flag"
    );
}

/// One wording, two routes: the rule the modify plan renders is the value the
/// new-design route inserts into its `DesignRequest`, so this pins the text
/// both prompts carry (issue #207).
#[test]
fn the_new_design_route_and_the_modify_plan_share_one_recipe_base_rule() {
    let (recipe_id, _) = a_placed_recipe();
    let recipe = op_editor_core::session_kit()
        .recipes
        .iter()
        .find(|recipe| recipe.id == recipe_id)
        .expect("the recipe the placement named");
    let node_id = op_editor_core::NodeId::new("n42");

    let rule = crate::web_chat_standard::recipe_base_rule(recipe, &node_id);
    assert_eq!(rule.id, "doc:recipe-base");
    assert_eq!(rule.kind, jian_ops_schema::DesignRuleKind::Require);
    assert_eq!(rule.scope, jian_ops_schema::DesignRuleScope::Global);
    assert_eq!(rule.priority, i32::MIN + 1);

    let rendered = op_editor_core::build_design_rules_policy(&[rule]);
    assert!(rendered.contains(&format!("Recipe already placed: {}", recipe.name)));
    assert!(rendered.contains("as node `n42`"));
    assert!(rendered.contains("adapt what it provides"));
}

// ---------------------------------------------------------------------------
// apply_design_modification (extractAndApplyDesignModification port)
// ---------------------------------------------------------------------------

#[test]
fn apply_modification_is_confined_to_the_captured_frame_scope() {
    use op_editor_core::{walkers::find_node, NodeId};

    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.active_children_mut().push(frame(
        "first",
        "First",
        375.0,
        vec![rect("first-card", "First Card")],
    ));
    state.active_children_mut().push(frame(
        "second",
        "Second",
        375.0,
        vec![rect("second-card", "Second Card")],
    ));
    let nodes = vec![
        modify_op(
            "null",
            serde_json::json!({
                "id": "first-card",
                "type": "rectangle",
                "name": "Must Not Change"
            }),
        ),
        modify_op(
            "first",
            serde_json::json!({
                "type": "text",
                "name": "Must Not Append",
                "content": "wrong frame"
            }),
        ),
        modify_op(
            "null",
            serde_json::json!({
                "type": "text",
                "name": "Scoped Addition",
                "content": "selected frame"
            }),
        ),
    ];

    assert_eq!(
        apply_design_modification(&mut state, &nodes, &[]),
        (0, false),
        "an empty scope must reject the whole modification"
    );
    let (count, mutated) = apply_design_modification(&mut state, &nodes, &["second".to_string()]);
    assert_eq!(count, 1, "only the in-scope implicit insert may apply");
    assert!(mutated);
    assert_eq!(
        find_node(state.active_children(), &NodeId::new("first-card"))
            .and_then(|node| node.base().name.as_deref()),
        Some("First Card")
    );
    let first = find_node(state.active_children(), &NodeId::new("first")).unwrap();
    assert!(!first
        .children()
        .unwrap()
        .iter()
        .any(|node| node.base().name.as_deref() == Some("Must Not Append")));
    let second = find_node(state.active_children(), &NodeId::new("second")).unwrap();
    assert!(second
        .children()
        .unwrap()
        .iter()
        .any(|node| node.base().name.as_deref() == Some("Scoped Addition")));

    let ambiguous = vec![modify_op(
        "null",
        serde_json::json!({
            "type": "text",
            "name": "Ambiguous Addition",
            "content": "no implicit parent"
        }),
    )];
    assert_eq!(
        apply_design_modification(
            &mut state,
            &ambiguous,
            &["first".to_string(), "second".to_string()]
        ),
        (0, false),
        "multiple selected Frames require an explicit parent"
    );
}

#[test]
fn apply_modification_replaces_existing_and_inserts_unknown_top_level() {
    let mut state = state_with_page();
    let nodes = vec![
        // Existing id -> whole-node replacement.
        modify_op(
            "null",
            serde_json::json!({
                "id": "hero",
                "type": "frame",
                "name": "Hero Updated",
                "width": 375.0,
                "height": 200.0,
                "children": []
            }),
        ),
        // Unknown id → insert under the captured target frame (canonical
        // TextNode carries `content`).
        modify_op(
            "null",
            serde_json::json!({
                "id": "fresh-1",
                "type": "text",
                "name": "New Caption",
                "content": "Hello",
            }),
        ),
    ];
    let (count, mutated) = apply_modify_ops_to_frame(&mut state, &nodes, "page-1");
    assert_eq!(count, 2, "replace existing plus insert unknown top-level");
    assert!(mutated);
    let doc = serde_json::to_string(&state.doc).unwrap();
    assert!(doc.contains("Hero Updated"), "existing node is replaced");
    assert!(doc.contains("New Caption"), "new node inserted");
    assert_eq!(count_node_id(state.active_children(), "hero"), 1);
    // The insert landed inside the page frame, not at the page root.
    let page = state
        .active_children()
        .iter()
        .find(|n| n.id_str() == "page-1")
        .unwrap();
    let kids = page.children().unwrap();
    assert!(
        kids.iter().any(|k| k.id_str() == "hero"
            && k.base()
                .name
                .as_deref()
                .is_some_and(|n| n == "Hero Updated")),
        "existing node remains in the primary frame"
    );
    assert!(
        kids.iter()
            .any(|k| k.base().name.as_deref().is_some_and(|n| n == "New Caption")),
        "implied-new node parents to the captured target frame"
    );
}

#[test]
fn apply_modification_adds_under_declared_existing_parent_without_touching_siblings() {
    use op_editor_core::{walkers::find_node, NodeId};

    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.active_children_mut().push(frame(
        "n217",
        "Player",
        320.0,
        vec![rect("n218", "Track Info"), rect("n220", "Actions")],
    ));
    let before_parent = find_node(state.active_children(), &NodeId::new("n217")).unwrap();
    let before_children = before_parent.children().unwrap();
    let before_n218 = serde_json::to_value(&before_children[0]).unwrap();
    let before_n220 = serde_json::to_value(&before_children[1]).unwrap();

    let nodes = vec![modify_op(
        "n217",
        serde_json::json!({
            "type": "frame",
            "name": "Progress Bar",
            "width": 220.0,
            "height": 8.0,
            "children": []
        }),
    )];

    let (count, mutated) = apply_modify_ops_to_frame(&mut state, &nodes, "n217");

    assert_eq!(count, 1);
    assert!(mutated);
    let parent = find_node(state.active_children(), &NodeId::new("n217")).unwrap();
    assert_eq!(parent.base().name.as_deref(), Some("Player"));
    let children = parent.children().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].id_str(), "n218");
    assert_eq!(children[1].id_str(), "n220");
    assert_eq!(children[2].base().name.as_deref(), Some("Progress Bar"));
    assert_eq!(serde_json::to_value(&children[0]).unwrap(), before_n218);
    assert_eq!(serde_json::to_value(&children[1]).unwrap(), before_n220);
}

#[test]
fn apply_modification_anchors_added_mobile_status_bar_first() {
    use op_editor_core::{walkers::find_node, NodeId};

    let root: PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame",
        "id": "search",
        "name": "Waveform — Search",
        "width": 390,
        "height": "fit_content",
        "layout": "vertical",
        "children": [
            {"type":"frame","id":"header","name":"Header","width":"fill_container","height":118},
            {"type":"frame","id":"tabs","name":"Tab Bar","width":"fill_container","height":82}
        ]
    }))
    .expect("mobile root");
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.active_children_mut().push(root);
    let root_before = find_node(state.active_children(), &NodeId::new("search")).unwrap();
    let children_before = root_before.children().unwrap();
    let header_before = serde_json::to_value(&children_before[0]).unwrap();
    let tabs_before = serde_json::to_value(&children_before[1]).unwrap();
    let nodes = vec![modify_op(
        "search",
        serde_json::json!({
            "type": "frame",
            "name": "Status Bar",
            "width": "fill_container",
            "height": "fit_content",
            "layout": "horizontal",
            "children": []
        }),
    )];

    let (count, mutated) = apply_modify_ops_to_frame(&mut state, &nodes, "search");

    assert_eq!(count, 1);
    assert!(mutated);
    let root = find_node(state.active_children(), &NodeId::new("search")).unwrap();
    let children = root.children().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].base().name.as_deref(), Some("Status Bar"));
    assert_eq!(children[0].base().role.as_deref(), Some("status-bar"));
    assert_eq!(serde_json::to_value(&children[1]).unwrap(), header_before);
    assert_eq!(serde_json::to_value(&children[2]).unwrap(), tabs_before);
}

#[test]
fn apply_modification_reuses_appended_mobile_status_bar_instead_of_duplicating_it() {
    use op_editor_core::{walkers::find_node, NodeId};

    let root: PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame",
        "id": "search",
        "name": "Waveform — Search",
        "width": 390,
        "height": "fit_content",
        "layout": "vertical",
        "children": [
            {"type":"frame","id":"header","name":"Header","width":"fill_container","height":118},
            {"type":"frame","id":"tabs","name":"Tab Bar","width":"fill_container","height":82},
            {
                "type":"frame","id":"n458","name":"Status Bar",
                "width":"fill_container","height":"fit_content","layout":"horizontal",
                "children":[{"type":"text","id":"n459","content":"9:41"}]
            }
        ]
    }))
    .expect("mobile root");
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.active_children_mut().push(root);
    let old_bar = serde_json::to_value(
        find_node(state.active_children(), &NodeId::new("n458")).expect("existing status bar"),
    )
    .unwrap();
    let nodes = vec![modify_op(
        "search",
        serde_json::json!({
            "type": "frame",
            "name": "状态栏",
            "width": "fill_container",
            "height": 44,
            "children": []
        }),
    )];

    let (count, mutated) = apply_modify_ops_to_frame(&mut state, &nodes, "search");

    assert_eq!(count, 1);
    assert!(mutated);
    let root = find_node(state.active_children(), &NodeId::new("search")).unwrap();
    let children = root.children().unwrap();
    assert_eq!(
        children.len(),
        3,
        "must reuse the existing bar, not add a duplicate"
    );
    assert_eq!(children[0].id_str(), "n458");
    assert_eq!(children[0].base().role.as_deref(), Some("status-bar"));
    let mut expected = old_bar;
    expected["role"] = serde_json::Value::String("status-bar".into());
    assert_eq!(serde_json::to_value(&children[0]).unwrap(), expected);
    assert_eq!(children[1].id_str(), "header");
    assert_eq!(children[2].id_str(), "tabs");
}

#[test]
fn apply_modification_inserts_idless_null_parent_under_selected_frame() {
    let mut state = state_with_page();
    let nodes = vec![modify_op(
        "null",
        serde_json::json!({
            "type": "text",
            "name": "Loose Label",
            "content": "Hello"
        }),
    )];

    let (count, mutated) = apply_modify_ops_to_frame(&mut state, &nodes, "page-1");

    assert_eq!(count, 1);
    assert!(mutated);
    let page = state
        .active_children()
        .iter()
        .find(|n| n.id_str() == "page-1")
        .unwrap();
    let kids = page.children().unwrap();
    assert!(
        kids.iter()
            .any(|k| k.base().name.as_deref().is_some_and(|n| n == "Loose Label")),
        "idless null-parent node inserts under the captured target frame"
    );
}

// ---------------------------------------------------------------------------
// run_cli_turn routing
// ---------------------------------------------------------------------------
