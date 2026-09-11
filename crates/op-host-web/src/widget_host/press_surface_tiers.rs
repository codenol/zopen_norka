//! Web `apply_press` tiers 9-11 — the AI chat panel, the toolbar, and the
//! LayerPanel drag peek + align toolbar that precede `apply_click`.
//!
//! The chat paints above the toolbar, so a press inside its rect is
//! consumed here even when that point also lies inside the toolbar rect
//! underneath. The align toolbar is hit-tested before `apply_click` so a
//! visible button always wins over a layer row sharing its screen y
//! (matches native order).

use super::press_ctx::PressCtx;
use super::{
    ChatDragState, ChatInputSelectionDragState, ChatTextSelectionDragState, LayerDragState,
    WidgetHost,
};
use op_editor_core::host_press_transitions as core_press;
use op_editor_ui::widgets::chat_click_flow;
use op_editor_ui::widgets::{AIChatHit, AIChatPlaceholder, LayerPanelHit, Toolbar, TOP_BAR_HEIGHT};
use op_editor_ui::Point2D;

impl WidgetHost {
    /// `None` — the chat did not claim the press.
    pub(in crate::widget_host) fn press_chat_tier(&mut self, ctx: &PressCtx) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let viewport_height = ctx.viewport_height;
        // 1. AI chat panel — painted on top of toolbar so a
        //    click inside its rect is consumed here, even when
        //    that point lies inside the toolbar rect underneath.
        if let Some(chat_rect) = self.ai_chat_rect(viewport_width, viewport_height) {
            let panel = AIChatPlaceholder::from_editor_at(&self.editor_state, self.now_ms)
                .owned_by(self.chat_panel_owner);
            if let Some(hit) = panel.hit_test(chat_rect, Point2D::new(x, y)) {
                if matches!(hit, AIChatHit::Resize(_)) {
                    return Some(true);
                }
                if let AIChatHit::SelectInputText(anchor) = hit {
                    self.chat_input_selection_drag = Some(ChatInputSelectionDragState { anchor });
                    chat_click_flow::begin_chat_input_selection(
                        &mut self.editor_state,
                        anchor,
                        self.now_ms,
                    );
                    self.mark_dirty();
                    return Some(true);
                }
                if let AIChatHit::SelectTranscriptText(message_index, anchor) = hit {
                    self.chat_text_selection_drag = Some(ChatTextSelectionDragState {
                        message_index,
                        anchor,
                    });
                    chat_click_flow::begin_chat_transcript_selection(
                        &mut self.editor_state,
                        message_index,
                        anchor,
                    );
                    self.mark_dirty();
                    return Some(true);
                }
                if matches!(hit, AIChatHit::DragHandle) {
                    self.chat_drag = Some(ChatDragState {
                        grab_dx: x - chat_rect.origin.x,
                        grab_dy: y - chat_rect.origin.y,
                        pos_x: chat_rect.origin.x,
                        pos_y: chat_rect.origin.y,
                    });
                    self.editor_state.chat.focused = false;
                    self.mark_dirty();
                    return Some(true);
                }
                let _ = self.apply_click(x, y, viewport_width, viewport_height);
                return Some(true);
            }
        }
        None
    }

    /// `None` — the toolbar did not claim the press.
    pub(in crate::widget_host) fn press_toolbar_tier(&mut self, ctx: &PressCtx) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let property_focus_committed = ctx.property_focus_committed;
        let rename_committed = ctx.rename_committed;
        let text_edit_committed = ctx.text_edit_committed;
        // 2. Toolbar — second-highest overlay. Bounding rect
        //    consumes all clicks (gaps + padding too) so it
        //    never falls through to the canvas for tool gaps
        //    that lie outside the chat panel.
        let toolbar_rect = self.toolbar_rect(viewport_width);
        let toolbar = Toolbar::for_editor(&self.editor_state);
        if (toolbar_rect).contains(Point2D::new(x, y)) {
            if let Some(hit) = toolbar.hit_test(toolbar_rect, Point2D::new(x, y)) {
                match hit {
                    op_editor_ui::widgets::ToolbarHit::Tool(tool) => {
                        self.apply_set_tool(tool);
                        core_press::close_shape_picker(&mut self.editor_state.editor_ui);
                        return Some(true);
                    }
                    op_editor_ui::widgets::ToolbarHit::Action(action) => {
                        core_press::close_shape_picker(&mut self.editor_state.editor_ui);
                        let acted = self.dispatch_toolbar_action(action);
                        return Some(acted || rename_committed || property_focus_committed);
                    }
                    op_editor_ui::widgets::ToolbarHit::ToggleShapePicker => {
                        core_press::toggle_shape_picker(&mut self.editor_state.editor_ui);
                        self.mark_dirty();
                        return Some(true);
                    }
                }
            }
            // Toolbar padding / gaps eat the click — blank press.
            let blurred = self.blur_text_inputs_on_blank_press();
            return Some(
                blurred || rename_committed || text_edit_committed || property_focus_committed,
            );
        }
        None
    }

    /// Seed a LayerPanel drag candidate, run the align toolbar, then
    /// `apply_click`. `None` — nothing consumed the press.
    pub(in crate::widget_host) fn press_layer_align_click_tiers(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let viewport_height = ctx.viewport_height;
        // 2b. The rail's slides tab owns the whole rail while it is on
        //     show, and its tab row takes clicks even while the layer
        //     tree owns the rest — so it is asked first.
        if self.slides_panel_press(x, y, viewport_width, viewport_height) {
            return Some(true);
        }
        // 3. apply_click — LayerPanel + chat-defocus.
        //    Pre-seed a `layer_drag` candidate when the press lands
        //    on a Layer row so a subsequent move past the threshold
        //    promotes the gesture to a drag-to-reorder.
        if self.editor_state.editor_ui.sidebar_open
            && !op_editor_ui::widgets::slides_panel_flow::assets_tab_active(&self.editor_state)
        {
            let layer_rect = self.layer_panel_rect(viewport_height);
            let panel = self.layer_panel();
            if let Some(LayerPanelHit::Layer(node_id)) =
                panel.hit_test(layer_rect, Point2D::new(x, y))
            {
                self.layer_drag = Some(LayerDragState {
                    source: node_id,
                    start_y: y,
                    current_x: x,
                    current_y: y,
                    active: false,
                });
            }
        }
        // 2.5. Floating align/distribute toolbar — visible when
        //      2+ nodes are selected. Hit-tested before apply_click
        //      so the visible button always wins over a layer row
        //      that happens to share screen y (matches native order).
        {
            use op_editor_ui::widgets::{AlignToolbar, AlignToolbarHit};
            let (acx, _, acw, ach) = self.canvas_region(viewport_width, viewport_height);
            let canvas_region = op_editor_ui::Rect {
                origin: Point2D::new(acx, TOP_BAR_HEIGHT),
                size: Point2D::new(acw, ach),
            };
            if let Some(hit) = AlignToolbar::for_canvas_region(canvas_region, &self.editor_state)
                .and_then(|tb| tb.hit_test_action(Point2D::new(x, y)))
            {
                match hit {
                    AlignToolbarHit::Align(action) => {
                        self.editor_state.align_selected(action);
                        self.mark_dirty();
                    }
                    AlignToolbarHit::Boolean(op) => {
                        let _ = self.apply_boolean_op(op);
                    }
                }
                return Some(true);
            }
        }

        if self.apply_click(x, y, viewport_width, viewport_height) {
            return Some(true);
        }
        None
    }
}
