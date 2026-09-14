//! Web `apply_press` tiers for comment threads.
//!
//! Three rungs, in the three places the comment surfaces sit in the hit-test
//! order:
//!
//! - **tier 1c** — the thread popover. It paints in the same band as the
//!   recovery banner, above the canvas and below every dropdown, modal and
//!   floating panel, so its tier sits immediately after the banner's and before
//!   the picker tiers.
//! - **tier 8b** — the rail's thread list. The rail has one occupant, and while
//!   the comment tool holds it the inspector is not built at all, so this rung
//!   sits exactly where the property panel's would have been. It is the same
//!   slot, in the same tier position, for the same reason: a press inside the
//!   rail belongs to the rail, a press beside it belongs to the canvas.
//! - **tier 11b** — a comment pin on the canvas, and the click that places one
//!   while the comment tool is active. A pin paints over the node tree but under
//!   the selection chrome; the press order follows paint order, so this rung
//!   sits immediately above the canvas tier and below everything that paints
//!   over the canvas.
//!
//! ## Why the rects are cached and the pins are not
//!
//! The popover and rail rects are the ones the paint pass drew
//! (`comments_popover_rect` / `comments_panel_rect`), for the reason the
//! recovery banner caches its own: the geometry depends on the canvas region
//! and on the conversation's length, either of which can change between the
//! paint and the press. The pins are different — they are placed from the same
//! numbers the paint used, so re-deriving them IS the parity, and caching them
//! would add a way to go stale.
//!
//! ## Why a click anywhere on the canvas places a pin
//!
//! The comment tool is active, so the click means "comment here" — on an
//! element, on a small icon, or on an empty patch of a frame. Nothing is
//! hit-tested to decide: the point is recorded as it was clicked, which is what
//! makes a comment about a four-pixel gap possible at all. The document
//! position comes from the shared screen→document helper, so the pin lands
//! where the cursor was under any pan or zoom.

use super::press_ctx::PressCtx;
use super::WidgetHost;
use op_editor_core::editor_ui_state::CommentAnchor;
use op_editor_ui::widgets::canvas_viewport::CanvasViewport;
use op_editor_ui::widgets::comments_flow::{self, CommentsPanelAction};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::{Point2D, Rect};

impl WidgetHost {
    /// The canvas region the comment surfaces are placed inside.
    fn comment_canvas_rect(&self, ctx: &PressCtx) -> Rect {
        canvas_geometry::canvas_rect(&self.editor_state, ctx.viewport_width, ctx.viewport_height)
    }

    /// Tier 1c — the comment popover.
    ///
    /// `None` — the popover did not claim the press.
    pub(in crate::widget_host) fn press_comments_tier(&mut self, ctx: &PressCtx) -> Option<bool> {
        let point = Point2D::new(ctx.x, ctx.y);
        if comments_flow::press_popover(&mut self.editor_state, self.comments_popover_rect, point) {
            self.mark_dirty();
            return Some(true);
        }
        None
    }

    /// Tier 8b — the rail's thread list.
    ///
    /// `None` — the rail is showing the inspector, or the press landed beside
    /// the list.
    pub(in crate::widget_host) fn press_comment_rail_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let point = Point2D::new(ctx.x, ctx.y);
        let page_id = self.active_page_id();
        let action = comments_flow::press_panel(
            &mut self.editor_state,
            self.comments_panel_rect,
            point,
            &page_id,
        );
        match action? {
            CommentsPanelAction::Handled => {
                self.mark_dirty();
                Some(true)
            }
            CommentsPanelAction::RevealAnchor(anchor) => {
                // Bringing the canvas to the point needs the viewport, which is
                // this host's — the widget flow opened the thread and asked for
                // the camera move rather than reaching for a viewport it cannot
                // see.
                self.centre_on_comment_anchor(&anchor, ctx.viewport_width, ctx.viewport_height);
                self.mark_dirty();
                Some(true)
            }
        }
    }

    /// Put a comment's point in the middle of the canvas.
    ///
    /// Centring rather than "reveal only if off screen": a row press is a jump,
    /// and a jump that sometimes does nothing (because the pin happened to be
    /// just inside the edge) reads as a broken row. Only the active page can be
    /// framed, which is the page the list is showing by construction.
    fn centre_on_comment_anchor(
        &mut self,
        anchor: &CommentAnchor,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if anchor.page_id != self.active_page_id() || !anchor.is_placeable() {
            return;
        }
        let canvas = canvas_geometry::canvas_rect(&self.editor_state, viewport_w, viewport_h);
        // The point ends up under the canvas centre. The pan is measured from
        // the canvas' own origin — `screen = canvas.origin + pan + doc * zoom`,
        // which is what `Viewport::to_document` inverts and what
        // `canvas_doc_mapping::doc_point_to_screen` applies — so the origin
        // cancels out of the equation and must not be added back in here.
        let zoom = self.editor_state.viewport.zoom;
        self.editor_state.viewport.pan_x = canvas.size.x / 2.0 - anchor.x as f32 * zoom;
        self.editor_state.viewport.pan_y = canvas.size.y / 2.0 - anchor.y as f32 * zoom;
    }

    /// Tier 11b — a pin under the cursor, or the click that places one.
    ///
    /// `None` — the press belongs to the canvas below.
    pub(in crate::widget_host) fn press_comment_pin_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        if !self.over_canvas(ctx.x, ctx.y, ctx.viewport_width, ctx.viewport_height) {
            return None;
        }
        // Preview (Play) mode owns the canvas: a pin is an editing affordance,
        // and a presentation that answered to one would be a presentation with a
        // hole in it.
        if self.editor_state.editor_ui.preview.mode {
            return None;
        }
        if !self.editor_state.editor_ui.comments.pin_mode {
            return None;
        }
        let canvas_rect = self.comment_canvas_rect(ctx);
        let touch = self.editor_state.editor_ui.touch;
        let point = Point2D::new(ctx.x, ctx.y);
        let canvas = CanvasViewport::from_editor(&self.editor_state, &self.layout_scene);
        // A pin under the cursor opens its thread — the marker is a target, and
        // a click on one means "show me this", not "leave another comment on top
        // of it".
        if let Some(thread_id) = canvas.hit_test_comment_pin(canvas_rect, point, touch) {
            self.editor_state.editor_ui.comments.open(thread_id);
            self.mark_dirty();
            return Some(true);
        }
        // The click is "comment on this point". Nothing is hit-tested: the
        // document position is taken as it landed, which is the whole point of a
        // coordinate comment (see the module notes).
        let page_id = self.active_page_id();
        let doc_point =
            canvas_geometry::canvas_doc_point_unclamped(&self.editor_state, ctx.x, ctx.y);
        self.editor_state
            .editor_ui
            .comments
            .begin_thread_at(CommentAnchor::new(
                page_id,
                doc_point.x as f64,
                doc_point.y as f64,
            ));
        self.mark_dirty();
        Some(true)
    }
}
