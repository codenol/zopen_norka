//! "Style: Dimension ✕" — what the next generation will actually use.
//!
//! The Asset Center already shows a pin: the card is outlined, the chip says
//! 已钉住. But the place a person presses Generate is here, and this is where
//! the pin was invisible. Two separate failures shipped looking identical from
//! the outside — a pin that never reached the plan, and a pin that reached it
//! carrying no readable values — and in both the user saw only that the colours
//! were wrong. Neither was diagnosable from the editor.
//!
//! So the row answers two questions at a glance, and the colour band is the
//! second one. The name says *which* style is in force; the band says whether
//! anything could be read out of it. An empty band on a named style is the
//! visible form of "this file parsed, but its values did not" — the condition
//! that previously required running a probe against the file to detect.
//!
//! It says nothing when there is nothing true to say: a pin naming a guide
//! that no longer exists shows no row, because generation has already fallen
//! back to choosing its own style and naming the dead guide would replace one
//! silent failure with a loud false one.

use op_ai_skills::style_guide::StyleGuideSummary;
use op_editor_core::EditorState;
use op_util::hex_color;

use crate::theme::Theme;
use crate::widgets::ai_chat_chip_row::{
    chip_clear_rect, draw_chip_clear, draw_chip_text, fill_chip, CHIP_CLEAR_W, CHIP_FONT,
    CHIP_PAD_X,
};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

const SWATCH_W: f32 = 9.0;
const SWATCH_H: f32 = 10.0;
const SWATCH_GAP: f32 = 2.0;
/// Trailing inset on a receipt that carries no ✕ — the band must not run
/// into the chip's right edge just because nothing is clearable.
const NO_CLEAR_INSET: f32 = 6.0;

/// What the row shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StyleReceipt {
    /// The name a person reads.
    pub name: String,
    /// The band, already parsed. Empty means the guide stated no values this
    /// build could read — which the row shows by drawing no band.
    pub swatches: Vec<Color>,
    /// Whether pressing ✕ can clear what this row describes.
    ///
    /// False for the rules row: the session's design rules are always in
    /// force — the AI reads them on every turn — so there is nothing to
    /// clear. Only a catalog style guide the user pinned is clearable.
    pub clearable: bool,
    /// Whether this row reports the session's design rules rather than a
    /// pinned catalog guide. It changes only the label.
    pub is_rules: bool,
}

impl StyleReceipt {
    /// The receipt for the current editor state, or `None` when the row
    /// should not paint at all.
    ///
    /// The session's design rules outrank a pinned guide: the pipeline hands
    /// the AI those rules on every turn, and the user cannot switch them off
    /// — the row exists so the chat says what is actually in force.
    pub(crate) fn for_state(state: &EditorState) -> Option<Self> {
        let rules = op_editor_core::effective_design_rules(state.doc.design_md.as_ref());
        if !rules.is_empty() {
            return Some(Self {
                // The count is the label's payload — see the chip's label.
                name: rules.len().to_string(),
                swatches: Vec::new(),
                clearable: false,
                is_rules: true,
            });
        }
        let pinned = state.editor_ui.pinned_style_guide.as_deref()?;
        let summary = op_ai_skills::style_guide::style_guide_summary(pinned)?;
        Some(Self::from_summary(&summary))
    }

    fn from_summary(summary: &StyleGuideSummary) -> Self {
        Self {
            name: summary.name.clone(),
            swatches: summary
                .swatches
                .iter()
                .filter_map(|hex| parse(hex))
                .collect(),
            clearable: true,
            is_rules: false,
        }
    }
}

fn parse(raw: &str) -> Option<Color> {
    let [r, g, b, a] = hex_color::parse_hex_rgba8(raw, hex_color::HexOptions::LENIENT)?;
    Some(Color::rgba_u8(r, g, b, a as f32 / 255.0))
}

/// Width of the colour band for `swatches` swatches, gutter included.
fn band_width(swatches: usize) -> f32 {
    if swatches == 0 {
        0.0
    } else {
        swatches as f32 * SWATCH_W + (swatches.saturating_sub(1)) as f32 * SWATCH_GAP + 8.0
    }
}

/// Trailing column: the ✕ target, or a plain inset when nothing can be
/// cleared here.
fn trailing_width(clearable: bool) -> f32 {
    if clearable {
        CHIP_CLEAR_W
    } else {
        NO_CLEAR_INSET
    }
}

/// Width the chip needs for `label` plus its band and clear button.
pub(crate) fn chip_width(label: &str, swatches: usize, clearable: bool) -> f32 {
    CHIP_PAD_X
        + crate::widgets::ai_chat_panel::footer_label_width(label, CHIP_FONT)
        + band_width(swatches)
        + trailing_width(clearable)
}

/// The ✕ target inside `chip`, or `None` when this receipt has nothing to
/// clear (design.md is bound and unbound from its own panel).
pub(crate) fn clear_rect(receipt: &StyleReceipt, chip: Rect) -> Option<Rect> {
    receipt.clearable.then(|| chip_clear_rect(chip))
}

/// Paint the receipt into the chip the row allotted it.
pub(crate) fn paint_style_chip(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    receipt: &StyleReceipt,
    label: &str,
    chip: Rect,
) {
    fill_chip(cx, theme, chip);

    // The label gets whatever the band does not need, but the band never
    // takes more than half the free width: a squeezed row must not turn the
    // style's NAME — the thing the row exists to state — into an ellipsis.
    let trailing = trailing_width(receipt.clearable);
    let free = (chip.size.x - CHIP_PAD_X - trailing).max(0.0);
    let band = band_width(receipt.swatches.len()).min(free * 0.5);
    let fitted = text_metrics::fit_chrome(cx.backend, label, free - band, CHIP_FONT);
    let label_x = chip.origin.x + CHIP_PAD_X;
    draw_chip_text(cx, theme, &fitted, label_x, chip);

    let label_end = label_x + text_metrics::measure_chrome(cx.backend, &fitted, CHIP_FONT);
    paint_band(cx, receipt, chip, label_end, trailing);

    if receipt.clearable {
        draw_chip_clear(cx, theme, chip);
    }
}

/// The colour band, clipped to whatever room is left between the name and the
/// clear button. Swatches that would overrun are dropped rather than squeezed:
/// a band is read as "these are the colours", and a half-drawn one reads as a
/// different palette.
fn paint_band(
    cx: &mut PaintCx<'_>,
    receipt: &StyleReceipt,
    chip: Rect,
    label_end: f32,
    trailing: f32,
) {
    if receipt.swatches.is_empty() {
        return;
    }
    let available = chip.origin.x + chip.size.x - trailing - (label_end + 8.0);
    let top = chip.origin.y + (chip.size.y - SWATCH_H) / 2.0;
    for (index, color) in receipt.swatches.iter().enumerate() {
        let x = label_end + 8.0 + index as f32 * (SWATCH_W + SWATCH_GAP);
        if x + SWATCH_W > label_end + 8.0 + available {
            break;
        }
        cx.backend.fill_round_rect(
            Rect {
                origin: Point2D::new(x, top),
                size: Point2D::new(SWATCH_W, SWATCH_H),
            },
            2.0,
            *color,
        );
    }
}

#[cfg(test)]
#[path = "ai_chat_style_receipt_tests.rs"]
mod ai_chat_style_receipt_tests;
