//! Pinned-style receipt tests.
//!
//! The row exists because two different failures were indistinguishable from
//! the editor. Most of what is asserted here is therefore about *not* showing
//! something: a row that appears when the pin is dead, or that keeps its ✕ on
//! a pin it cannot clear, would be a new way to mislead rather than a fix.

use super::*;
use crate::widgets::ai_chat_hit::AIChatHit;
use crate::widgets::asset_center_style_cards::style_test_support::exclusive_user_styles as exclusive_registry_for_tests;
use crate::widgets::AIChatPlaceholder;
use op_editor_core::EditorState;

const PANEL: Rect = Rect {
    origin: Point2D { x: 40.0, y: 60.0 },
    size: Point2D { x: 380.0, y: 520.0 },
};

const IMPORTED: &str = "\
---
name: Dimension
---

## Tokens — Colors

| Name | Value | Token | Role |
| --- | --- | --- | --- |
| Void Canvas | `#0a0a0a` | `--color-void-canvas` | Primary page background, base surface |
| Graphite | `#161616` | `--color-graphite` | Elevated surface for floating panels |
| Dusk Violet | `linear-gradient(90deg, rgba(0,0,0,0), rgba(107,98,242,0.565) 50%)` | `--color-dusk-violet` | The only chromatic accent |
| Bone | `#ededed` | `--color-bone` | Primary readable text on dark surfaces |
";

fn state_with_pin(pin: Option<&str>) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.pinned_style_guide = pin.map(str::to_string);
    state
}

/// The session's rules are in force whether or not the user pinned a guide,
/// so the row is always there — that is the point of it.
#[test]
fn the_rules_row_is_present_without_a_pin() {
    let _guard = exclusive_registry_for_tests();
    let state = state_with_pin(None);

    let receipt = StyleReceipt::for_state(&state).expect("rules are always in force");
    assert!(receipt.is_rules);
    assert!(!receipt.clearable);

    let panel = AIChatPlaceholder::from_editor_at(&state, 0);
    assert!(panel.chip_row_h() > 0.0, "the chip takes room in the block");
}

/// A pinned catalog guide no longer owns the row: the session's rules are
/// what the model actually reads, so the row keeps reporting them.
#[test]
fn a_pin_does_not_displace_the_rules_row() {
    let _guard = exclusive_registry_for_tests();
    let imported = op_ai_skills::style_guide::import_design_md(IMPORTED, "d.md").expect("imports");
    let state = state_with_pin(Some(&imported.id));

    let receipt = StyleReceipt::for_state(&state).expect("a row");
    assert!(receipt.is_rules);
    assert_ne!(receipt.name, "Dimension");

    let panel = AIChatPlaceholder::from_editor_at(&state, 0);
    assert!(panel.chip_row_h() > 0.0);
    let label = panel.style_receipt_label().expect("a row has a label");
    assert!(
        !label.contains("{{count}}"),
        "the count placeholder must be substituted: {label}"
    );
    assert!(label.contains(&receipt.name), "{label}");
}

/// The session's design rules outrank a pin in the pipeline, so they outrank
/// it here. Showing the pinned name would claim a style that is not in force;
/// showing nothing would leave the user unable to learn why their pin stopped
/// mattering. The rules row is never clearable — the AI always reads them.
#[test]
fn design_rules_are_reported_instead_of_the_pin_and_carry_no_clear() {
    let _guard = exclusive_registry_for_tests();
    let imported = op_ai_skills::style_guide::import_design_md(IMPORTED, "d.md").expect("imports");
    let mut state = state_with_pin(Some(&imported.id));
    state.doc.design_md = Some(jian_ops_schema::DesignMdSpec {
        raw: String::new(),
        project_name: None,
        visual_theme: None,
        color_palette: None,
        typography: None,
        component_styles: None,
        layout_principles: None,
        generation_notes: None,
        rules: Vec::new(),
    });

    let receipt = StyleReceipt::for_state(&state).expect("a row");
    assert!(
        receipt.is_rules,
        "the row reports rules, not a catalog guide"
    );
    assert!(
        receipt.name.parse::<usize>().is_ok(),
        "the row carries the rule count, got {:?}",
        receipt.name
    );
    assert_ne!(receipt.name, "Dimension");
    assert!(!receipt.clearable, "the rules cannot be switched off");
    assert!(clear_rect(&receipt, Rect::xywh(0.0, 0.0, 120.0, 22.0)).is_none());
}

// ─── Layout + hit-test ─────────────────────────────────────────────────

#[test]
fn the_row_reserves_space_above_the_input_text() {
    let _guard = exclusive_registry_for_tests();
    let bare_state = state_with_pin(None);
    let bare = AIChatPlaceholder::from_editor_at(&bare_state, 0);

    let imported = op_ai_skills::style_guide::import_design_md(IMPORTED, "d.md").expect("imports");
    let pinned_state = state_with_pin(Some(&imported.id));
    let pinned = AIChatPlaceholder::from_editor_at(&pinned_state, 0);

    // Both states carry the rules row, so the block is the same height and
    // the text area starts below the chip either way.
    assert_eq!(
        pinned.input_height_for_rect(PANEL),
        bare.input_height_for_rect(PANEL),
        "the rules row is present with or without a pin"
    );
    let chip = pinned
        .chip_row(pinned.input_rect(PANEL))
        .style
        .expect("a live pin shows its chip");
    assert!(
        chip.origin.y + chip.size.y <= pinned.input_text_rect(PANEL).origin.y,
        "the chip must sit above the text area, not over it"
    );
}

#[test]
fn the_rules_row_offers_no_clear_target_and_is_not_a_button() {
    let _guard = exclusive_registry_for_tests();
    let imported = op_ai_skills::style_guide::import_design_md(IMPORTED, "d.md").expect("imports");
    let state = state_with_pin(Some(&imported.id));
    let panel = AIChatPlaceholder::from_editor_at(&state, 0);
    let input_rect = panel.input_rect(PANEL);
    assert!(
        panel.style_receipt_clear_rect(input_rect).is_none(),
        "the rules cannot be cleared"
    );

    // The row is not a button anywhere: pressing it focuses the input, the
    // same as pressing anywhere else in the block.
    let chip = panel
        .style_chip_rect(PANEL)
        .expect("the rules row shows a chip");
    let on_chip = Point2D::new(
        chip.origin.x + chip.size.x / 2.0,
        chip.origin.y + chip.size.y / 2.0,
    );
    assert_eq!(panel.hit_test(PANEL, on_chip), Some(AIChatHit::FocusInput));
}

/// The chip must not overrun its block however long the style is named — a
/// name is user-supplied and can be anything.
#[test]
fn a_very_long_name_is_clamped_to_the_input_block() {
    let receipt = StyleReceipt {
        name: "N".repeat(400),
        swatches: vec![Color::rgba_u8(1, 2, 3, 1.0); 4],
        clearable: true,
        is_rules: false,
    };
    let input_rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(200.0, 100.0),
    };
    let natural = chip_width(&receipt.name, receipt.swatches.len(), receipt.clearable);
    let chip = crate::widgets::ai_chat_chip_row::chip_row_layout(Some(natural), None, input_rect)
        .style
        .expect("a chip");
    assert!(chip.size.x <= input_rect.size.x);
    let clear = clear_rect(&receipt, chip).expect("clearable");
    assert!(clear.origin.x + clear.size.x <= input_rect.origin.x + input_rect.size.x);
}
