//! The reference card's press tier (issue #63).
//!
//! The card paints over the canvas, so its press has to be resolved before
//! the canvas tier turns the same click into a selection, a marquee, or a
//! deselection. It is deliberately called LATE — after every menu, modal,
//! floating panel, rail and tooltip tier — because the card paints under all
//! of them: where a panel covers the card, the panel owns the click.

use super::press_ctx::PressCtx;
use super::WidgetHostNative;
use op_editor_ui::widgets::reference_view::{ReferenceView, ReferenceViewHit};

impl WidgetHostNative {
    /// `None` — the press was not inside the card. `Some(true)` — consumed
    /// (the card body is inert but still absorbs the press; see
    /// [`ReferenceViewHit::Inside`]).
    pub(in crate::widget_host) fn press_reference_view_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let Some(rect) =
            ReferenceView::card_rect(&self.editor_state, ctx.viewport_width, ctx.viewport_height)
        else {
            return None;
        };
        match ReferenceView::hit_test(rect, op_editor_ui::Point2D::new(ctx.x, ctx.y)) {
            Some(ReferenceViewHit::Close) => {
                self.editor_state.editor_ui.reference_view.close();
                self.mark_dirty();
                Some(true)
            }
            Some(ReferenceViewHit::Inside) => Some(true),
            None => None,
        }
    }
}
