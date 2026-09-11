//! The context-chip row.
//!
//! The row replaced two stacked full-width rows, so the assertions that
//! matter are the ones the old layout would fail: the chips share one line,
//! the line is no taller than a chip plus its padding, and a panel with
//! nothing to say reserves no height at all.

use super::*;
use crate::widgets::ai_chat_hit::AIChatHit;
use crate::widgets::asset_center_style_cards::style_test_support::exclusive_user_styles as exclusive_registry_for_tests;
use op_editor_core::EditorState;

const PANEL: Rect = Rect {
    origin: Point2D { x: 40.0, y: 60.0 },
    size: Point2D { x: 380.0, y: 520.0 },
};

const STYLE_MD: &str = "\
---
name: Dimension
---

## Tokens — Colors

| Name | Value | Token | Role |
| --- | --- | --- | --- |
| Void Canvas | `#0a0a0a` | `--color-void-canvas` | Primary page background, base surface |
| Bone | `#ededed` | `--color-bone` | Primary readable text on dark surfaces |
";

/// A state with a live pin AND a multi-node selection — the case the user
/// reported, where both chips were live at once.
fn state_with_both_chips() -> EditorState {
    let imported =
        op_ai_skills::style_guide::import_design_md(STYLE_MD, "d.md").expect("guide imports");
    let mut state = EditorState::new();
    state.editor_ui.pinned_style_guide = Some(imported.id.clone());
    state.selection.set = (0..4)
        .map(|idx| op_editor_core::NodeId::new(format!("n{idx}")))
        .collect();
    state
}

#[test]
fn both_chips_share_one_line() {
    let _guard = exclusive_registry_for_tests();
    let state = state_with_both_chips();
    let panel = AIChatPlaceholder::from_editor(&state);
    let input_rect = panel.input_rect(PANEL);

    let row = panel.chip_row(input_rect);
    let style = row.style.expect("a live pin shows its chip");
    let selection = row.selection.expect("a selection shows its chip");

    assert_eq!(
        style.origin.y, selection.origin.y,
        "the chips must sit on ONE line — stacking them is the bug this row fixed"
    );
    assert_eq!(style.size.y, selection.size.y);
    assert!(
        selection.origin.x >= style.origin.x + style.size.x,
        "the selection chip must follow the receipt horizontally, not overlap it"
    );
    assert!(
        selection.origin.x - (style.origin.x + style.size.x) <= 8.0,
        "the in-row gap must stay tight, got {}",
        selection.origin.x - (style.origin.x + style.size.x)
    );
    assert!(
        selection.origin.x + selection.size.x <= input_rect.origin.x + input_rect.size.x + 0.01,
        "the row must not overrun the input block"
    );
}

#[test]
fn the_row_is_no_taller_than_one_chip_plus_its_padding() {
    let _guard = exclusive_registry_for_tests();
    let state = state_with_both_chips();
    let panel = AIChatPlaceholder::from_editor(&state);

    let row_h = panel.chip_row_h();

    assert!(
        row_h <= CHIP_H + CHIP_ROW_PAD_Y * 2.0,
        "two chips must not cost two rows: {row_h}"
    );
    let chip = panel
        .chip_row(panel.input_rect(PANEL))
        .style
        .expect("a chip");
    assert!(
        chip.size.y <= CHIP_H,
        "the chip itself must stay small: {}",
        chip.size.y
    );
}

/// The rules chip is always present — the session's rules are in force with
/// nothing pinned — so the row always reserves its band above the textarea.
#[test]
fn the_rules_chip_keeps_its_band_above_the_text() {
    let state = EditorState::new();
    let panel = AIChatPlaceholder::from_editor(&state);
    let input_rect = panel.input_rect(PANEL);

    assert!(panel.chip_row_h() > 0.0);
    assert!(panel.chip_row(input_rect).style.is_some());
    assert!(
        panel.input_text_rect(PANEL).origin.y > input_rect.origin.y,
        "the text starts below the chip band"
    );
}

#[test]
fn one_live_chip_still_starts_at_the_left_edge() {
    let mut state = EditorState::new();
    state.selection.set = vec![op_editor_core::NodeId::new("only")];
    let panel = AIChatPlaceholder::from_editor(&state);
    let input_rect = panel.input_rect(PANEL);

    let row = panel.chip_row(input_rect);

    assert!(row.style.is_some(), "the rules chip is always present");
    let chip = row.selection.expect("a selection shows its chip");
    assert!(chip.origin.x >= input_rect.origin.x);
}

#[test]
fn each_clear_target_clears_its_own_chip() {
    let _guard = exclusive_registry_for_tests();
    let state = state_with_both_chips();
    let panel = AIChatPlaceholder::from_editor(&state);
    let input_rect = panel.input_rect(PANEL);

    assert!(
        panel.style_receipt_clear_rect(input_rect).is_none(),
        "the rules row has nothing to clear"
    );
    let selection_clear = panel
        .selection_chip_clear_rect(input_rect)
        .expect("a selection is always clearable");

    let centre = |rect: Rect| {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    };
    assert_eq!(
        panel.hit_test(PANEL, centre(selection_clear)),
        Some(AIChatHit::ClearSelection)
    );
}

#[test]
fn a_clear_target_is_never_smaller_than_the_sixteen_pixel_floor() {
    let _guard = exclusive_registry_for_tests();
    let state = state_with_both_chips();
    let panel = AIChatPlaceholder::from_editor(&state);
    let input_rect = panel.input_rect(PANEL);

    // Only the selection chip carries a ✕ now: the rules row cannot be
    // switched off, so it has no target to floor.
    for clear in [panel
        .selection_chip_clear_rect(input_rect)
        .expect("selection ✕")] {
        assert!(
            clear.size.x >= 16.0 && clear.size.y >= 16.0,
            "the ✕ must stay hittable, got {}×{}",
            clear.size.x,
            clear.size.y
        );
    }
}

#[test]
fn a_narrow_row_shrinks_both_chips_instead_of_overrunning() {
    let narrow = Rect::xywh(0.0, 0.0, 160.0, 60.0);

    let row = chip_row_layout(Some(200.0), Some(120.0), narrow);

    let style = row.style.expect("both chips survive at 160 px");
    let selection = row.selection.expect("both chips survive at 160 px");
    assert_eq!(style.origin.y, selection.origin.y);
    assert!(
        selection.origin.x + selection.size.x <= narrow.size.x + 0.01,
        "shrunk chips must still fit: {} > {}",
        selection.origin.x + selection.size.x,
        narrow.size.x
    );
    assert!(style.size.x >= 44.0 && selection.size.x >= 44.0);
}

#[test]
fn a_row_too_narrow_for_two_keeps_the_one_carrying_an_action() {
    let cramped = Rect::xywh(0.0, 0.0, 80.0, 60.0);

    let row = chip_row_layout(Some(200.0), Some(120.0), cramped);

    assert!(
        row.style.is_none(),
        "the receipt is a readout — it yields first"
    );
    let selection = row.selection.expect("the actionable chip survives");
    assert_eq!(selection.origin.x, 0.0);
    assert!(selection.size.x <= cramped.size.x);
}
