//! Paint primitives shared by the three comment surfaces.
//!
//! The pin in the canvas, the thread popover and the list panel are one visual
//! language: the same author dot, the same button, the same panel chrome. They
//! live here so a change to "what a resolve button looks like" is one edit, and
//! so the three widgets stay under the repository's file ceiling while agreeing
//! with each other.

use crate::theme::Theme;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Color, Point2D, Rect, TextLayout};

/// Draw one run of text with an explicit baseline origin.
///
/// `draw_text` is baseline-relative (see the workspace notes on
/// `centered_text_baseline_y`), so callers pass the baseline, never a top edge.
pub(crate) fn text(
    cx: &mut PaintCx<'_>,
    value: &str,
    size: f32,
    color: Color,
    origin: Point2D,
    weight: u16,
) {
    let layout = TextLayout::single_run(value, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    cx.backend.draw_text(&layout, origin);
}

/// The colour a pin's number is painted in, chosen against its own fill.
///
/// The operator's role colours span a yellow that white text disappears into and
/// a violet that black text disappears into, so a fixed foreground would make
/// one of the seven illegible. Relative luminance is the smallest rule that
/// answers "which of the two is readable on THIS fill".
pub(crate) fn label_on(fill: Color) -> Color {
    let luminance = 0.299 * fill.r + 0.587 * fill.g + 0.114 * fill.b;
    if luminance > 0.6 {
        Color::rgb_u8(0x1A, 0x1A, 0x1A)
    } else {
        Color::rgb_u8(0xFF, 0xFF, 0xFF)
    }
}

/// A filled circle with the author's role colour — the marker in a list row.
///
/// Falls back to the muted tone when the role is unknown: a dot that is a
/// colour at all is what makes a row scannable, and refusing to paint one would
/// make those rows read as broken rather than as unroled.
pub(crate) fn role_dot(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    center: Point2D,
    radius: f32,
    color: Option<Color>,
) {
    let color = color.unwrap_or(theme.muted_foreground);
    let bounds = Rect::xywh(
        center.x - radius,
        center.y - radius,
        radius * 2.0,
        radius * 2.0,
    );
    cx.backend.fill_oval(bounds, color);
}

/// The rounded panel every comment surface paints on.
pub(crate) fn panel(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect) {
    cx.backend.fill_round_rect(rect, 10.0, theme.popover);
    cx.backend
        .stroke_round_rect(rect, 10.0, theme.border.with_alpha(0.9), 1.0);
}

/// A button on a comment surface.
///
/// `enabled` dims rather than hides, because a send button that vanishes when
/// the field is empty leaves the reviewer wondering where it went; a disabled
/// one says "type something first".
pub(crate) fn button(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    label: &str,
    primary: bool,
    enabled: bool,
    hovered: bool,
) {
    let background = if primary {
        theme.primary
    } else {
        theme.secondary
    };
    cx.backend.fill_round_rect(
        rect,
        6.0,
        background.with_alpha(if enabled { 1.0 } else { 0.45 }),
    );
    if hovered && enabled {
        cx.backend.fill_round_rect(rect, 6.0, theme.button_hover);
        cx.backend
            .stroke_round_rect(rect, 6.0, theme.foreground.with_alpha(0.12), 1.0);
    }
    let color = if primary {
        theme.primary_foreground
    } else {
        theme.secondary_foreground
    };
    let width = text_metrics::measure_chrome_weighted(cx.backend, label, 11.0, 500);
    text(
        cx,
        label,
        11.0,
        color.with_alpha(if enabled { 1.0 } else { 0.55 }),
        Point2D::new(
            rect.origin.x + (rect.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(rect, 11.0),
        ),
        500,
    );
}

/// A one-line input's box: a field the reviewer types into.
pub(crate) fn input(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    placeholder: &str,
    draft: &str,
    focused: bool,
) {
    cx.backend.fill_round_rect(rect, 6.0, theme.input);
    cx.backend.stroke_round_rect(
        rect,
        6.0,
        if focused { theme.ring } else { theme.border },
        1.0,
    );
    let baseline = jian_widgets::centered_text_baseline_y(rect, 12.0);
    let origin = Point2D::new(rect.origin.x + 8.0, baseline);
    if draft.is_empty() {
        let fitted =
            text_metrics::fit_chrome(cx.backend, placeholder, (rect.size.x - 16.0).max(0.0), 12.0);
        text(cx, &fitted, 12.0, theme.muted_foreground, origin, 400);
    } else {
        let fitted =
            text_metrics::fit_chrome(cx.backend, draft, (rect.size.x - 16.0).max(0.0), 12.0);
        text(cx, &fitted, 12.0, theme.foreground, origin, 400);
    }
}

/// A row's caption: muted, single line, ellipsized to `width`.
pub(crate) fn caption(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    value: &str,
    width: f32,
    origin: Point2D,
    size: f32,
) {
    let fitted = text_metrics::fit_chrome(cx.backend, value, width.max(0.0), size);
    text(cx, &fitted, size, theme.muted_foreground, origin, 400);
}

/// An outlined chip, used for a thread's state (`Open` / `Resolved`).
pub(crate) fn chip(cx: &mut PaintCx<'_>, rect: Rect, label: &str, color: Color) {
    cx.backend
        .fill_round_rect(rect, 4.0, color.with_alpha(0.16));
    let width = text_metrics::measure_chrome_weighted(cx.backend, label, 10.0, 500);
    text(
        cx,
        label,
        10.0,
        color,
        Point2D::new(
            rect.origin.x + (rect.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(rect, 10.0),
        ),
        500,
    );
}

/// The width a chip needs for `label`, so layout and paint agree.
pub(crate) fn chip_width(cx: &mut PaintCx<'_>, label: &str) -> f32 {
    text_metrics::measure_chrome_weighted(cx.backend, label, 10.0, 500) + 12.0
}

/// Diameter of the count badge.
///
/// Small enough to sit on a 32 px toolbar button without covering its glyph,
/// large enough for a two-character label at [`BADGE_FONT`] — that pair is the
/// whole constraint, and both surfaces that show a count read them from here.
pub const BADGE_DIAMETER: f32 = 15.0;
/// Font size inside the badge.
const BADGE_FONT: f32 = 10.0;
/// How far the badge's ring reaches outside the circle it rings.
///
/// Part of the badge's geometry rather than a local constant of the painter: a
/// row that places a badge beside a name has to leave room for the ring, and a
/// gutter measured against the circle alone would be 1.5 px short of the truth.
pub(crate) const BADGE_RING: f32 = 1.5;

/// How many open threads a badge says before it stops counting.
///
/// `9+` rather than `12`: the badge is a nudge, not a ledger, and a third
/// character would need a wider circle than a corner badge should be. The rail
/// prints the exact number a few centimetres away.
pub fn badge_label(count: usize) -> String {
    if count > 9 {
        "9+".to_string()
    } else {
        count.to_string()
    }
}

/// The area a badge actually covers: its circle plus the ring under it.
pub(crate) fn badge_ink(slot: Rect) -> Rect {
    inflate(slot, BADGE_RING)
}

/// A count badge drawn inside `slot`, filling it.
///
/// The single painter behind every count the chrome shows. The two callers pass
/// different slots and nothing else: the toolbar icon's corner badge
/// ([`count_badge`]) and a page row's marker. Keeping one painter is what makes
/// "the same number looks the same wherever it appears" a property of the code
/// rather than of two painters that happen to agree today — the fill order, the
/// ring in the surface colour under the circle, and the ring's colour all come
/// from here.
///
/// Nothing is painted at `0`: a "0" badge claims there is something to look at.
pub(crate) fn count_badge_at(cx: &mut PaintCx<'_>, theme: &Theme, slot: Rect, count: usize) {
    if count == 0 {
        return;
    }
    let label = badge_label(count);
    cx.backend
        .fill_oval(badge_ink(slot), theme.popover.with_alpha(0.95));
    cx.backend.fill_oval(slot, theme.primary);
    let width = text_metrics::measure_chrome_weighted(cx.backend, &label, BADGE_FONT, 700);
    text(
        cx,
        &label,
        BADGE_FONT,
        theme.primary_foreground,
        Point2D::new(
            slot.origin.x + (slot.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(slot, BADGE_FONT),
        ),
        700,
    );
}

/// A count badge pinned to the top-right corner of `anchor`.
///
/// The ring in the surface colour under it is what makes a 15 px badge legible
/// over an 18 px icon drawn in the same corner: without it the glyph and the
/// badge's edge touch, and the number reads as part of the icon. A surface that
/// has room of its own for the badge (a page row, whose tail is reserved for it)
/// passes that slot to [`count_badge_at`] instead of borrowing this corner.
pub(crate) fn count_badge(cx: &mut PaintCx<'_>, theme: &Theme, anchor: Rect, count: usize) {
    count_badge_at(cx, theme, corner_badge_slot(anchor), count);
}

/// Where a corner badge sits over `anchor` — the toolbar's icon rect.
fn corner_badge_slot(anchor: Rect) -> Rect {
    Rect::xywh(
        anchor.origin.x + anchor.size.x - BADGE_DIAMETER * 0.62,
        anchor.origin.y - BADGE_DIAMETER * 0.28,
        BADGE_DIAMETER,
        BADGE_DIAMETER,
    )
}

fn inflate(rect: Rect, by: f32) -> Rect {
    Rect::xywh(
        rect.origin.x - by,
        rect.origin.y - by,
        rect.size.x + by * 2.0,
        rect.size.y + by * 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_fill_gets_dark_text_and_a_dark_fill_white() {
        // The analyst's yellow is the case this exists for: white on #FDE047 is
        // invisible on a dark chrome.
        let yellow = crate::util::parse_hex_color("#FDE047").unwrap();
        let violet = crate::util::parse_hex_color("#8B5CF6").unwrap();
        assert_eq!(label_on(yellow), Color::rgb_u8(0x1A, 0x1A, 0x1A));
        assert_eq!(label_on(violet), Color::rgb_u8(0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn the_two_label_colours_are_the_only_ones_painted() {
        // Every role colour resolves to one of the two, whichever it is.
        for role in op_editor_core::ProductRole::ALL {
            let fill = crate::util::parse_hex_color(role.colour().hex).unwrap();
            let label = label_on(fill);
            assert!(
                label == Color::rgb_u8(0x1A, 0x1A, 0x1A)
                    || label == Color::rgb_u8(0xFF, 0xFF, 0xFF),
                "role {role:?} produced an unexpected label colour"
            );
        }
    }
}
