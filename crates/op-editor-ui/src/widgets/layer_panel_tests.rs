//! Tests for `widgets::layer_panel` — moved to a sibling file to
//! keep `layer_panel.rs` under the 800-line cap.
//!
//! Phase 6: the panel now builds from `op_editor_core::EditorState`,
//! so the fixtures construct `EditorState` values instead of the old
//! shell-core `Document`.

use super::layer_panel::*;
use super::layer_panel_walkers::{row_index_at, visible_row_range};
use super::Widget;
use crate::{Point2D, Rect};
use op_editor_core::EditorState;
use op_editor_core::NodeId;

// The fixtures and the recording backend live in a sibling; the test cases
// above keep their names.
#[path = "layer_panel_tests_support.rs"]
mod support;

use support::*;

#[test]
fn owned_build_reuses_cache_on_unchanged_inputs() {
    let owner = LayerPanel::next_layer_panel_owner();
    let state = EditorState::sample();
    // Prime the slot for this owner, then a second identical resolve must
    // NOT recompute (the whole point of the row-model cache).
    let _ = LayerPanel::from_editor_owned(&state, owner);
    let before = super::layer_panel_cache::layer_row_build_count();
    let panel = LayerPanel::from_editor_owned(&state, owner);
    let after = super::layer_panel_cache::layer_row_build_count();
    assert_eq!(after, before, "unchanged inputs must reuse the cached rows");
    assert_eq!(panel.items.len(), 5);
}

#[test]
fn owned_build_rebuilds_on_document_mutation() {
    let owner = LayerPanel::next_layer_panel_owner();
    let mut state = EditorState::sample();
    let _ = LayerPanel::from_editor_owned(&state, owner);
    let before = super::layer_panel_cache::layer_row_build_count();
    // Bumping the document revision is THE content-change signal the key
    // rides — it must invalidate the cached rows.
    state.mark_document_changed();
    let _ = LayerPanel::from_editor_owned(&state, owner);
    let after = super::layer_panel_cache::layer_row_build_count();
    assert_eq!(
        after,
        before + 1,
        "a document mutation must rebuild the row model"
    );
}

#[test]
fn owned_build_reuses_cache_on_styling_only_change() {
    let owner = LayerPanel::next_layer_panel_owner();
    let mut state = EditorState::starter();
    let _ = LayerPanel::from_editor_owned(&state, owner);
    let before = super::layer_panel_cache::layer_row_build_count();
    // Selection + hover are styling-only overlays: they must NOT rebuild
    // the rows, yet the panel must still reflect them live.
    state.set_single_selection(NodeId::new("n10"));
    state.editor_ui.hovered_layer_id = Some(NodeId::new("n10"));
    let panel = LayerPanel::from_editor_owned(&state, owner);
    let after = super::layer_panel_cache::layer_row_build_count();
    assert_eq!(
        after, before,
        "selection/hover-only change must reuse cached rows"
    );
    assert!(panel.is_row_selected(&NodeId::new("n10")));
    assert!(panel.is_row_hovered(&NodeId::new("n10")));
}

#[test]
fn whole_document_replacement_needs_owner_rotation_to_avoid_stale_rows() {
    // Two DIFFERENT documents that both start at revision 0 with the same
    // active page index — so their `LayerRowKey`s (revision + page +
    // collapsed_fp + rename_fp) are byte-identical. This is exactly the
    // whole-document-replacement aliasing the reviewer flagged: opening /
    // importing / MCP-replacing a document restarts the revision at 0 while the
    // host's layer-panel owner never rotates on its own.
    let doc_a = four_rects(); // rows A/B/C/D
    let doc_b = state_from(
        r##"{ "version": "1.0.0", "children": [
              {"type":"rectangle","id":"m1","name":"W","width":10,"height":10},
              {"type":"rectangle","id":"m2","name":"X","width":10,"height":10}
        ]}"##,
    ); // rows W/X
    assert_eq!(
        doc_a.document_revision(),
        doc_b.document_revision(),
        "both freshly loaded documents must share a revision (the aliasing precondition)"
    );
    assert_eq!(doc_a.ui.active_page_index, doc_b.ui.active_page_index);

    let owner = LayerPanel::next_layer_panel_owner();
    let primed = LayerPanel::from_editor_owned(&doc_a, owner);
    assert_eq!(primed.items.len(), 4, "primed rows are document A's");

    // Without rotation the colliding (rev 0, page 0) key serves document A's
    // STALE rows for document B — the bug the owner rotation fixes.
    let stale = LayerPanel::from_editor_owned(&doc_b, owner);
    assert_eq!(
        stale.items.len(),
        4,
        "same owner + colliding key serves the PREVIOUS document's cached rows"
    );

    // Rotating the owner (what the hosts now do at every replacement seam) makes
    // the next owned resolve miss the stale slot and rebuild against document B.
    let rotated = LayerPanel::next_layer_panel_owner();
    let before = super::layer_panel_cache::layer_row_build_count();
    let fresh = LayerPanel::from_editor_owned(&doc_b, rotated);
    let after = super::layer_panel_cache::layer_row_build_count();
    assert_eq!(
        after,
        before + 1,
        "a rotated owner must rebuild the row model after replacement"
    );
    assert_eq!(
        fresh.items.len(),
        2,
        "rebuilt rows reflect the NEW document"
    );
    assert_eq!(fresh.items[0].label, "W");
    assert_eq!(fresh.items[1].label, "X");
}

#[test]
fn from_sample_doc_flattens_to_5_layer_rows() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.items.len(), 5);
    assert_eq!(panel.items[0].label, "Frame");
    assert_eq!(panel.items[0].depth, 0);
    assert_eq!(panel.items[1].depth, 1);
}

#[test]
fn from_sample_doc_has_one_active_page() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.pages.len(), 1);
    assert!(panel.pages[0].active);
    assert_eq!(panel.pages[0].label, "Page 1");
}

#[test]
fn selection_flag_marks_only_selected_row() {
    let state = EditorState::sample(); // selection anchors on n11
    let panel = LayerPanel::from_editor(&state);
    let selected = panel
        .items
        .iter()
        .filter(|i| panel.is_row_selected(&i.node_id))
        .count();
    assert_eq!(selected, 1);
}

#[test]
fn empty_document_yields_one_default_page_no_layers() {
    let state = EditorState::new();
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.pages.len(), 1);
    assert!(panel.items.is_empty());
}

#[test]
fn collapsed_node_hides_its_children() {
    let state = state_from(
        r##"{ "version": "1.0.0", "children": [
              {"type":"frame","id":"n1","name":"Frame","width":100,"height":100,
               "children":[
                 {"type":"rectangle","id":"n2","name":"Child","width":10,"height":10}
               ]}
        ]}"##,
    );
    let mut collapsed = state;
    collapsed
        .editor_ui
        .collapsed_layers
        .insert(op_editor_core::NodeId::new("n1"));
    let panel = LayerPanel::from_editor(&collapsed);
    // Only the frame row paints; the collapsed child is hidden.
    assert_eq!(panel.items.len(), 1);
    assert!(panel.items[0].collapsed);
}

#[test]
fn unnamed_node_row_falls_back_to_kind_label() {
    // MCP `batch_design` / `insert_node` can create nodes with no
    // `name`. The layer row must then show the kind label (TS parity:
    // `node.name ?? node.type`) instead of a blank label.
    let state = state_from(
        r##"{ "version": "1.0.0", "children": [
              {"type":"rectangle","id":"n1","width":10,"height":10},
              {"type":"frame","id":"n2","width":20,"height":20},
              {"type":"rectangle","id":"n3","name":"Hero","width":10,"height":10}
        ]}"##,
    );
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.items.len(), 3);
    assert_eq!(
        panel.items[0].label, "Rectangle",
        "unnamed rect → kind label"
    );
    assert_eq!(panel.items[1].label, "Frame", "unnamed frame → kind label");
    assert_eq!(panel.items[2].label, "Hero", "named node keeps its name");
}

#[test]
fn ghost_item_for_unnamed_node_falls_back_to_kind_label() {
    // The drag-ghost row (host paints it at the cursor) shares the same
    // name-or-kind fallback so a nameless node isn't a blank ghost.
    let state = state_from(
        r##"{ "version": "1.0.0", "children": [
              {"type":"ellipse","id":"n1","width":10,"height":10}
        ]}"##,
    );
    let ghost = LayerPanel::ghost_item_for(&state, &NodeId::new("n1"))
        .expect("ghost item for an on-page node");
    assert_eq!(ghost.label, "Ellipse");
}

#[test]
fn hit_test_resolves_first_layer_row() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let layer_y = panel.regions(rect).layers_rows_top + LAYER_ROW_HEIGHT / 2.0;
    let p = Point2D::new(rect.size.x / 2.0, layer_y);
    match panel.hit_test(rect, p) {
        Some(LayerPanelHit::Layer(id)) => assert_eq!(id, panel.items[0].node_id),
        other => panic!("expected first layer hit, got {:?}", other),
    }
}

#[test]
fn selected_visible_unlocked_layer_does_not_expose_trailing_actions_without_hover() {
    let mut state = EditorState::starter();
    state.set_single_selection(NodeId::new("n10"));
    let panel = LayerPanel::from_editor(&state);
    assert!(panel.is_row_selected(&panel.items[0].node_id));
    assert!(!panel.is_row_hovered(&panel.items[0].node_id));
    assert!(!panel.items[0].hidden);
    assert!(!panel.items[0].locked);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let (eye, lock) = first_layer_trailing_points(&panel, rect);

    assert!(matches!(
        panel.hit_test(rect, eye),
        Some(LayerPanelHit::Layer(_))
    ));
    assert!(matches!(
        panel.hit_test(rect, lock),
        Some(LayerPanelHit::Layer(_))
    ));
}

#[test]
fn hovered_layer_exposes_trailing_actions() {
    let mut state = EditorState::starter();
    state.editor_ui.hovered_layer_id = Some(NodeId::new("n10"));
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let (eye, lock) = first_layer_trailing_points(&panel, rect);

    assert_eq!(
        panel.hit_test(rect, eye),
        Some(LayerPanelHit::ToggleHidden(NodeId::new("n10")))
    );
    assert_eq!(
        panel.hit_test(rect, lock),
        Some(LayerPanelHit::ToggleLocked(NodeId::new("n10")))
    );
}

#[test]
fn hidden_locked_layer_does_not_expose_trailing_actions_without_hover() {
    let mut state = EditorState::starter();
    state.toggle_node_hidden(&NodeId::new("n10"));
    state.toggle_node_locked(&NodeId::new("n10"));
    let panel = LayerPanel::from_editor(&state);
    assert!(!panel.is_row_hovered(&panel.items[0].node_id));
    assert!(panel.items[0].hidden);
    assert!(panel.items[0].locked);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let (eye, lock) = first_layer_trailing_points(&panel, rect);

    assert!(matches!(
        panel.hit_test(rect, eye),
        Some(LayerPanelHit::Layer(_))
    ));
    assert!(matches!(
        panel.hit_test(rect, lock),
        Some(LayerPanelHit::Layer(_))
    ));
}

#[test]
fn hidden_locked_layer_does_not_paint_trailing_actions_without_hover() {
    let mut state = EditorState::starter();
    state.toggle_node_hidden(&NodeId::new("n10"));
    state.toggle_node_locked(&NodeId::new("n10"));
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let eye_top_left = first_layer_eye_top_left(&panel, rect);
    let lock_top_left = first_layer_lock_top_left(&panel, rect);
    let mut backend = LayerPaintBackend::default();
    let mut cx = super::PaintCx {
        backend: &mut backend,
    };

    panel.paint(&mut cx, rect);

    assert!(!backend
        .strokes
        .iter()
        .any(|(top_left, size, _)| approx_point(*top_left, eye_top_left)
            && (*size - 12.0).abs() < 1e-4));
    assert!(!backend
        .strokes
        .iter()
        .any(|(top_left, size, _)| approx_point(*top_left, lock_top_left)
            && (*size - 12.0).abs() < 1e-4));
}

#[test]
fn hovered_hidden_layer_eye_icon_uses_ts_yellow_state_color() {
    let mut state = EditorState::starter();
    state.toggle_node_hidden(&NodeId::new("n10"));
    state.editor_ui.hovered_layer_id = Some(NodeId::new("n10"));
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let eye_top_left = first_layer_eye_top_left(&panel, rect);
    let mut backend = LayerPaintBackend::default();
    let mut cx = super::PaintCx {
        backend: &mut backend,
    };

    panel.paint(&mut cx, rect);

    assert!(backend.strokes.iter().any(|(top_left, size, color)| {
        approx_point(*top_left, eye_top_left)
            && (*size - 12.0).abs() < 1e-4
            && is_yellow_400(*color)
    }));
}

#[test]
fn hit_test_resolves_add_page_plus_icon() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let plus_x = rect.size.x - ROW_PAD_X - 12.0;
    let plus_y = 8.0 + (SECTION_HEADER_HEIGHT - 14.0) / 2.0;
    assert_eq!(
        panel.hit_test(rect, Point2D::new(plus_x + 7.0, plus_y + 7.0)),
        Some(LayerPanelHit::AddPage)
    );
    assert_eq!(
        panel.hit_test(rect, Point2D::new(plus_x - 3.0, plus_y + 7.0)),
        Some(LayerPanelHit::AddPage)
    );
}

#[test]
fn hit_test_resolves_first_page_row() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let page_y = 8.0 + SECTION_HEADER_HEIGHT + PAGE_ROW_HEIGHT / 2.0;
    let p = Point2D::new(rect.size.x / 2.0, page_y);
    assert_eq!(panel.hit_test(rect, p), Some(LayerPanelHit::Page(0)));
}

#[test]
fn access_node_advertises_tree_role_and_layers_label() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let node = panel.access_node();
    assert_eq!(node.role(), accesskit::Role::Tree);
    assert_eq!(node.label(), Some("Layers"));
}

#[test]
fn from_document_scopes_to_active_page_only() {
    let mut state = state_from(
        r##"{ "version": "1.0.0", "children": [], "pages": [
              {"id":"n1","name":"Page 1","children":[
                {"type":"frame","id":"n2","name":"P1-Node","width":10,"height":10}]},
              {"id":"n3","name":"Page 2","children":[
                {"type":"frame","id":"n4","name":"P2-Node","width":10,"height":10}]}
        ]}"##,
    );
    state.ui.active_page_index = 1;
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.items.len(), 1);
    assert_eq!(panel.items[0].label, "P2-Node");
    assert_eq!(panel.pages.len(), 2);
    assert!(!panel.pages[0].active);
    assert!(panel.pages[1].active);
}

#[test]
fn drop_indicator_matches_post_commit_layout_when_dragging_down() {
    let mut state = four_rects();
    let panel = LayerPanel::from_editor_with_drag_source(&state, &NodeId::new("n1"));
    assert_eq!(panel.items.len(), 3); // A excluded → [B, C, D]
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let layers_top = panel.regions(rect).layers_rows_top;
    let row_top_of_d = layers_top + 2.0 * LAYER_ROW_HEIGHT;
    let drop = panel
        .drop_target_at(rect, Point2D::new(rect.size.x / 2.0, row_top_of_d + 4.0))
        .unwrap();
    assert_eq!(drop.position, DropPosition::Before);
    assert!((drop.indicator_y - row_top_of_d).abs() < 0.5);
    // Commit and check A's new row top matches indicator_y.
    assert!(state.reorder_before(op_editor_core::NodeId::new("n1"), drop.anchor.clone()));
    let post = LayerPanel::from_editor(&state);
    let a_idx = post
        .items
        .iter()
        .position(|i| i.node_id == NodeId::new("n1"))
        .unwrap();
    let a_row_top = layers_top + a_idx as f32 * LAYER_ROW_HEIGHT;
    assert!(
        (drop.indicator_y - a_row_top).abs() < 0.5,
        "preview/commit y mismatch: indicator at {} but A lands at {}",
        drop.indicator_y,
        a_row_top
    );
}

#[test]
fn drop_target_at_resolves_before_and_after_halves() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    let y0 = panel.regions(rect).layers_rows_top;
    let mid_x = rect.size.x / 2.0;
    let before = panel
        .drop_target_at(rect, Point2D::new(mid_x, y0 + 4.0))
        .unwrap();
    assert_eq!(before.anchor, panel.items[0].node_id);
    assert_eq!(before.position, DropPosition::Before);
    assert!((before.indicator_y - y0).abs() < 0.5);
    let after = panel
        .drop_target_at(rect, Point2D::new(mid_x, y0 + LAYER_ROW_HEIGHT - 4.0))
        .unwrap();
    assert_eq!(after.position, DropPosition::After);
    assert!((after.indicator_y - (y0 + LAYER_ROW_HEIGHT)).abs() < 0.5);
}

#[test]
fn drop_target_at_in_empty_area_below_rows_drops_at_end() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height() + 200.0),
    };
    let layers_top = panel.regions(rect).layers_rows_top;
    let rows_bottom = layers_top + panel.items.len() as f32 * LAYER_ROW_HEIGHT;
    let drop = panel
        .drop_target_at(rect, Point2D::new(rect.size.x / 2.0, rows_bottom + 50.0))
        .expect("below-rows hit should drop at end");
    assert_eq!(drop.position, DropPosition::After);
    assert_eq!(drop.anchor, panel.items.last().unwrap().node_id);
    assert!((drop.indicator_y - rows_bottom).abs() < 0.5);
}

#[test]
fn deep_layer_tree_exposes_horizontal_scroll_range() {
    run_deep_layer_fixture(|| {
        let state = state_from(&nested_frame_doc(50));
        let panel = LayerPanel::from_editor(&state);
        let rect = Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(LAYER_PANEL_WIDTH, 700.0),
        };

        let regions = panel.regions(rect);

        assert!(
            regions.layers.content_width > rect.size.x,
            "deep layer content should be wider than the fixed panel viewport"
        );
        assert!(
            regions.layers.max_horizontal_offset > 0.0,
            "deep layer rows need a horizontal scroll range"
        );
    });
}

#[test]
fn layer_horizontal_scroll_offset_is_clamped() {
    run_deep_layer_fixture(|| {
        let mut state = state_from(&nested_frame_doc(50));
        state.editor_ui.layer_layers_h_scroll.offset = 10_000.0;
        let panel = LayerPanel::from_editor(&state);
        let rect = Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(LAYER_PANEL_WIDTH, 700.0),
        };

        let regions = panel.regions(rect);

        assert_eq!(
            regions.layers.horizontal_offset,
            regions.layers.max_horizontal_offset
        );
    });
}

#[test]
fn layer_panel_caches_content_widths_on_build() {
    run_deep_layer_fixture(|| {
        let state = state_from(&nested_frame_doc(50));
        let panel = LayerPanel::from_editor(&state);
        let rect = Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(LAYER_PANEL_WIDTH, 700.0),
        };

        assert!(panel.layers_scroll.content_width > rect.size.x);
        assert_eq!(
            panel.regions(rect).layers.content_width,
            panel.layers_scroll.content_width
        );
        assert_eq!(
            panel.regions(rect).pages.content_width,
            panel.pages_scroll.content_width
        );
    });
}

#[test]
fn visible_row_range_jumps_directly_to_scrolled_rows() {
    let range = visible_row_range(
        1_000,
        LAYER_ROW_HEIGHT * 500.0,
        LAYER_ROW_HEIGHT * 10.0,
        LAYER_ROW_HEIGHT,
    );

    assert_eq!(range.start, 500);
    assert!(
        range.end <= 512,
        "visible range should cover only the viewport plus small overscan, got {range:?}"
    );
}

#[test]
fn row_index_at_maps_scrolled_point_to_item_without_linear_scan() {
    let rows_top = 80.0;
    let scroll = LAYER_ROW_HEIGHT * 500.0;
    let point_y = rows_top + LAYER_ROW_HEIGHT * 2.5;

    let (index, row_top) = row_index_at(
        1_000,
        rows_top,
        scroll,
        LAYER_ROW_HEIGHT * 10.0,
        LAYER_ROW_HEIGHT,
        point_y,
    )
    .expect("point lands on a visible row");

    assert_eq!(index, 502);
    assert!((row_top - (rows_top + LAYER_ROW_HEIGHT * 2.0)).abs() < 0.01);
}

/// Pages rows are 32 px tall; the visible-row window and hit-test must
/// use PAGE_ROW_HEIGHT, not the 28 px layer height. With 45 pages
/// scrolled near the bottom, the window must still start at the row
/// whose top the offset lands on — using the wrong height skips rows
/// and leaves a blank gap above the last page.
#[test]
fn pages_visible_range_uses_page_row_height() {
    // Scroll so the top of row 39 is at the viewport top.
    let scroll = PAGE_ROW_HEIGHT * 39.0;
    let range = visible_row_range(45, scroll, PAGE_ROW_HEIGHT * 6.0, PAGE_ROW_HEIGHT);
    assert_eq!(
        range.start, 39,
        "pages window must start at the 32px-row index, got {range:?}"
    );
    assert!(
        range.end >= 45,
        "window must reach the last page, got {range:?}"
    );
}

#[test]
fn pages_hit_test_uses_page_row_height() {
    let rows_top = 60.0;
    let scroll = PAGE_ROW_HEIGHT * 39.0;
    // Cursor 2.5 rows down from the viewport top → row 41.
    let point_y = rows_top + PAGE_ROW_HEIGHT * 2.5;
    let (index, row_top) = row_index_at(
        45,
        rows_top,
        scroll,
        PAGE_ROW_HEIGHT * 6.0,
        PAGE_ROW_HEIGHT,
        point_y,
    )
    .expect("point lands on a page row");
    assert_eq!(index, 41, "hit-test must map to the 32px-row index");
    assert!((row_top - (rows_top + PAGE_ROW_HEIGHT * 2.0)).abs() < 0.01);
}

#[test]
fn component_store_pages_are_listed_between_pages_and_layers() {
    let mut state = EditorState::new();
    let master = serde_json::from_value(serde_json::json!({
        "id": "atom-btn",
        "type": "frame",
        "name": "Button/Default",
        "reusable": true,
        "width": 80,
        "height": 32
    }))
    .expect("master");
    assert_eq!(state.append_components_page_masters(vec![master]), 1);
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(panel.pages.len(), 1);
    assert_eq!(panel.pages[0].label, "Page 1");
    assert_eq!(panel.components.len(), 1);
    assert_eq!(panel.components[0].label, "Button");
}

/// The Recipes section is a first-class rail section: its rows must be
/// hittable, and they are listed even for a document with no rules.
#[test]
fn recipe_rows_are_hittable_in_the_rail() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, panel.intrinsic_height()),
    };
    assert!(!panel.recipes.is_empty(), "the session kit ships recipes");
    let r = panel.regions(rect);
    assert!(
        r.recipes_view_h > 0.0,
        "the recipes region must be laid out"
    );
    let p = Point2D::new(
        rect.size.x / 2.0,
        r.recipes_rows_top + PAGE_ROW_HEIGHT / 2.0,
    );
    match panel.hit_test(rect, p) {
        Some(LayerPanelHit::Recipe(index)) => assert_eq!(index, 0),
        other => panic!("expected the first recipe row, got {other:?}"),
    }
}

/// The layer tree keeps a floor at any height the rail can be given.
///
/// Issue #66: the palettes above the tree (pages, components, recipes) each
/// capped their own height, but only on the touch layout — so on a short desktop
/// window they took the whole rail, `layers_view_h` came out as 0, and the panel
/// said "this document has no layers" while the document had them. The row was
/// not scrolled out of view; it was laid out past the bottom edge.
#[test]
fn the_layer_tree_keeps_room_at_every_panel_height() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    assert!(
        !panel.recipes.is_empty(),
        "the fixture has recipes to crowd it"
    );
    let metrics = panel.metrics;

    // From a rail shorter than its own fixed sections to a comfortable one.
    for height in [120.0_f32, 168.0, 200.0, 260.0, 340.0, 720.0] {
        let rect = Rect {
            origin: Point2D::new(0.0, 0.0),
            size: Point2D::new(LAYER_PANEL_WIDTH, height),
        };
        let r = panel.regions(rect);
        assert!(
            r.layers_view_h >= metrics.layer_row_height,
            "at height {height} the layer viewport collapsed to {}",
            r.layers_view_h
        );
        assert!(
            r.layers_rows_top + metrics.layer_row_height <= rect.origin.y + rect.size.y,
            "at height {height} the first layer row (y={}) is below the panel \
             (bottom={})",
            r.layers_rows_top,
            rect.origin.y + rect.size.y
        );
    }
}

/// A tall rail still lets the palettes show what they have.
#[test]
fn a_roomy_rail_still_shows_palette_rows() {
    let state = EditorState::sample();
    let panel = LayerPanel::from_editor(&state);
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(LAYER_PANEL_WIDTH, 720.0),
    };
    let r = panel.regions(rect);
    assert!(r.pages_view_h > 0.0, "pages keep their rows");
    assert!(
        r.recipes_view_h >= PAGE_ROW_HEIGHT,
        "and so do the recipes, when there is room for them: {}",
        r.recipes_view_h
    );
}
