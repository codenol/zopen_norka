//! Design-MD-panel press dispatch — extracted from `press.rs` to
//! keep that file under the repo's 800-line cap.
//!
//! The floating Design-MD panel sits alongside the Git panel in
//! paint order; `apply_press` calls
//! [`WidgetHostNative::dispatch_design_md_press`] after the Git panel
//! and before the canvas overlays.

use op_editor_ui::widgets::{DesignMdHit, DesignMdPanel};
use op_editor_ui::Point2D;

use super::{DesignMdDragState, WidgetHostNative};

impl WidgetHostNative {
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
        // The gate is resolved first — the flow needs `&mut editor_state`.
        let allow_rule_mutation = self.collab_allows_document_mutation(
            op_editor_core::CollabDocumentMutation::Unsupported(
                op_editor_core::CollabUnsupportedFeature::RootMetadata,
            ),
        );
        if op_editor_ui::widgets::apply_design_rules_hit(
            &mut self.editor_state,
            hit,
            allow_rule_mutation,
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
                self.design_md_drag = Some(DesignMdDragState {
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
