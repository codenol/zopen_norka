use crate::{editor_state_to_active_page_layout_scene, editor_state_to_layout_scene};
use jian_ops_schema::variable::{VariableKind, VariableScalar};
use op_editor_core::{EditorState, NodeId, PenNodeExt};

fn state_from(src: &str) -> EditorState {
    EditorState::from_document(
        jian_ops_schema::load_str(src)
            .expect("fixture parses")
            .value,
    )
}

#[test]
fn interactive_scene_keeps_page_indices_but_builds_only_active_children() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"a","name":"A","children":[
              {"type":"rectangle","id":"a-rect","x":0,"y":0,"width":10,"height":10}
            ]},
            {"id":"b","name":"B","children":[
              {"type":"rectangle","id":"b-rect","x":20,"y":30,"width":40,"height":50}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let scene = editor_state_to_active_page_layout_scene(&state);

    assert_eq!(scene.pages.len(), 2);
    assert_eq!(scene.active_page_index, 1);
    assert!(scene.pages[0].children.is_empty());
    assert_eq!(scene.pages[1].children.len(), 1);
    assert_eq!(scene.pages[1].children[0].id, "b-rect");
    assert!(scene.content_bounds().is_some());
}

#[test]
fn active_page_refs_can_still_resolve_masters_from_an_inactive_page() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"button","name":"Button","reusable":true,
               "x":0,"y":0,"width":100,"height":40,"children":[
                 {"type":"rectangle","id":"button-bg","x":0,"y":0,"width":100,"height":40}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"button-instance","ref":"button",
               "x":50,"y":60,"width":100,"height":40}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let scene = editor_state_to_active_page_layout_scene(&state);
    let instance = scene.pages[1]
        .find("button-instance")
        .expect("instance expands from inactive-page master");

    assert_eq!(instance.children.len(), 1);
    assert_eq!(instance.children[0].id, "button-instance__button-bg");
    assert!(scene.pages[0].children.is_empty());
}

#[test]
fn active_page_refs_keep_legacy_non_reusable_target_fallback() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"legacy-button","name":"Legacy Button",
               "x":0,"y":0,"width":90,"height":36,"children":[
                 {"type":"rectangle","id":"legacy-bg","x":0,"y":0,
                  "width":90,"height":36}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"legacy-instance","ref":"legacy-button",
               "x":30,"y":40,"width":90,"height":36}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;
    assert!(
        state.components.is_empty(),
        "target is intentionally not reusable"
    );

    let scene = editor_state_to_active_page_layout_scene(&state);
    let instance = scene.pages[1]
        .find("legacy-instance")
        .expect("legacy non-reusable target resolves through the document fallback");

    assert_eq!(instance.children.len(), 1);
    assert_eq!(instance.children[0].id, "legacy-instance__legacy-bg");
}

#[test]
fn edited_component_master_uses_live_document_instead_of_stale_registry_snapshot() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"card","name":"Card","reusable":true,
               "x":0,"y":0,"width":100,"height":40,"children":[
                 {"type":"rectangle","id":"card-bg","x":0,"y":0,
                  "width":100,"height":40}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"card-instance","ref":"card",
               "x":20,"y":30,"width":100,"height":40}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let pages = state.doc.pages.as_mut().expect("fixture has pages");
    let master =
        op_editor_core::walkers::find_node_mut(&mut pages[0].children, &NodeId::new("card-bg"))
            .expect("master child exists");
    master.set_width_px(175.0);
    state.mark_document_changed();

    // The runtime component registry intentionally remains the load-time
    // prototype. An edited document must therefore bypass it and match the
    // canonical full builder.
    let full = editor_state_to_layout_scene(&state);
    let active = editor_state_to_active_page_layout_scene(&state);
    assert_eq!(active.pages[1], full.pages[1]);
}

#[test]
fn active_page_matches_full_builder_with_refs_variables_and_both_layout_modes() {
    let mut state = state_from(
        r##"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"card","name":"Card","reusable":true,
               "x":5,"y":7,"width":120,"height":60,"children":[
                 {"type":"rectangle","id":"card-bg","x":2,"y":3,
                  "width":116,"height":54,
                  "fill":[{"type":"solid","color":"#112233"}]}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"card-instance","ref":"card",
               "x":40,"y":50,"width":120,"height":60},
              {"type":"rectangle","id":"accent","x":200,"y":80,
               "width":30,"height":20,
               "fill":[{"type":"solid","color":"#000000"}]}
            ]}
          ]
        }"##,
    );
    state.ui.active_page_index = 1;
    state.create_variable(
        "brand",
        VariableKind::Color,
        VariableScalar::Str("#ff8800".into()),
    );
    state
        .ui
        .variables
        .fill_refs
        .insert(NodeId::new("accent"), "brand".into());

    for preserve in [false, true] {
        state.editor_ui.preserve_authored_geometry = preserve;
        let full = editor_state_to_layout_scene(&state);
        let active = editor_state_to_active_page_layout_scene(&state);

        assert_eq!(active.active_page_index, full.active_page_index);
        assert_eq!(active.pages.len(), full.pages.len());
        assert_eq!(active.pages[0].id, full.pages[0].id);
        assert_eq!(active.pages[0].name, full.pages[0].name);
        assert!(active.pages[0].children.is_empty());
        assert_eq!(active.pages[1], full.pages[1]);
    }
}

#[test]
fn the_scene_names_its_page_by_the_shared_page_identity() {
    // The canvas, the comment pins and the comment rail are all keyed on the
    // scene's page id, and the client's own page-scoped writes are keyed on
    // `active_page_identity`. Read through the same rule rather than through a
    // second copy of it: a synthesized single-page id that drifted would file a
    // comment under a page nobody could look at.
    let single = state_from(
        r#"{"version":"1.0.0","name":"Untitled","children":[
            {"type":"rectangle","id":"n1","x":0,"y":0,"width":10,"height":10}
        ]}"#,
    );
    let scene = editor_state_to_active_page_layout_scene(&single);
    assert_eq!(
        scene.active_page().map(|page| page.id.as_str()),
        Some(single.active_page_identity().0.as_str())
    );
    assert_eq!(single.active_page_identity().0, "page-1");

    let multi =
        state_from(r#"{"version":"1.0.0","pages":[{"id":"only","name":"Only","children":[]}]}"#);
    let scene = editor_state_to_active_page_layout_scene(&multi);
    assert_eq!(
        scene.active_page().map(|page| page.id.as_str()),
        Some(multi.active_page_identity().0.as_str())
    );
}
