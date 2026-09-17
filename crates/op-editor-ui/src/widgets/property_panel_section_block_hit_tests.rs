//! The Section block's geometry and paint, as tests.
//!
//! Split out of `property_panel_section_block.rs` at the 800-line cap — the same
//! reason that file exists at all. The module is declared with `#[path]` so the
//! test names stay what they were.

use super::*;
use crate::widgets::property_panel::PropertyPanel;
use crate::widgets::property_panel_inputs::{HEADER_HEIGHT, TAB_HEIGHT};
#[allow(unused_imports)]
use crate::widgets::Widget as _;
use crate::Rect;
use op_editor_core::editor_ui_state::section_panel::SummaryField;
use op_editor_core::section::{SectionProperties, SectionSummary};
use op_editor_core::{EditorState, NodeId};

const VIEWPORT: (f32, f32) = (1440.0, 900.0);
const PANEL_WIDTH: f32 = 280.0;

/// A panel whose selection is a section carrying one answered question, on
/// the tab that shows the block.
///
/// `Overview` explicitly: a section's two tabs split what used to be one
/// column, so «Дизайн» carries only design and the block is the «Обзор»
/// tab's whole content. A section left on the global default therefore
/// opens on Design, and a test about the block has to say which tab it
/// means.
fn panel_with_a_section() -> (PropertyPanel, Rect) {
    let mut state = EditorState::sample();
    state.editor_ui.property_tab = op_editor_core::PropertyTab::Overview;
    let node = state.selection.anchor.clone();
    state.editor_ui.section_panel.select(Some(node.clone()));
    state.editor_ui.section_panel.apply(
        &node,
        SectionProperties {
            summary: SectionSummary {
                what_it_is: "Checkout".to_string(),
                ..SectionSummary::default()
            },
            ..SectionProperties::empty()
        },
        Vec::new(),
    );
    let panel_rect = Rect::xywh(
        VIEWPORT.0 - PANEL_WIDTH,
        crate::widgets::TOP_BAR_HEIGHT,
        PANEL_WIDTH,
        VIEWPORT.1 - crate::widgets::TOP_BAR_HEIGHT,
    );
    let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selected section");
    (panel, panel_rect)
}

/// The strings the pass drew, in paint order.
fn painted(panel: &PropertyPanel, panel_rect: Rect) -> String {
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    panel.paint(
        &mut crate::widgets::PaintCx {
            backend: &mut backend,
        },
        panel_rect,
    );
    backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn overview_paints_the_block_and_no_design_section() {
    // The bug this exists for: the two tabs were split in the state and in
    // the hit-test, and the PAINT pass kept drawing the design half on
    // «Обзор» — it reads `caps` and the snapshot for most sections rather
    // than the visibility mask. Found on the live editor, where the tab said
    // «Обзор» and Position/Flex/Fill were underneath the block.
    let mut state = EditorState::sample();
    state.editor_ui.property_tab_section = op_editor_core::PropertyTab::Overview;
    let node = state.selection.anchor.clone();
    state.editor_ui.section_panel.select(Some(node.clone()));
    state.editor_ui.section_panel.apply(
        &node,
        SectionProperties {
            summary: SectionSummary {
                what_it_is: "Checkout".to_string(),
                ..SectionSummary::default()
            },
            ..SectionProperties::empty()
        },
        Vec::new(),
    );
    let panel_rect = Rect::xywh(0.0, 0.0, PANEL_WIDTH, 800.0);
    let labels =
        crate::widgets::property_panel_sections::PropertyLabels::for_editor_ui(&state.editor_ui);

    let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selected section");
    assert_eq!(
        panel.tab,
        op_editor_core::PropertyTab::Overview,
        "a section opens on its analytics"
    );
    let on_overview = painted(&panel, panel_rect);
    assert!(
        on_overview.contains("Checkout"),
        "the block's own sentences are the tab: {on_overview}"
    );
    for design_label in [labels.position, labels.flex_layout, labels.fill] {
        assert!(
            !on_overview.contains(design_label),
            "{design_label:?} belongs to «Дизайн», not «Обзор»: {on_overview}"
        );
    }

    // The same panel on «Дизайн» is the other half: design, and no block.
    state.editor_ui.property_tab_section = op_editor_core::PropertyTab::Design;
    let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selected section");
    assert_eq!(panel.tab, op_editor_core::PropertyTab::Design);
    let on_design = painted(&panel, panel_rect);
    assert!(
        on_design.contains(labels.position),
        "«Дизайн» is where the inspector lives: {on_design}"
    );
    assert!(
        !on_design.contains("Checkout"),
        "and the block is not on it: {on_design}"
    );
}

#[test]
fn the_block_sits_directly_under_the_node_header() {
    let (panel, panel_rect) = panel_with_a_section();

    let block = panel.section_block_rect(panel_rect).expect("a block");

    assert_eq!(
        block.origin.y,
        panel_rect.origin.y + TAB_HEIGHT + HEADER_HEIGHT,
        "the same place paint puts it, with nothing scrolled"
    );
    assert_eq!(block.size.y, panel.section_block_height);
    assert!(block.size.y > 0.0);
}

#[test]
fn a_question_is_clickable_where_it_is_painted() {
    let (panel, panel_rect) = panel_with_a_section();
    let block = panel.section_block_rect(panel_rect).expect("a block");

    let fields = section_field_rects(
        &panel.section_panel,
        block.origin.x,
        block.origin.y,
        block.size.x,
    );

    assert_eq!(fields.len(), 1, "one answered question, one rect");
    assert_eq!(fields[0].0, SummaryField::WhatItIs);
    let inside = Point2D::new(fields[0].1.origin.x + 4.0, fields[0].1.origin.y + 4.0);
    assert!(block.contains(inside), "the field is inside the block");
    assert!(fields[0].1.contains(inside));
}

#[test]
fn a_selection_that_is_not_a_section_has_no_block() {
    let state = EditorState::sample();
    let panel_rect = Rect::xywh(
        VIEWPORT.0 - PANEL_WIDTH,
        crate::widgets::TOP_BAR_HEIGHT,
        PANEL_WIDTH,
        VIEWPORT.1 - crate::widgets::TOP_BAR_HEIGHT,
    );
    let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selection");

    assert!(panel.section_block_rect(panel_rect).is_none());
    let _ = NodeId::new("unused");
}
