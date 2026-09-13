//! Web `apply_press` tier 1b — the launch-time recovery banner (#26).
//!
//! The banner paints above the canvas and below the whole menu / modal /
//! floating-panel band, so its tier sits between tier 1 (which owns that band)
//! and the picker tiers below it. Hit-test is reverse paint order, and this is
//! where the bar falls in it.
//!
//! One divergence is deliberate and harmless: the file-menu and export
//! dropdowns are handled in tier 3, while they paint *after* the bar. They are
//! anchored in the TopBar's corners and the bar is centred in the canvas, so
//! the two do not overlap at any viewport width the editor supports; if they
//! ever did, the bar would win a press the menu should have had, which costs
//! one click to reopen.

use super::press_ctx::PressCtx;
use super::WidgetHost;
use op_editor_ui::Point2D;

impl WidgetHost {
    /// `None` — no recovery offer claimed the press.
    pub(in crate::widget_host) fn press_recovery_banner_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        // The rect the paint pass drew, not a re-derived one: the bar's
        // vertical slot moves with the align toolbar and the toast, either of
        // which can have changed since. The shared flow re-checks the offer is
        // still on screen before trusting the cache, so a stale rect cannot
        // swallow a press aimed at the canvas.
        if !op_editor_ui::widgets::recovery_banner_flow::press(
            &mut self.editor_state,
            self.recovery_banner_rect,
            Point2D::new(ctx.x, ctx.y),
            self.now_ms,
        ) {
            return None;
        }
        // The press only *records* the answer (`RecoveryRequest`); the request
        // itself is made by the frame tick, because this path has no transport
        // and no `CkInner` to hand a response. See `crate::web_recovery`.
        self.mark_dirty();
        Some(true)
    }
}
