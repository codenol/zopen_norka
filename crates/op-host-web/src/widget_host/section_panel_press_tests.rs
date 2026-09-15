//! The Section block's press, typing and commit, driven through the host.
//!
//! The arithmetic of where the block paints is proved beside the widget
//! (`property_panel_section_block`); what these prove is the wiring — that a
//! press on a question focuses it, that a keystroke reaches the draft rather
//! than the canvas shortcuts, and that Enter queues exactly one write of the
//! whole properties object.
//!
//! Driven through `apply_press` / `apply_text` / `apply_send` rather than
//! through the tier helpers, because that is what the browser calls: a test of
//! the tier alone would pass while the ladder above it swallowed the press.

use super::WidgetHost;
use op_editor_core::editor_ui_state::section_panel::SummaryField;
use op_editor_core::section::{SectionProperties, SectionSummary};
use op_editor_core::{NodeId, PropertyTab};
use op_editor_ui::widgets::{PropertyPanel, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

const VIEWPORT_W: f32 = 1440.0;
const VIEWPORT_H: f32 = 900.0;

/// A host whose selected node is a section carrying one answered question.
fn host_with_a_section() -> WidgetHost {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.property_tab = PropertyTab::Design;
    // A node from the starter document, so the inspector has something to
    // build itself around. Which node it is does not matter to the block: the
    // block paints for a selected SECTION, and the state says which section
    // that is.
    host.editor_state.set_single_selection(NodeId::new("n10"));
    let node = host.editor_state.selection.anchor.clone();
    host.editor_state
        .editor_ui
        .section_panel
        .select(Some(node.clone()));
    host.editor_state.editor_ui.section_panel.apply(
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
    host
}

/// The panel rect the host uses, and the block inside it.
fn block_rect(host: &WidgetHost) -> Rect {
    let panel_rect = Rect::xywh(
        VIEWPORT_W - host.editor_state.editor_ui.property_panel_width,
        TOP_BAR_HEIGHT,
        host.editor_state.editor_ui.property_panel_width,
        VIEWPORT_H - TOP_BAR_HEIGHT,
    );
    PropertyPanel::for_selection_at(&host.editor_state, 0)
        .expect("a selected node")
        .section_block_rect(panel_rect)
        .expect("the block paints for a section")
}

/// The centre of the first answered question, in screen coordinates.
fn first_field_point(host: &WidgetHost) -> Point2D {
    let block = block_rect(host);
    let fields = op_editor_ui::widgets::property_panel_section_block::section_field_rects(
        &host.editor_state.editor_ui.section_panel,
        block.origin.x,
        block.origin.y,
        block.size.x,
    );
    let (field, rect) = fields.first().expect("an answered question");
    assert_eq!(*field, SummaryField::WhatItIs);
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn a_press_on_a_question_focuses_it() {
    let mut host = host_with_a_section();
    let point = first_field_point(&host);

    assert!(host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H));

    assert_eq!(
        host.editor_state.editor_ui.section_panel.focus,
        Some(SummaryField::WhatItIs)
    );
    assert_eq!(
        host.editor_state.editor_ui.section_panel.draft.text(),
        "Checkout",
        "the draft starts from what is stored"
    );
}

#[test]
fn a_keystroke_reaches_the_draft_instead_of_the_canvas() {
    // The letters that are also canvas tools are the ones worth typing here:
    // `r` selects the rectangle tool when nothing owns the keyboard.
    let mut host = host_with_a_section();
    let point = first_field_point(&host);
    host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H);
    let tool_before = host.editor_state.tool;

    for character in "Rect".chars() {
        assert!(host.apply_text(character));
    }

    assert_eq!(
        host.editor_state.editor_ui.section_panel.draft.text(),
        "CheckoutRect"
    );
    assert_eq!(
        host.editor_state.tool, tool_before,
        "a bare letter in a focused question must not switch tools"
    );
}

#[test]
fn enter_queues_one_write_of_the_whole_properties_object() {
    let mut host = host_with_a_section();
    let point = first_field_point(&host);
    host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H);
    host.apply_text('!');

    assert!(host.apply_send());

    let queued = host
        .editor_state
        .editor_ui
        .section_panel
        .pending_save
        .clone()
        .expect("a queued write");
    assert_eq!(queued.summary.what_it_is, "Checkout!");
    assert!(
        host.editor_state.editor_ui.section_panel.saving,
        "the panel is busy until the daemon answers"
    );
    // A second Enter while the write is in flight queues nothing NEW: two
    // writes racing would land in whichever order the network chose, so the
    // queued one is still the only one.
    assert!(host.apply_send());
    assert_eq!(
        host.editor_state
            .editor_ui
            .section_panel
            .pending_save
            .as_ref()
            .map(|queued| queued.summary.what_it_is.as_str()),
        Some("Checkout!"),
        "still the one write, unchanged"
    );
}

#[test]
fn the_attach_control_asks_the_host_for_a_file() {
    let mut host = host_with_a_section();
    let block = block_rect(&host);
    let attach = op_editor_ui::widgets::property_panel_section_block::section_attach_rect(
        &host.editor_state.editor_ui.section_panel,
        block.origin.x,
        block.origin.y,
        block.size.x,
    )
    .expect("the control paints with the block");

    assert!(host.apply_press(
        attach.origin.x + attach.size.x / 2.0,
        attach.origin.y + attach.size.y / 2.0,
        VIEWPORT_W,
        VIEWPORT_H,
    ));

    assert!(
        host.editor_state
            .editor_ui
            .section_panel
            .wants_analytics_file,
        "the widget layer owns no files: it asks the host"
    );
    assert_eq!(
        host.editor_state.editor_ui.section_panel.focus, None,
        "and the press is not read as a press on a question"
    );
}

#[test]
fn escape_gives_the_keyboard_back_without_saving() {
    let mut host = host_with_a_section();
    let point = first_field_point(&host);
    host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H);
    host.apply_text('!');

    assert!(host.apply_escape());

    assert_eq!(host.editor_state.editor_ui.section_panel.focus, None);
    assert!(host
        .editor_state
        .editor_ui
        .section_panel
        .pending_save
        .is_none());
}

#[test]
fn a_press_beside_the_questions_leaves_the_field() {
    let mut host = host_with_a_section();
    let point = first_field_point(&host);
    host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H);
    assert!(host.editor_state.editor_ui.section_panel.focus.is_some());
    // The block's own title row, above every question.
    let block = block_rect(&host);

    host.apply_press(
        block.origin.x + block.size.x / 2.0,
        block.origin.y + 6.0,
        VIEWPORT_W,
        VIEWPORT_H,
    );

    assert_eq!(
        host.editor_state.editor_ui.section_panel.focus, None,
        "a press off the questions is not a press on the last one"
    );
}

/// A section the daemon has not answered yet has no questions to press — the
/// block says so instead of offering four empty ones.
#[test]
fn an_unread_section_offers_nothing_to_edit() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.property_tab = PropertyTab::Design;
    host.editor_state.set_single_selection(NodeId::new("n10"));
    let node = host.editor_state.selection.anchor.clone();
    host.editor_state.editor_ui.section_panel.select(Some(node));

    let block = block_rect(&host);
    let fields = op_editor_ui::widgets::property_panel_section_block::section_field_rects(
        &host.editor_state.editor_ui.section_panel,
        block.origin.x,
        block.origin.y,
        block.size.x,
    );

    assert!(fields.is_empty());
    let _ = NodeId::new("unused");
}
