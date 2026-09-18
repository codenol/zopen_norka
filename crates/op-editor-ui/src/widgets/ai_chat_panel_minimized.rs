//! The AI chat panel's minimized form: a compact input bar.
//!
//! Entering the app or opening a document starts the panel here rather
//! than as a full chat panel, so the canvas is the thing the user sees
//! first. The bar reads as an input — sparkle glyph, the same
//! placeholder the real textarea uses (or the unsent draft), the active
//! model's name and a submit circle — but it has no interactive
//! sub-controls: the whole bar is one click target that expands the
//! panel back to its normal form.

use super::ai_chat_panel::{chat_neutral_feedback_color, footer_label_width, AIChatPlaceholder};
use crate::theme::Theme;
use crate::widgets::ai_chat_panel_footer::fit_footer_label;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

/// Bar height at rest. Sized so the bar reads as a thin input strip,
/// not a slab: the submit circle plus its breathing room sets the floor.
///
/// There is deliberately no matching `_WIDTH`: minimizing changes the
/// panel's height only, and the width comes from the host's expanded-panel
/// width so the two states' edges line up. A second width constant here is
/// exactly what made the bar jump narrower than the panel it replaced.
pub const AI_CHAT_MINIMIZED_HEIGHT: f32 = 48.0;
/// Narrowest the bar may be squeezed to before a host hides it.
pub const AI_CHAT_MINIMIZED_MIN_WIDTH: f32 = 240.0;

const MINIMIZED_RADIUS: f32 = 12.0;
const MINIMIZED_PAD_X: f32 = 12.0;
const MINIMIZED_SPARKLE: f32 = 14.0;
const MINIMIZED_GAP: f32 = 8.0;
const MINIMIZED_SUBMIT_D: f32 = 26.0;
const MINIMIZED_SUBMIT_GLYPH: f32 = 12.0;
const MINIMIZED_TEXT_FONT: f32 = 12.0;
const MINIMIZED_MODEL_FONT: f32 = 10.5;
/// Ceiling on the model-name column so a long model name cannot squeeze
/// the placeholder out of the bar.
const MINIMIZED_MODEL_MAX_W: f32 = 96.0;
/// Share of the text+model span the model name may take. The absolute
/// ceiling above is not enough on a squeezed bar: 96 px of model name
/// inside a 240 px bar would leave the placeholder a stub.
const MINIMIZED_MODEL_MAX_SHARE: f32 = 0.45;
/// The model name is secondary chrome — one alpha step below the
/// placeholder so the eye lands on the prompt first.
const MINIMIZED_MODEL_ALPHA: f32 = 0.7;

/// Where each part of the bar sits inside `rect`.
///
/// Paint-only: the bar has no per-part hit-testing (any press expands
/// the panel), but the rects are resolved here so paint and tests read
/// the same geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MinimizedBarLayout {
    /// Sparkle glyph box at the far left.
    pub(crate) sparkle: Rect,
    /// Placeholder / draft text column.
    pub(crate) text: Rect,
    /// Right-aligned model-name column; zero-width when there is no room.
    pub(crate) model: Rect,
    /// Submit circle at the far right.
    pub(crate) submit: Rect,
}

pub(crate) fn minimized_bar_layout(rect: Rect, model_name: &str) -> MinimizedBarLayout {
    let center_y = rect.origin.y + rect.size.y / 2.0;
    let sparkle = Rect {
        origin: Point2D::new(
            rect.origin.x + MINIMIZED_PAD_X,
            center_y - MINIMIZED_SPARKLE / 2.0,
        ),
        size: Point2D::new(MINIMIZED_SPARKLE, MINIMIZED_SPARKLE),
    };
    let submit = Rect {
        origin: Point2D::new(
            rect.origin.x + rect.size.x - MINIMIZED_PAD_X - MINIMIZED_SUBMIT_D,
            center_y - MINIMIZED_SUBMIT_D / 2.0,
        ),
        size: Point2D::new(MINIMIZED_SUBMIT_D, MINIMIZED_SUBMIT_D),
    };
    let text_left = sparkle.origin.x + sparkle.size.x + MINIMIZED_GAP;
    let model_right = submit.origin.x - MINIMIZED_GAP;
    let model_w = footer_label_width(model_name, MINIMIZED_MODEL_FONT)
        .min(MINIMIZED_MODEL_MAX_W)
        .min(((model_right - text_left) * MINIMIZED_MODEL_MAX_SHARE).max(0.0));
    let model = Rect {
        origin: Point2D::new(model_right - model_w, center_y - MINIMIZED_MODEL_FONT),
        size: Point2D::new(model_w, MINIMIZED_MODEL_FONT * 2.0),
    };
    let text_right = if model_w > 0.0 {
        model.origin.x - MINIMIZED_GAP
    } else {
        model_right
    };
    let text = Rect {
        origin: Point2D::new(text_left, center_y - MINIMIZED_TEXT_FONT),
        size: Point2D::new((text_right - text_left).max(0.0), MINIMIZED_TEXT_FONT * 2.0),
    };
    MinimizedBarLayout {
        sparkle,
        text,
        model,
        submit,
    }
}

/// First line of the unsent draft, or `None` when the input is empty.
fn draft_line(draft: &str) -> Option<&str> {
    let line = draft.lines().find(|line| !line.trim().is_empty())?;
    Some(line.trim())
}

pub(crate) fn paint_minimized_bar(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    widget: &AIChatPlaceholder<'_>,
    rect: Rect,
) {
    use op_editor_core::ChatHeaderButton;

    // Lift the bar off the canvas with ONE very light shadow layer — the
    // expanded panel stacks two because it is a window; the bar is a
    // strip and reads heavy the moment its shadow is visible as such.
    cx.backend.fill_round_rect(
        Rect {
            origin: Point2D::new(rect.origin.x, rect.origin.y + 2.0),
            size: rect.size,
        },
        MINIMIZED_RADIUS,
        (Color::BLACK).with_alpha(0.05),
    );
    cx.backend
        .fill_round_rect(rect, MINIMIZED_RADIUS, (theme.card).with_alpha(0.98));
    // The whole bar is one button, so its feedback wash covers the whole
    // surface — the same neutral hover/press pair the header buttons use.
    let hovered = widget.header_hover == Some(ChatHeaderButton::ToggleCollapse);
    let pressed = widget.header_pressed == Some(ChatHeaderButton::ToggleCollapse);
    if hovered || pressed {
        cx.backend.fill_round_rect(
            rect,
            MINIMIZED_RADIUS,
            chat_neutral_feedback_color(theme, pressed),
        );
    }
    cx.backend
        .stroke_round_rect(rect, MINIMIZED_RADIUS, (theme.border).with_alpha(0.82), 1.0);

    let selected_model = widget.state.selected_model_entry();
    let model_name: &str = selected_model
        .map(|entry| entry.display_name.as_str())
        .unwrap_or(widget.label_no_models.as_str());
    let layout = minimized_bar_layout(rect, model_name);

    draw_icon(
        cx.backend,
        Icon::Sparkles,
        layout.sparkle.origin,
        MINIMIZED_SPARKLE,
        theme.muted_foreground,
        1.6,
    );

    // A reply in flight is the bar's own state, and it outranks the
    // placeholder: the panel can be collapsed at any point during a turn
    // (issue #255), so the bar has to say that work is happening rather than
    // look idle. A draft still wins over both — a user who typed and then
    // minimized must see what they were writing.
    let draft = draft_line(widget.state.input.text());
    let (label, label_color) = match draft {
        Some(line) => (line, theme.foreground),
        None if widget.state.has_streaming_turn() => {
            (widget.label_generating.as_str(), theme.foreground)
        }
        None => (
            widget.label_input_placeholder.as_str(),
            theme.muted_foreground,
        ),
    };
    if layout.text.size.x > 0.0 {
        let fitted = crate::util::ellipsize_to_width(label, layout.text.size.x, |s| {
            text_metrics::measure_chrome(cx.backend, s, MINIMIZED_TEXT_FONT)
        });
        let text = TextLayout::single_run(
            &fitted,
            "system-ui",
            MINIMIZED_TEXT_FONT,
            (label_color).to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                layout.text.origin.x,
                jian_widgets::centered_text_baseline_y(layout.text, MINIMIZED_TEXT_FONT),
            ),
        );
    }

    if layout.model.size.x > 0.0 {
        let fitted = fit_footer_label(
            cx.backend,
            model_name,
            MINIMIZED_MODEL_FONT,
            layout.model.size.x,
        );
        let text = TextLayout::single_run(
            &fitted,
            "system-ui",
            MINIMIZED_MODEL_FONT,
            (theme.muted_foreground)
                .with_alpha(theme.muted_foreground.a * MINIMIZED_MODEL_ALPHA)
                .to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                layout.model.origin.x,
                jian_widgets::centered_text_baseline_y(layout.model, MINIMIZED_MODEL_FONT),
            ),
        );
    }

    // Submit circle. Purely a visual promise of where sending happens
    // once expanded — pressing it expands like the rest of the bar.
    let has_draft = draft.is_some();
    let (submit_bg, submit_glyph) = if has_draft {
        (theme.primary, theme.primary_foreground)
    } else {
        (
            (theme.muted).with_alpha(0.25),
            Color {
                a: 0.3,
                ..theme.muted_foreground
            },
        )
    };
    cx.backend
        .fill_round_rect(layout.submit, MINIMIZED_SUBMIT_D / 2.0, submit_bg);
    draw_icon(
        cx.backend,
        Icon::ArrowUp,
        Point2D::new(
            layout.submit.origin.x + (MINIMIZED_SUBMIT_D - MINIMIZED_SUBMIT_GLYPH) / 2.0,
            layout.submit.origin.y + (MINIMIZED_SUBMIT_D - MINIMIZED_SUBMIT_GLYPH) / 2.0,
        ),
        MINIMIZED_SUBMIT_GLYPH,
        submit_glyph,
        1.6,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Rect {
        Rect::xywh(
            0.0,
            0.0,
            crate::widgets::AI_CHAT_WIDTH,
            AI_CHAT_MINIMIZED_HEIGHT,
        )
    }

    #[test]
    fn layout_parts_stay_inside_the_bar_and_do_not_overlap() {
        let layout = minimized_bar_layout(bar(), "Claude Opus 5");
        let right = bar().origin.x + bar().size.x;
        assert!(layout.sparkle.origin.x >= 0.0);
        assert!(layout.submit.origin.x + layout.submit.size.x <= right);
        assert!(layout.text.origin.x >= layout.sparkle.origin.x + layout.sparkle.size.x);
        assert!(layout.text.origin.x + layout.text.size.x <= layout.model.origin.x);
        assert!(layout.model.origin.x + layout.model.size.x <= layout.submit.origin.x);
    }

    #[test]
    fn a_long_model_name_cannot_squeeze_out_the_placeholder() {
        let layout = minimized_bar_layout(bar(), &"x".repeat(200));
        assert!(layout.model.size.x <= MINIMIZED_MODEL_MAX_W);
        assert!(
            layout.text.size.x > 0.0,
            "placeholder column must survive a long model name"
        );
    }

    #[test]
    fn the_narrowest_bar_still_gives_the_placeholder_the_larger_share() {
        // At the minimum width the absolute model ceiling is not enough —
        // 96 px of model name would leave the prompt a stub, so the share
        // rule has to take over.
        let layout = minimized_bar_layout(
            Rect::xywh(
                0.0,
                0.0,
                AI_CHAT_MINIMIZED_MIN_WIDTH,
                AI_CHAT_MINIMIZED_HEIGHT,
            ),
            "Some Very Long Model Name Indeed",
        );
        assert!(layout.model.origin.x + layout.model.size.x <= layout.submit.origin.x);
        assert!(
            layout.text.size.x > layout.model.size.x,
            "prompt column ({}) must stay wider than the model column ({})",
            layout.text.size.x,
            layout.model.size.x
        );
        assert!(
            layout.text.size.x >= 60.0,
            "prompt column must stay readable at the minimum width, got {}",
            layout.text.size.x
        );
    }

    #[test]
    fn draft_line_skips_blank_leading_lines() {
        assert_eq!(draft_line("\n\n  hello \nworld"), Some("hello"));
        assert_eq!(draft_line("   \n\t"), None);
        assert_eq!(draft_line(""), None);
    }
}
