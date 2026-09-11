//! Design-MD-panel press dispatch — mirror of the native host's
//! `widget_host/design_md_press.rs`.
//!
//! The floating Design-MD panel paints top-most; `apply_press` calls
//! [`WidgetHost::dispatch_design_md_press`] first so a click on its
//! rect is the panel's before any lower layer can claim it.

use op_editor_ui::widgets::{DesignMdHit, DesignMdPanel};
use op_editor_ui::Point2D;

use super::{PanelDragState, WidgetHost};

impl WidgetHost {
    /// Dispatch a press inside the floating Design-MD panel.
    ///
    /// Returns `true` when the click was consumed — a close, a
    /// section toggle, a queued import / export request, a drag
    /// start, or a swallowed click inside the panel body.
    pub(in crate::widget_host) fn dispatch_design_md_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let Some(panel_rect) = self.design_md_panel_rect(viewport_width, viewport_height) else {
            return false;
        };
        let point = Point2D::new(x, y);
        let Some((hit, pressed_button)) =
            DesignMdPanel::for_editor(&self.editor_state).and_then(|p| {
                Some((
                    p.hit_test(panel_rect, point)?,
                    p.hover_at(panel_rect, point),
                ))
            })
        else {
            return false;
        };
        if let Some(button) = pressed_button {
            self.editor_state.editor_ui.pressed_button =
                Some(op_editor_core::ButtonPressTarget::DesignMd(button));
        }
        // Rule hits (filters, row switches, the markdown editor) go through
        // the shared flow, which owns the `EditorCommand` writes and the
        // undo snapshot; it returns `false` for close / drag / blank press.
        // The browser host has no collaboration gate on document edits.
        if op_editor_ui::widgets::apply_design_rules_hit(
            &mut self.editor_state,
            hit,
            true,
            self.now_ms,
        ) {
            self.mark_dirty();
            return true;
        }
        match hit {
            DesignMdHit::Close => {
                self.editor_state.editor_ui.design_md_panel.open = false;
                self.editor_state.editor_ui.design_md_panel.hover = None;
            }
            DesignMdHit::DragHeader => {
                self.design_md_drag = Some(PanelDragState {
                    grab_dx: x - panel_rect.origin.x,
                    grab_dy: y - panel_rect.origin.y,
                });
            }
            DesignMdHit::Inside => {
                // Blank press on panel chrome — blur chrome inputs.
                self.blur_text_inputs_on_blank_press();
            }
            // Rule variants were consumed by the shared flow above.
            _ => {}
        }
        self.mark_dirty();
        true
    }
}
