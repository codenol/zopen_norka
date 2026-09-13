//! Web `apply_press` tiers for comment threads.
//!
//! Two rungs, in the two places the comment surfaces sit in the hit-test order:
//!
//! - **tier 1c** — the thread list panel and the thread popover. Both paint in
//!   the same band as the recovery banner, above the canvas and below every
//!   dropdown, modal and floating panel, so their tier sits immediately after
//!   the banner's and before the picker tiers.
//! - **tier 11b** — a comment pin on the canvas, and the click that places one
//!   while pin mode is armed. A pin paints over the node tree but under the
//!   selection chrome; the press order follows paint order, so this rung sits
//!   immediately above the canvas tier and below everything that paints over
//!   the canvas.
//!
//! ## Why the rects are cached and the pins are not
//!
//! The panel and popover rects are the ones the paint pass drew
//! (`comments_panel_rect` / `comments_popover_rect`), for the reason the
//! recovery banner caches its own: the geometry depends on the canvas region
//! and on the conversation's length, either of which can change between the
//! paint and the press. The pins are different — they are placed from the render
//! tree by the same call the paint made, so re-deriving them IS the parity, and
//! caching them would add a way to go stale.

use super::press_ctx::PressCtx;
use super::WidgetHost;
use op_editor_ui::widgets::canvas_viewport::CanvasViewport;
use op_editor_ui::widgets::comments_flow::{self, CommentsPanelAction};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::host_overlay_geometry as overlay_geometry;
use op_editor_ui::{Point2D, Rect};

impl WidgetHost {
    /// Whether an element with this id is in the document the canvas shows.
    ///
    /// The one question the list needs to mark a thread "no pin" — asked of the
    /// render tree, which is the same answer the pin placement uses, so the list
    /// and the canvas can never disagree about which threads are attachable.
    fn comment_node_exists(&self, node_id: &str) -> bool {
        self.layout_scene
            .active_page()
            .is_some_and(|page| page.find(node_id).is_some())
    }

    /// The canvas region the comment surfaces are placed inside.
    fn comment_canvas_rect(&self, ctx: &PressCtx) -> Rect {
        canvas_geometry::canvas_rect(&self.editor_state, ctx.viewport_width, ctx.viewport_height)
    }

    /// Tier 1c — the comment popover, then the thread list panel.
    ///
    /// `None` — neither claimed the press.
    pub(in crate::widget_host) fn press_comments_tier(&mut self, ctx: &PressCtx) -> Option<bool> {
        let point = Point2D::new(ctx.x, ctx.y);
        // The pill first: it only exists while the panel is shut, so the two
        // never contend for the corner.
        if comments_flow::press_toggle(&mut self.editor_state, self.comments_toggle_rect, point) {
            self.mark_dirty();
            return Some(true);
        }
        // The popover first: it paints last of the two, and a popover opened from
        // a row overlaps the panel it came from.
        if comments_flow::press_popover(&mut self.editor_state, self.comments_popover_rect, point) {
            self.mark_dirty();
            return Some(true);
        }
        let scene = &self.layout_scene;
        let action = comments_flow::press_panel(
            &mut self.editor_state,
            self.comments_panel_rect,
            point,
            &|node_id| {
                scene
                    .active_page()
                    .is_some_and(|page| page.find(node_id).is_some())
            },
        );
        match action? {
            CommentsPanelAction::Handled => {
                self.mark_dirty();
                Some(true)
            }
            CommentsPanelAction::RevealNode(node_id) => {
                // Framing the element on the canvas needs the render tree, which
                // is this host's — the widget flow selected the node and asked
                // for the camera move rather than reaching for a viewport it
                // cannot see.
                overlay_geometry::zoom_to_fit_node(
                    &mut self.editor_state,
                    &self.layout_scene,
                    &node_id,
                    ctx.viewport_width,
                    ctx.viewport_height,
                );
                self.mark_dirty();
                Some(true)
            }
        }
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
        let canvas_rect = self.comment_canvas_rect(ctx);
        let touch = self.editor_state.editor_ui.touch;
        let point = Point2D::new(ctx.x, ctx.y);
        let canvas = CanvasViewport::from_editor(&self.editor_state, &self.layout_scene);
        if let Some(thread_id) = canvas.hit_test_comment_pin(canvas_rect, point, touch) {
            self.editor_state.editor_ui.comments.open(thread_id);
            self.mark_dirty();
            return Some(true);
        }
        if !self.editor_state.editor_ui.comments.pin_mode {
            return None;
        }
        // Pin mode: the click is "comment on this element", so it must not also
        // select and drag it. The element under the cursor is the deepest node of
        // the hit path, which is the one the reviewer sees.
        let doc_point =
            canvas_geometry::canvas_doc_point_unclamped(&self.editor_state, ctx.x, ctx.y);
        let target = self
            .layout_scene
            .node_path_at_doc_point(doc_point, self.editor_state.viewport.zoom)
            .and_then(|path| path.last().map(|id| id.to_string()));
        let Some(target) = target else {
            // An empty patch of canvas: nothing to attach a comment to. The mode
            // stays armed, so the next click on an element works.
            return Some(true);
        };
        self.editor_state.editor_ui.comments.begin_thread_on(target);
        self.mark_dirty();
        Some(true)
    }
}
