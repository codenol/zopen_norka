//! Pinned-style hover-card tests.
//!
//! Two things can go wrong here in ways that look like nothing at all: a card
//! that appears while the cursor is only passing through (which makes the whole
//! input area twitch), and a card that paints over the ✕ it hangs above (which
//! silently takes away the only control on that row). Most of what is asserted
//! below is one of those two.

use super::*;
use crate::widgets::asset_center_style_cards::style_test_support::exclusive_user_styles as exclusive_registry_for_tests;
use crate::widgets::test_capture_backend::CaptureBackend;
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

## Overview

A dark reference system built around one violet accent.

## Tokens — Colors

| Name | Value | Token | Role |
| --- | --- | --- | --- |
| Void Canvas | `#0a0a0a` | `--color-void-canvas` | Primary page background |
| Graphite | `#161616` | `--color-graphite` | Elevated surface |
| Bone | `#ededed` | `--color-bone` | Primary readable text |
";

/// A state with an imported guide pinned and the cursor resting on its chip
/// since `hover_since_ms` (or not resting on it at all).
fn state_hovering(hover_since_ms: Option<u64>) -> EditorState {
    let imported = op_ai_skills::style_guide::import_design_md(IMPORTED, "d.md").expect("imports");
    let mut state = EditorState::new();
    state.editor_ui.pinned_style_guide = Some(imported.id.clone());
    state.editor_ui.chat_style_chip_hover_since_ms = hover_since_ms;
    state
}

fn card_at(state: &EditorState, now_ms: u64) -> Option<StyleCard> {
    StyleCard::for_state(state, now_ms)
}

fn layout_at(state: &EditorState, now_ms: u64) -> Option<StyleCardLayout> {
    let panel = AIChatPlaceholder::from_editor_at(state, now_ms);
    let card = panel.style_card.clone()?;
    let chip = panel.chip_row(panel.input_rect(PANEL)).style?;
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    Some(layout_style_card(
        &mut cx,
        &card,
        chip,
        PANEL,
        state.editor_ui.locale,
    ))
}

// ─── Dwell ─────────────────────────────────────────────────────────────

/// Crossing the chip on the way to the input must show nothing. This is the
/// whole reason the card waits rather than tracking the hover directly.
#[test]
fn nothing_appears_before_the_dwell_elapses() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(1_000));

    assert_eq!(card_at(&state, 1_000), None);
    assert_eq!(card_at(&state, 1_000 + STYLE_CARD_DWELL_MS - 1), None);
    assert!(card_at(&state, 1_000 + STYLE_CARD_DWELL_MS).is_some());
}

/// The dwell expiring is not an input event, so its instant has to reach the
/// hosts' scheduler — otherwise the card only ever appears on the next jiggle.
#[test]
fn a_pending_card_reports_its_due_instant_and_then_stops() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(1_000));

    assert_eq!(
        next_deadline_ms(&state.editor_ui, 1_000),
        Some(1_000 + STYLE_CARD_DWELL_MS)
    );
    assert_eq!(
        next_deadline_ms(&state.editor_ui, 1_000 + STYLE_CARD_DWELL_MS),
        None,
        "a showing card needs no further wake-up"
    );
    assert_eq!(next_deadline_ms(&EditorState::new().editor_ui, 1_000), None);
}

#[test]
fn moving_off_the_chip_retires_the_card_immediately() {
    let _guard = exclusive_registry_for_tests();
    let mut state = state_hovering(Some(1_000));
    let showing = 1_000 + STYLE_CARD_DWELL_MS;
    assert!(card_at(&state, showing).is_some());

    assert!(state.editor_ui.set_chat_style_chip_hover(false, showing));
    assert_eq!(card_at(&state, showing), None);
    assert_eq!(card_at(&state, showing + 10_000), None);
}

/// With nothing pinned there is no chip, so a hover flag left over from some
/// other path must still resolve to no card rather than an empty one.
#[test]
fn no_pinned_style_means_no_card_however_long_the_cursor_rests() {
    let _guard = exclusive_registry_for_tests();
    let mut state = EditorState::new();
    state.editor_ui.chat_style_chip_hover_since_ms = Some(1_000);

    assert_eq!(card_at(&state, 1_000 + STYLE_CARD_DWELL_MS), None);
    assert!(
        AIChatPlaceholder::from_editor_at(&state, 1_000 + STYLE_CARD_DWELL_MS)
            .style_card
            .is_none()
    );
}

/// A pin whose guide was deleted names nothing — the chip shows no row, and
/// the card must not invent one either.
#[test]
fn a_stale_pin_shows_no_card() {
    let _guard = exclusive_registry_for_tests();
    let mut state = EditorState::new();
    state.editor_ui.pinned_style_guide = Some("user:deleted-last-week".into());
    state.editor_ui.chat_style_chip_hover_since_ms = Some(1_000);

    assert_eq!(card_at(&state, 1_000 + STYLE_CARD_DWELL_MS), None);
}

// ─── Provenance ────────────────────────────────────────────────────────

/// The fact the chip cannot carry: which catalogue the pin points into. Getting
/// this wrong is what makes a user think their import "didn't work".
#[test]
fn an_import_and_a_corpus_guide_are_labelled_differently() {
    let _guard = exclusive_registry_for_tests();
    let imported = card_at(&state_hovering(Some(0)), STYLE_CARD_DWELL_MS).expect("a card");
    assert_eq!(imported.source, StyleCardSource::Imported);
    assert_eq!(imported.name, "Dimension");

    let corpus_name = op_ai_skills::style_guide::style_guide_registry()[0]
        .name
        .clone();
    let mut state = EditorState::new();
    state.editor_ui.pinned_style_guide = Some(corpus_name.clone());
    state.editor_ui.chat_style_chip_hover_since_ms = Some(0);
    let builtin = card_at(&state, STYLE_CARD_DWELL_MS).expect("a card");
    assert_eq!(builtin.source, StyleCardSource::Builtin);
    assert_eq!(builtin.name, corpus_name);

    assert_ne!(
        op_i18n::translate(op_editor_core::Locale::EnUs, imported.source.label_key()),
        op_i18n::translate(op_editor_core::Locale::EnUs, builtin.source.label_key()),
        "the two sources must not read the same"
    );
}

/// design.md outranks a pin in the pipeline, so it outranks it here — and it
/// arrives already structured, so the card states its real palette rather than
/// the displaced pin's.
#[test]
fn document_design_md_outranks_the_pin_and_carries_its_own_values() {
    let _guard = exclusive_registry_for_tests();
    let mut state = state_hovering(Some(0));
    state.doc.design_md = Some(jian_ops_schema::DesignMdSpec {
        raw: String::new(),
        project_name: Some("Northwind".into()),
        visual_theme: Some("Warm minimal, generous whitespace.\nMore detail below.".into()),
        color_palette: Some(vec![jian_ops_schema::DesignMdColor {
            name: "Primary".into(),
            hex: "#2E5AAC".into(),
            role: "buttons".into(),
        }]),
        typography: Some(jian_ops_schema::DesignMdTypography {
            font_family: Some("Inter".into()),
            headings: None,
            body: Some("Inter".into()),
            scale: None,
        }),
        component_styles: None,
        layout_principles: None,
        generation_notes: None,
        rules: Vec::new(),
    });

    let card = card_at(&state, STYLE_CARD_DWELL_MS).expect("a card");
    assert_eq!(card.source, StyleCardSource::DocumentDesignMd);
    assert_eq!(card.name, "Northwind", "the brief names itself when it can");
    assert_eq!(card.swatches.len(), 1);
    assert_eq!(card.swatches[0].hex, "#2E5AAC");
    // Identical families collapse — "Inter / Inter" says less than "Inter".
    assert_eq!(card.fonts.as_deref(), Some("Inter"));
    assert_eq!(
        card.description.as_deref(),
        Some("Warm minimal, generous whitespace."),
        "only the theme's first line belongs on a two-line caption"
    );
}

// ─── Placement ─────────────────────────────────────────────────────────

/// Above the chip, always: the ✕ is the only control on that row, and a card
/// covering it would remove the ability to unpin without any visible cause.
#[test]
fn the_card_sits_above_the_chip_and_inside_the_panel() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(0));
    let layout = layout_at(&state, STYLE_CARD_DWELL_MS).expect("a placed card");

    let panel = AIChatPlaceholder::from_editor_at(&state, STYLE_CARD_DWELL_MS);
    let chip = panel
        .chip_row(panel.input_rect(PANEL))
        .style
        .expect("a chip");
    assert!(
        layout.rect.origin.y + layout.rect.size.y <= chip.origin.y,
        "card bottom {} overlapped the chip at {}",
        layout.rect.origin.y + layout.rect.size.y,
        chip.origin.y
    );
    assert!(layout.rect.origin.y >= PANEL.origin.y);
    assert!(layout.rect.origin.x >= PANEL.origin.x);
    assert!(layout.rect.origin.x + layout.rect.size.x <= PANEL.origin.x + PANEL.size.x);
    assert!(layout.rect.size.y > 0.0);
}

/// A narrow panel shrinks the card rather than letting it run off the edge.
#[test]
fn a_narrow_panel_clamps_the_card_to_its_own_width() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(0));
    let narrow = Rect::xywh(0.0, 0.0, 300.0, 400.0);

    let panel = AIChatPlaceholder::from_editor_at(&state, STYLE_CARD_DWELL_MS);
    let card = panel.style_card.clone().expect("a card");
    let chip = panel
        .chip_row(panel.input_rect(narrow))
        .style
        .expect("a chip");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let layout = layout_style_card(&mut cx, &card, chip, narrow, state.editor_ui.locale);

    assert!(
        layout.rect.origin.x + layout.rect.size.x <= narrow.origin.x + narrow.size.x,
        "card ran past the panel: {:?}",
        layout.rect
    );
    assert!(layout.rect.origin.y >= narrow.origin.y);
    assert!(layout.rect.origin.y + layout.rect.size.y <= chip.origin.y);
}

/// A panel too short for everything drops rows from the bottom up. Provenance
/// survives every drop — it is the one thing the card exists to state.
#[test]
fn a_short_panel_drops_rows_rather_than_growing_over_the_chip() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(0));
    let short = Rect::xywh(
        0.0,
        0.0,
        320.0,
        crate::widgets::ai_chat_panel::AI_CHAT_MIN_HEIGHT,
    );

    let panel = AIChatPlaceholder::from_editor_at(&state, STYLE_CARD_DWELL_MS);
    let card = panel.style_card.clone().expect("a card");
    let chip = panel
        .chip_row(panel.input_rect(short))
        .style
        .expect("a chip");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let layout = layout_style_card(&mut cx, &card, chip, short, state.editor_ui.locale);

    assert!(layout.rect.origin.y >= short.origin.y);
    assert!(layout.rect.origin.y + layout.rect.size.y <= chip.origin.y);
    assert!(!layout.title.is_empty());
    assert!(!layout.badge_label.is_empty());
}

// ─── Hover resolution ──────────────────────────────────────────────────

#[test]
fn the_hover_covers_the_whole_chip_and_nothing_else() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(None);
    let panel = AIChatPlaceholder::from_editor_at(&state, 0);
    let chip = panel
        .chip_row(panel.input_rect(PANEL))
        .style
        .expect("a chip");

    let centre = Point2D::new(
        chip.origin.x + chip.size.x / 2.0,
        chip.origin.y + chip.size.y / 2.0,
    );
    assert!(panel.style_chip_hover_at(PANEL, centre));
    // The whole row is hoverable, edge to edge: the row reports the session's
    // rules and carries no clear button, so there is no dead zone in it.
    let left = Point2D::new(chip.origin.x + 2.0, chip.origin.y + chip.size.y / 2.0);
    let right = Point2D::new(
        chip.origin.x + chip.size.x - 2.0,
        chip.origin.y + chip.size.y / 2.0,
    );
    assert!(panel.style_chip_hover_at(PANEL, left));
    assert!(panel.style_chip_hover_at(PANEL, right));

    // Just below the row is the text area, not the chip.
    assert!(!panel.style_chip_hover_at(
        PANEL,
        Point2D::new(centre.x, chip.origin.y + chip.size.y + 8.0)
    ));
    assert!(!panel.style_chip_hover_at(PANEL, Point2D::new(centre.x, PANEL.origin.y + 4.0)));
}

/// The rules chip is present with nothing pinned — the session's rules are in
/// force on their own — and hovering it opens the card like any other chip.
#[test]
fn the_rules_chip_is_hoverable_without_a_pin() {
    let _guard = exclusive_registry_for_tests();
    let state = EditorState::new();
    let panel = AIChatPlaceholder::from_editor_at(&state, 0);
    let chip = panel
        .style_chip_rect(PANEL)
        .expect("the rules chip is always present");
    assert!(panel.style_chip_hover_at(
        PANEL,
        Point2D::new(
            chip.origin.x + chip.size.x / 2.0,
            chip.origin.y + chip.size.y / 2.0
        )
    ));
}

// ─── Paint ─────────────────────────────────────────────────────────────

/// The card is painted last so nothing in the input block covers it, and its
/// strings are the ones the layout resolved.
#[test]
fn the_panel_paints_the_card_over_its_input_block() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(Some(0));
    let panel = AIChatPlaceholder::from_editor_at(&state, STYLE_CARD_DWELL_MS);
    let layout = layout_at(&state, STYLE_CARD_DWELL_MS).expect("a placed card");

    let mut backend = CaptureBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        crate::widgets::Widget::paint(&panel, &mut cx, PANEL);
    }
    assert!(
        backend
            .round_fills
            .iter()
            .any(|(rect, _, _)| *rect == layout.rect),
        "the card's surface was never painted"
    );
    assert!(
        backend.texts.iter().any(|(text, _)| text == &layout.title),
        "the style name is missing from the card"
    );
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, _)| text == &layout.badge_label),
        "the source badge is missing from the card"
    );
    // Every hex on the band is stated, which is what makes the card more than
    // a wider copy of the chip.
    for (_, _, hex) in &layout.swatches {
        assert!(
            backend.texts.iter().any(|(text, _)| text == hex),
            "{hex} was painted with no label"
        );
    }
}

#[test]
fn an_un_hovered_panel_paints_no_card() {
    let _guard = exclusive_registry_for_tests();
    let state = state_hovering(None);
    let panel = AIChatPlaceholder::from_editor_at(&state, 10_000);
    assert!(panel.style_card.is_none());

    let mut backend = CaptureBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        crate::widgets::Widget::paint(&panel, &mut cx, PANEL);
    }
    let badge = op_i18n::translate(
        state.editor_ui.locale,
        StyleCardSource::Imported.label_key(),
    );
    assert!(
        !backend.texts.iter().any(|(text, _)| text == badge),
        "a card painted with no hover at all"
    );
}
