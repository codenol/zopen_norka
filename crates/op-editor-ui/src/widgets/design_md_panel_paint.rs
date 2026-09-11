//! Rules-panel paint helpers — the shared button chrome and the text
//! primitive every row and chip draws through.
//!
//! Split out of `design_md_panel.rs` to keep that module under the
//! 800-line ceiling. A `#[path]` submodule (declared as a child of
//! `design_md_panel`, not re-exported) so this second `impl
//! DesignMdPanel` block keeps access to the parent's private items
//! (theme constants / `t` / `is_pressed`).

use super::DesignMdPanel;
use crate::widgets::button::paint_button_feedback_wash;
use crate::widgets::{Icon, PaintCx};
use crate::{Color, Point2D, Rect, TextLayout};

impl DesignMdPanel<'_> {
    /// Paint one square header icon button.
    pub(super) fn icon_button(
        &self,
        cx: &mut PaintCx<'_>,
        rect: Rect,
        icon: Icon,
        hovered: bool,
        pressed: bool,
    ) {
        cx.backend.fill_round_rect(rect, 6.0, self.theme.muted);
        paint_button_feedback_wash(cx.backend, &self.theme, rect, 6.0, hovered, pressed);
        jian_widgets::components::icon_button::IconButton {
            icon_paths: icon.paths(),
            hovered,
            pressed,
            active: false,
            enabled: true,
            icon_size: rect.size.x - 10.0,
            stroke_width: 1.5,
        }
        .paint(
            cx.backend,
            rect,
            &crate::widgets::button::tokens_from_theme(&self.theme),
        );
    }

    /// Draw one line of text.
    pub(super) fn text(
        &self,
        cx: &mut PaintCx<'_>,
        s: &str,
        x: f32,
        baseline_y: f32,
        size: f32,
        color: Color,
    ) {
        let layout = TextLayout::single_run(
            s,
            "system-ui",
            size,
            (color).to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(&layout, Point2D::new(x, baseline_y));
    }
}
