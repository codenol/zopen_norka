//! Shared geometry helpers for the web widget host.

use super::WidgetHost;
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::host_overlay_geometry as overlay_geometry;
use op_editor_ui::widgets::{Toolbar, TopBar};
use op_editor_ui::{Point2D, Rect};

impl WidgetHost {
    pub(in crate::widget_host) fn top_bar(&self) -> TopBar {
        TopBar::for_editor_ui(&self.editor_state.editor_ui).with_traffic_controls(false)
    }

    pub(in crate::widget_host) fn top_bar_rect(&self, viewport_w: f32) -> Rect {
        overlay_geometry::top_bar_rect(viewport_w)
    }

    /// Canvas region (logical px, viewport-relative). The math is
    /// single-sourced with the native host — see the coordinate invariant
    /// in `op_editor_ui::widgets::host_canvas_geometry`.
    pub(in crate::widget_host) fn canvas_region(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> (f32, f32, f32, f32) {
        canvas_geometry::canvas_region(&self.editor_state, viewport_w, viewport_h)
    }

    pub(in crate::widget_host) fn over_canvas(
        &self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        canvas_geometry::over_canvas(&self.editor_state, x, y, viewport_w, viewport_h)
    }

    /// `(anchor, viewport)` for the top-bar import dropdown — mirrors
    /// the native host so both chromes clamp the popup identically.
    /// `(anchor, viewport)` for the top-bar import dropdown — shared with
    /// the native host so both chromes clamp the popup identically.
    pub(in crate::widget_host) fn import_menu_anchor(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> (Rect, Rect) {
        overlay_geometry::import_menu_anchor(&self.editor_state, viewport_w, viewport_h)
    }

    /// Close the import dropdown and clear its hover row.
    /// Close the import dropdown and clear its hover row.
    pub(in crate::widget_host) fn close_import_menu(&mut self) {
        overlay_geometry::close_import_menu(&mut self.editor_state);
    }

    pub(in crate::widget_host) fn locale_picker_rect(&self, viewport_w: f32) -> Rect {
        overlay_geometry::locale_picker_rect(&self.editor_state, viewport_w)
    }

    /// The rail rect the LAYERS tree gets — the whole rail, less the
    /// slides tab row when this document shows one. Every layer-tree
    /// caller goes through here, so the tab row can never shift the
    /// painted rows out from under their own click targets.
    pub(in crate::widget_host) fn layer_panel_rect(&self, viewport_h: f32) -> Rect {
        self.layers_content_rect(viewport_h)
    }

    pub(in crate::widget_host) fn toolbar_rect(&mut self, _viewport_w: f32) -> Rect {
        self.refresh_layout_scene();
        canvas_geometry::toolbar_rect_for(&self.editor_state)
    }

    /// Per-button hover wash on the floating toolbar. Mirrors
    /// `op_host_native::widget_host::geometry::update_toolbar_hover`.
    /// Returns `true` if the hover state changed.
    pub(in crate::widget_host) fn update_toolbar_hover(&mut self, x: f32, y: f32) -> bool {
        let rect = self.toolbar_rect(self.last_viewport_w);
        let toolbar = Toolbar::for_editor(&self.editor_state);
        let new_hover = toolbar
            .hit_test(rect, Point2D::new(x, y))
            .map(op_editor_ui::widgets::editor_state_ext::toolbar_hover);
        if new_hover != self.editor_state.editor_ui.toolbar_hover {
            self.editor_state.editor_ui.toolbar_hover = new_hover;
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// The id of the page being edited, as every page-scoped projection names it.
    ///
    /// Read from the editor state, which is where the rule lives
    /// (`EditorState::active_page_identity`) and which the render scene builder
    /// also calls — so the canvas, the pins and the comment rail can only ever
    /// agree about which page they are on. Never empty: a document with no
    /// `pages` array gets the same synthesized id the scene uses for it.
    pub(in crate::widget_host) fn active_page_id(&self) -> String {
        self.editor_state.active_page_identity().0
    }

    /// Anchor / bezier-handle hit-test for the selected Path node.
    /// The math is shared with the native host — see
    /// `op_editor_ui::widgets::host_canvas_geometry::path_anchor_hit`.
    pub(in crate::widget_host) fn path_anchor_hit(
        &self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<(String, usize, super::AnchorDragTarget)> {
        canvas_geometry::path_anchor_hit(
            &self.editor_state,
            &self.layout_scene,
            x,
            y,
            viewport_w,
            viewport_h,
        )
    }
}
