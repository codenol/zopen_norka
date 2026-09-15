//! Web `apply_press` tiers 7-8 — the property-panel popover band and the
//! PropertyPanel input row.
//!
//! Every popover follows the same contract: a row hit applies, a press on
//! the popup chrome is swallowed, and the first outside press dismisses
//! and is swallowed.

use super::press_ctx::PressCtx;
use super::{CodeSelectionDragState, WidgetHost};
use op_editor_core::codegen::CodeSelection;
use op_editor_ui::widgets::press_flow;
use op_editor_ui::widgets::{PropertyPanel, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

impl WidgetHost {
    /// Fill-type, compositing, effects, colour-variable, and export
    /// pickers. `None` — none of them claimed the press.
    pub(in crate::widget_host) fn press_property_overlay_tiers(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let viewport_height = ctx.viewport_height;
        // 0c0. Fill-type picker — outside-click dismiss. A row
        // click applies the fill type; a click inside the popup body
        // is swallowed; any outside click closes the picker.
        if self.editor_state.editor_ui.fill_type_picker.open {
            let press = press_flow::press_fill_type_picker(
                &mut self.editor_state,
                viewport_width,
                viewport_height,
                Point2D::new(x, y),
            );
            return Some(self.finish_property_overlay_press(press));
        }

        // 0c0a. Layer / mask / fill-blend compositing picker — row clicks
        // apply through the same undo-safe action dispatch as native; popup
        // chrome swallows, and the first outside click only dismisses.
        if self.editor_state.editor_ui.compositing_picker.open {
            let press = press_flow::press_compositing_picker(
                &mut self.editor_state,
                viewport_width,
                viewport_height,
                Point2D::new(x, y),
            );
            return Some(self.finish_property_overlay_press(press));
        }

        // 0c0z. Effects "+" add-menu — outside-click dismiss.
        if self.editor_state.editor_ui.effect_add_picker_open {
            let press = press_flow::press_effect_add_menu(
                &mut self.editor_state,
                viewport_width,
                viewport_height,
                Point2D::new(x, y),
            );
            return Some(self.finish_property_overlay_press(press));
        }

        // 0c0a0. Fill/stroke colour-variable picker — outside-click dismiss.
        if self
            .editor_state
            .editor_ui
            .property_color_variable_picker_open
            .is_some()
        {
            let press = press_flow::press_color_variable_picker(
                &mut self.editor_state,
                viewport_width,
                viewport_height,
                Point2D::new(x, y),
            );
            return Some(self.finish_property_overlay_press(press));
        }

        // 0c0b. Export scale / format inline select popup —
        //       outside-click dismiss. A click on a popup row or a
        //       dropdown toggle is applied; any other click closes
        //       both pickers and is swallowed. Mirrors the native
        //       host's `0c0b` block.
        if self.editor_state.editor_ui.export_scale_picker_open
            || self.editor_state.editor_ui.export_format_picker_open
        {
            if let Some(panel) = PropertyPanel::for_selection(&self.editor_state) {
                let property_rect = Rect {
                    origin: Point2D::new(
                        viewport_width - self.editor_state.editor_ui.property_panel_width,
                        TOP_BAR_HEIGHT,
                    ),
                    size: Point2D::new(
                        self.editor_state.editor_ui.property_panel_width,
                        (viewport_height - TOP_BAR_HEIGHT).max(0.0),
                    ),
                };
                if let Some(action) = panel.hit_test_action(property_rect, Point2D::new(x, y)) {
                    if matches!(
                        action,
                        op_editor_ui::widgets::PropertyPanelAction::SetExportScale(_)
                            | op_editor_ui::widgets::PropertyPanelAction::SetExportFormat(_)
                            | op_editor_ui::widgets::PropertyPanelAction::ToggleExportScalePicker
                            | op_editor_ui::widgets::PropertyPanelAction::ToggleExportFormatPicker
                    ) {
                        self.apply_property_action(action);
                        return Some(true);
                    }
                }
            }
            self.editor_state.editor_ui.export_scale_picker_open = false;
            self.editor_state.editor_ui.export_format_picker_open = false;
            self.mark_dirty();
            return Some(true);
        }
        None
    }

    /// Image popovers, font-family / font-weight / padding / stroke
    /// popovers, then the chat model-picker overlay.
    /// `None` — none of them claimed the press.
    pub(in crate::widget_host) fn press_font_and_picker_tiers(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let viewport_height = ctx.viewport_height;

        // 0c0b1. Image-node Search / Generate popovers — overlay
        // controls win; outside clicks dismiss.
        if self.dismiss_image_popovers_on_press(x, y, viewport_width, viewport_height) {
            return Some(true);
        }

        // 0c0b2. Font-family picker — outside-click dismiss. A click
        //        on an entry / the trigger is applied; one inside the
        //        popup body (search box / headers) is swallowed.
        if self.editor_state.editor_ui.font_picker.open {
            use op_editor_ui::widgets::PropertyPanelAction as A;
            if let Some(panel) = PropertyPanel::for_selection(&self.editor_state) {
                let property_rect = Rect {
                    origin: Point2D::new(
                        viewport_width - self.editor_state.editor_ui.property_panel_width,
                        TOP_BAR_HEIGHT,
                    ),
                    size: Point2D::new(
                        self.editor_state.editor_ui.property_panel_width,
                        (viewport_height - TOP_BAR_HEIGHT).max(0.0),
                    ),
                };
                let point = Point2D::new(x, y);
                if let Some(action) = panel.hit_test_action(property_rect, point) {
                    if matches!(
                        action,
                        A::SetFontFamilyIndex(_)
                            | A::ToggleFontFamilyPicker
                            | A::ImportFont
                            | A::RemoveImportedFont(_)
                    ) {
                        self.apply_property_action(action);
                        return Some(true);
                    }
                }
                if panel.font_picker_contains(property_rect, point) {
                    return Some(true);
                }
            }
            let ui = &mut self.editor_state.editor_ui;
            ui.close_font_picker();
            self.mark_dirty();
            return Some(true);
        }

        // 0c0c. Font-weight dropdown + padding mode-selector popover —
        //       outside-click dismiss. A click on a picker row / toggle
        //       is applied; any other click closes the popover and is
        //       swallowed (mirrors the native host's dismiss handlers).
        if self.editor_state.editor_ui.font_weight_picker_open
            || self.editor_state.editor_ui.padding_mode_popover_open
            || self.editor_state.editor_ui.stroke_mode_popover_open
        {
            use op_editor_ui::widgets::PropertyPanelAction as A;
            if let Some(panel) = PropertyPanel::for_selection(&self.editor_state) {
                let property_rect = Rect {
                    origin: Point2D::new(
                        viewport_width - self.editor_state.editor_ui.property_panel_width,
                        TOP_BAR_HEIGHT,
                    ),
                    size: Point2D::new(
                        self.editor_state.editor_ui.property_panel_width,
                        (viewport_height - TOP_BAR_HEIGHT).max(0.0),
                    ),
                };
                if let Some(action) = panel.hit_test_action(property_rect, Point2D::new(x, y)) {
                    if matches!(
                        action,
                        A::SetFontWeight(_)
                            | A::ToggleFontWeightPicker
                            | A::SetPaddingMode(_)
                            | A::TogglePaddingModePopover
                            | A::SetStrokeMode(_)
                            | A::ToggleStrokeModePopover
                    ) {
                        if let A::SetFontWeight(choice) = action {
                            self.editor_state.editor_ui.pressed_button =
                                op_editor_ui::widgets::FontWeightChoice::ALL
                                    .iter()
                                    .position(|c| *c == choice)
                                    .map(op_editor_core::ButtonPressTarget::FontWeightPicker);
                            self.mark_dirty();
                            return Some(true);
                        }
                        self.apply_property_action(action);
                        return Some(true);
                    }
                }
            }
            self.editor_state.editor_ui.font_weight_picker_open = false;
            self.editor_state.editor_ui.font_weight_picker_hover = None;
            self.editor_state.editor_ui.padding_mode_popover_open = false;
            self.editor_state.editor_ui.padding_mode_popover_hover = None;
            self.editor_state.editor_ui.stroke_mode_popover_open = false;
            self.editor_state.editor_ui.stroke_mode_popover_hover = None;
            self.mark_dirty();
            return Some(true);
        }

        // The model dropdown is part of the chat's floating layer and may
        // extend beyond the chat rect. Route its painted bounds before the
        // base Property/Layer panels can consume the press; the popovers above
        // already had first refusal in the blocks above.
        if self.apply_chat_model_picker_overlay_press(x, y, viewport_width, viewport_height) {
            return Some(true);
        }
        None
    }

    /// PropertyPanel code selection / action / input-row focus.
    /// `None` — the press was not inside the panel.
    pub(in crate::widget_host) fn press_property_panel_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        let viewport_width = ctx.viewport_width;
        let viewport_height = ctx.viewport_height;

        // 0c. PropertyPanel button / checkbox — flex modes + size
        //     flags. Runs AFTER locale picker + TopBar so the
        //     dropdown overlays still win.
        if let Some(panel) =
            PropertyPanel::for_selection_with_scene(&self.editor_state, &self.layout_scene)
        {
            let property_rect = Rect {
                origin: Point2D::new(
                    viewport_width - self.editor_state.editor_ui.property_panel_width,
                    TOP_BAR_HEIGHT,
                ),
                size: Point2D::new(
                    self.editor_state.editor_ui.property_panel_width,
                    (viewport_height - TOP_BAR_HEIGHT).max(0.0),
                ),
            };
            if let Some(anchor) = self.code_text_offset_at_screen(x, y) {
                self.commit_property_family_focus_if_any();
                self.editor_state.codegen.code_selection = Some(CodeSelection {
                    anchor,
                    focus: anchor,
                });
                self.code_selection_drag = Some(CodeSelectionDragState { anchor });
                self.editor_state.chat.transcript_selection = None;
                self.editor_state.codegen.framework_hover = None;
                self.editor_state.codegen.action_hover = None;
                self.editor_state.chat.focused = false;
                self.mark_dirty();
                return Some(true);
            }
            let point = Point2D::new(x, y);
            // The Section block (#59) sits above every ordinary section, so it
            // is asked first: a click on a summary question focuses it, and a
            // click anywhere else in the block blurs rather than falling
            // through to the fields the block has pushed down.
            if let Some(block) = panel.section_block_rect(property_rect) {
                if block.contains(point) {
                    let fields =
                        op_editor_ui::widgets::property_panel_section_block::section_field_rects(
                            &self.editor_state.editor_ui.section_panel,
                            block.origin.x,
                            block.origin.y,
                            block.size.x,
                        );
                    let hit = fields
                        .into_iter()
                        .find(|(_, rect)| rect.contains(point))
                        .map(|(field, _)| field);
                    match hit {
                        Some(field) => {
                            self.editor_state
                                .editor_ui
                                .section_panel
                                .focus_field(field, self.now_ms);
                        }
                        None => self.editor_state.editor_ui.section_panel.blur(),
                    }
                    self.commit_property_focus_if_any();
                    self.mark_dirty();
                    return Some(true);
                }
            }
            if let Some(action) = panel.hit_test_action(property_rect, point) {
                self.editor_state.editor_ui.pressed_button =
                    if let op_editor_ui::widgets::PropertyPanelAction::Codegen(codegen_action) =
                        action
                    {
                        op_editor_ui::widgets::property_panel_code::codegen_hover_for_action(
                            codegen_action,
                        )
                        .map(op_editor_core::ButtonPressTarget::Codegen)
                    } else {
                        panel
                            .action_hover_index(property_rect, point)
                            .map(op_editor_core::ButtonPressTarget::PropertyPanel)
                    };
                self.commit_property_focus_if_any();
                // Anchor the colour picker at the clicked y so it
                // pops next to the swatch row, not at the panel top.
                if let op_editor_ui::widgets::PropertyPanelAction::OpenColorPicker(target) = action
                {
                    let _ = self.editor_state.open_color_picker(
                        super::property_dispatch::color_target_public(target),
                        y,
                    );
                    self.mark_dirty();
                } else if let op_editor_ui::widgets::PropertyPanelAction::OpenFillColorPicker(
                    index,
                ) = action
                {
                    // Non-primary fill swatch — bind the picker to this
                    // fill so HSV writes back to `fills[index]`.
                    self.editor_state.editor_ui.close_color_variable_picker();
                    let _ = self.editor_state.open_color_picker_for_fill(
                        op_editor_core::ui_draft::ColorTarget::Fill,
                        index,
                        y,
                    );
                    self.mark_dirty();
                } else if let op_editor_ui::widgets::PropertyPanelAction::OpenEffectColorPicker(
                    index,
                ) = action
                {
                    let _ = self.editor_state.open_color_picker(
                        op_editor_core::ui_draft::ColorTarget::EffectColor(index),
                        y,
                    );
                    self.mark_dirty();
                } else {
                    self.apply_property_action(action);
                }
                return Some(true);
            }
            if let Some(focus) = panel.hit_test(property_rect, point) {
                return Some(self.focus_property_input_from_press(focus, property_rect, point));
            }
            if (property_rect).contains(point) {
                self.blur_text_inputs_on_blank_press();
                return Some(true);
            }
        }
        None
    }
}
