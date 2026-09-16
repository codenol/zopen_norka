//! The per-panel paint arms of the native host's composition pass: the
//! right-rail PropertyPanel and its overlays, the floating VariablesPanel and
//! its preset menu, the Toolbar, the chat panel, the StatusBar, the floating
//! align toolbar, the marquee band, the reference card, and the
//! PropertyPanel's own image-fill popover.
//!
//! Split off `paint.rs` at the 800-line cap; pure code motion, so the arm
//! order below IS the z-order `paint` established and the press ladder
//! mirrors in reverse. The four frame facts the arms read — `presenting`,
//! `canvas_rect`, `canvas_w` and `dpi` — arrive as arguments rather than
//! being recomputed here.

use super::WidgetHostNative;
use crate::backend::NativeFrameBackend;
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::{
    variables_panel::VariablesPanel, AIChatPlaceholder, AlignToolbar, LayoutCx, PaintCx,
    PropertyPanel, StatusBar, Toolbar, Widget, TOOLBAR_WIDTH,
};
use op_editor_ui::{Rect, RenderBackend};

impl WidgetHostNative {
    /// Paint the editor's panel band — see the module doc for what it covers.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::widget_host) fn paint_panel_arms(
        &mut self,
        frame: &mut NativeFrameBackend<'_>,
        viewport_width: f32,
        viewport_height: f32,
        presenting: bool,
        canvas_rect: Rect,
        canvas_w: f32,
        dpi: f32,
    ) {
        // 5. PropertyPanel — only when selection.
        let property_panel = PropertyPanel::for_selection_at_with_scene(
            &self.editor_state,
            &self.layout_scene,
            self.now_ms,
        );
        let touch_layout = self.editor_state.editor_ui.touch_chrome();
        let properties_open = !touch_layout
            || self.editor_state.editor_ui.expanded_touch_layout()
            || self.editor_state.editor_ui.mobile_sheet
                == Some(op_editor_core::size_class::MobileSheetKind::Properties);
        if let Some(panel) = property_panel
            .as_ref()
            .filter(|_| !presenting && properties_open)
        {
            let property_rect = self.property_rect(viewport_width, viewport_height);
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            if self.editor_state.editor_ui.compact_layout()
                || self.editor_state.editor_ui.medium_layout()
            {
                crate::widget_host::paint_mobile::paint_property_sheet(
                    &self.editor_state,
                    panel,
                    &mut cx,
                    property_rect,
                );
            } else {
                panel.paint(&mut cx, property_rect);
            }
        }

        // 5b. VariablesPanel — mirrors TS' `{}` toolbar toggle as a
        //     floating canvas overlay next to the toolbar.
        if let Some(vars_rect) = self
            .variables_panel_rect(viewport_width, viewport_height)
            .filter(|_| !presenting)
        {
            let vars = VariablesPanel::for_editor_at(&self.editor_state, self.now_ms);
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            vars.paint(&mut cx, vars_rect);
        }

        // 5b-1. Theme-preset dropdown (#20) — painted after the panel
        //       so the functional menu covers the panel's static stub
        //       rows (variables_preset_press.rs owns the geometry).
        if let Some((preset_menu, preset_menu_rect)) = self
            .variables_preset_menu_with_rect(viewport_width, viewport_height)
            .filter(|_| !presenting)
        {
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            preset_menu.paint(&mut cx, preset_menu_rect);
        }

        // 6. Toolbar — floating column.
        let toolbar = Toolbar::for_editor(&self.editor_state);
        let toolbar_h = toolbar
            .layout(&LayoutCx {
                available_width: TOOLBAR_WIDTH,
                dpi,
            })
            .rect
            .size
            .y;
        let toolbar_rect = canvas_geometry::toolbar_rect(&self.editor_state, toolbar_h);
        let touch_layout = self.editor_state.editor_ui.touch_chrome();
        if canvas_geometry::toolbar_fits(canvas_w) && !presenting && !touch_layout {
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            toolbar.paint(&mut cx, toolbar_rect);
        }

        // 7. AIChatPlaceholder — painted LAST so it sits on top
        //    of the toolbar in any overlap region (matches the
        //    user's requested z-order: chat above toolbar).
        let chat_open = !touch_layout
            || self.editor_state.editor_ui.mobile_sheet
                == Some(op_editor_core::size_class::MobileSheetKind::Ai);
        if let Some(chat_rect) = self
            .ai_chat_rect(viewport_width, viewport_height)
            .filter(|_| !presenting && chat_open)
        {
            // Owner-stamp so paint stores the canonical build under THIS host's
            // owner — the display-frame cursor hint reads it back by that owner.
            let chat = AIChatPlaceholder::from_editor_at(&self.editor_state, self.now_ms)
                .owned_by(self.chat_panel_owner);
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            chat.paint(&mut cx, chat_rect);
        }

        // 8. StatusBar — floating bottom-right.
        if let Some(status_rect) =
            canvas_geometry::status_bar_rect(&self.editor_state, viewport_width, viewport_height)
                .filter(|_| !presenting && !touch_layout)
        {
            let status = StatusBar::for_editor(&self.editor_state);
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            status.paint(&mut cx, status_rect);
        }

        // 8.4. Floating align/distribute toolbar — visible whenever
        //      2+ nodes are selected. Sits above the canvas but
        //      below status / modal overlays.
        let canvas_region = canvas_rect;
        if self.preview.is_none()
            && !touch_layout
            && self.editor_state.editor_ui.mobile_sheet.is_none()
        {
            if let Some(toolbar) =
                AlignToolbar::for_canvas_region(canvas_region, &self.editor_state)
            {
                let hover = self.editor_state.editor_ui.align_toolbar_hover;
                toolbar.paint(&mut *frame, &self.theme, hover);
            }
        }

        // 8.5. Marquee selection rect — painted above canvas but
        //      below the floating pickers / status. Visible only
        //      while the user is dragging a rect-select on empty
        //      canvas (Select tool). Never in preview mode.
        if let Some(rect) = self
            .marquee_drag
            .filter(|_| self.preview.is_none())
            .as_ref()
            .and_then(canvas_geometry::marquee_rect)
        {
            {
                let primary = self.theme.primary;
                // 10% primary-tinted fill so the rect reads as a
                // selection band without obscuring the canvas.
                let fill = op_editor_ui::Color {
                    r: primary.r,
                    g: primary.g,
                    b: primary.b,
                    a: primary.a * 0.12,
                };
                frame.fill_rect(rect, fill);
                frame.stroke_rect(rect, primary, 1.0);
            }
        }

        // 8.55. Reference card (issue #63) — the picture the turn was asked
        //       to match, beside the generated frame. Over the canvas (so it
        //       cannot be selected or dragged), under every panel and modal
        //       that follows, which is also the order `apply_press` uses.
        if !presenting {
            if let Some(rect) = op_editor_ui::widgets::ReferenceView::card_rect(
                &self.editor_state,
                viewport_width,
                viewport_height,
            ) {
                if let Some(view) =
                    op_editor_ui::widgets::ReferenceView::from_state(&self.editor_state)
                {
                    let mut cx = PaintCx {
                        backend: &mut *frame,
                    };
                    view.paint(&mut cx, rect);
                }
            }
        }

        // 8.6. PropertyPanel overlays — painted after canvas floating
        //      controls so the image-fill popover can cover the zoom
        //      status pill when it extends into the canvas.
        let properties_open = !touch_layout
            || self.editor_state.editor_ui.expanded_touch_layout()
            || self.editor_state.editor_ui.mobile_sheet
                == Some(op_editor_core::size_class::MobileSheetKind::Properties);
        if let Some(panel) = property_panel
            .as_ref()
            .filter(|_| !presenting && properties_open)
        {
            let property_rect = self.property_rect(viewport_width, viewport_height);
            let mut cx = PaintCx {
                backend: &mut *frame,
            };
            panel.paint_overlays(&mut cx, property_rect);
            self.image_input_geometry =
                panel.image_popover_input_geometry(property_rect, &mut *frame);
        }
    }
}
