//! Rules-panel paint: the document list and the markdown editor.
//!
//! Split from `design_md_panel_rules.rs` (layout + hit-test) so both files
//! stay well under the 800-line ceiling, and a `#[path]` submodule of
//! `design_md_panel` so this second `impl DesignMdPanel` block reaches the
//! parent's private theme / `t` / `text` helpers.

use jian_widgets::components::text_area::TextArea;

use super::helpers::truncate;
use super::rules::{EditorLayout, EDITOR_BUTTON_H, FONT, PAD_X, ROW_ACTION, ROW_H, ROW_TOGGLE_W};
use super::{DesignMdPanel, PAD};
use crate::widgets::button::{
    paint_button_feedback_wash, paint_ghost_button_feedback, tokens_from_theme,
};
use crate::widgets::{draw_icon, Icon, PaintCx};
use crate::{Color, Point2D, Rect};
use op_editor_core::DesignMdButton;

/// Baseline nudge for the markdown fields, matching the chat input's.
const BASELINE_ASCENT: f32 = 14.0;

/// Dim a colour for a disabled control.
fn dim(color: Color, factor: f32) -> Color {
    color.with_alpha(color.a * factor)
}

impl DesignMdPanel<'_> {
    /// Paint the whole panel body: the list, or the open document.
    pub(in crate::widgets) fn paint_rules(&self, cx: &mut PaintCx<'_>, panel: Rect) {
        if let Some(editor) = self.editor_layout(panel) {
            self.paint_editor(cx, panel, &editor);
            return;
        }
        self.paint_list(cx, panel);
    }

    fn paint_list(&self, cx: &mut PaintCx<'_>, panel: Rect) {
        let new_rule = Self::new_rule_rect(panel);
        jian_widgets::components::button::Button {
            label: self.t("designMd.rules.new"),
            icon_paths: Some(Icon::Plus.paths()),
            variant: jian_widgets::components::button::ButtonVariant::Secondary,
            enabled: true,
            hovered: self.hover == Some(DesignMdButton::NewRule),
            pressed: self.is_pressed(DesignMdButton::NewRule),
            font_size: 11.0,
        }
        .paint(cx.backend, new_rule, &tokens_from_theme(&self.theme));

        if self.rows.is_empty() {
            self.text(
                cx,
                self.t("designMd.rules.empty"),
                panel.origin.x + PAD,
                super::DesignMdPanel::rows_top(panel) + 20.0,
                FONT,
                self.theme.muted_foreground,
            );
            return;
        }

        let scroll = self.effective_scroll(panel);
        let viewport = Rect::xywh(
            panel.origin.x,
            super::DesignMdPanel::rows_top(panel),
            panel.size.x,
            panel.size.y - (super::DesignMdPanel::rows_top(panel) - panel.origin.y),
        );
        for row in self.row_rects(panel) {
            let Some(model) = self.rows.get(row.index as usize) else {
                continue;
            };
            let rect = Rect {
                origin: Point2D::new(row.rect.origin.x, row.rect.origin.y - scroll),
                size: row.rect.size,
            };
            if rect.origin.y + rect.size.y < viewport.origin.y
                || rect.origin.y > viewport.origin.y + viewport.size.y
            {
                continue;
            }
            self.paint_row(cx, row.index, rect, model, scroll);
        }
    }

    fn paint_row(
        &self,
        cx: &mut PaintCx<'_>,
        index: u16,
        rect: Rect,
        model: &op_editor_core::PanelRow,
        scroll: f32,
    ) {
        cx.backend.fill_round_rect(rect, 7.0, self.theme.muted);
        paint_button_feedback_wash(
            cx.backend,
            &self.theme,
            rect,
            7.0,
            self.hover == Some(DesignMdButton::RuleEdit(index)),
            false,
        );
        let enabled = model.enabled;
        let label = if model.is_primary {
            self.t("designMd.rules.aiInstructions").to_string()
        } else {
            truncate(&model.label, 34)
        };
        self.text(
            cx,
            &label,
            rect.origin.x + 10.0,
            rect.origin.y + ROW_H / 2.0 + 4.0,
            FONT,
            if enabled {
                self.theme.foreground
            } else {
                dim(self.theme.foreground, 0.55)
            },
        );

        // On / off switch.
        let toggle = Rect::xywh(
            rect.origin.x + rect.size.x - PAD - ROW_ACTION - 8.0 - ROW_TOGGLE_W,
            rect.origin.y + (ROW_H - ROW_ACTION) / 2.0,
            ROW_TOGGLE_W,
            ROW_ACTION,
        );
        cx.backend.fill_round_rect(
            toggle,
            ROW_ACTION / 2.0,
            if enabled {
                self.theme.primary
            } else {
                self.theme.input
            },
        );
        paint_button_feedback_wash(
            cx.backend,
            &self.theme,
            toggle,
            ROW_ACTION / 2.0,
            self.hover == Some(DesignMdButton::RuleToggle(index)),
            self.is_pressed(DesignMdButton::RuleToggle(index)),
        );
        let icon = if enabled { Icon::Check } else { Icon::Close };
        let icon_size = ROW_TOGGLE_W.min(ROW_ACTION) - 12.0;
        draw_icon(
            cx.backend,
            icon,
            Point2D::new(
                toggle.origin.x + (toggle.size.x - icon_size) / 2.0,
                toggle.origin.y + (toggle.size.y - icon_size) / 2.0,
            ),
            icon_size,
            if enabled {
                self.theme.primary_foreground
            } else {
                self.theme.muted_foreground
            },
            1.6,
        );

        // Component documents are never deletable, so they paint no bin.
        if !model.removable {
            return;
        }
        let delete = Rect::xywh(
            rect.origin.x + rect.size.x - PAD - ROW_ACTION,
            rect.origin.y + (ROW_H - ROW_ACTION) / 2.0,
            ROW_ACTION,
            ROW_ACTION,
        );
        let delete_hovered = self.hover == Some(DesignMdButton::RuleDelete(index));
        let color = paint_ghost_button_feedback(
            cx.backend,
            &self.theme,
            delete,
            delete_hovered,
            self.is_pressed(DesignMdButton::RuleDelete(index)),
        );
        let size = ROW_ACTION - 8.0;
        draw_icon(
            cx.backend,
            Icon::Trash,
            Point2D::new(
                delete.origin.x + (delete.size.x - size) / 2.0,
                delete.origin.y + (delete.size.y - size) / 2.0,
            ),
            size,
            if delete_hovered {
                self.theme.destructive
            } else {
                color
            },
            1.6,
        );
        let _ = scroll;
    }

    fn paint_editor(&self, cx: &mut PaintCx<'_>, panel: Rect, editor: &EditorLayout) {
        let Some(draft) = self.draft else {
            return;
        };
        // The document's name sits above the text — for a component it is
        // the component's own name and is not editable.
        let name = if draft.id == op_editor_core::AI_INSTRUCTION_RULE_ID {
            self.t("designMd.rules.aiInstructions").to_string()
        } else {
            truncate(draft.title_text(), 44)
        };
        self.text(
            cx,
            &name,
            panel.origin.x + PAD,
            super::DesignMdPanel::list_top(panel) + 12.0,
            12.0,
            if draft.title_editable {
                self.theme.foreground
            } else {
                self.theme.muted_foreground
            },
        );
        if let Some(title) = editor.title {
            self.paint_field(
                cx,
                title,
                &draft.title_input,
                true,
                "designMd.rules.form.title",
            );
        }
        self.paint_field(
            cx,
            editor.body,
            &draft.body,
            true,
            "designMd.rules.bodyHint",
        );

        let tokens = tokens_from_theme(&self.theme);
        let valid = draft.is_valid();
        jian_widgets::components::button::Button {
            label: self.t("designMd.rules.form.save"),
            icon_paths: Some(Icon::Check.paths()),
            variant: jian_widgets::components::button::ButtonVariant::Primary,
            enabled: valid,
            hovered: valid && self.hover == Some(DesignMdButton::RuleSave),
            pressed: self.is_pressed(DesignMdButton::RuleSave),
            font_size: 11.0,
        }
        .paint(cx.backend, editor.save, &tokens);
        jian_widgets::components::button::Button {
            label: self.t("designMd.rules.form.cancel"),
            icon_paths: None,
            variant: jian_widgets::components::button::ButtonVariant::Secondary,
            enabled: true,
            hovered: self.hover == Some(DesignMdButton::RuleCancel),
            pressed: self.is_pressed(DesignMdButton::RuleCancel),
            font_size: 11.0,
        }
        .paint(cx.backend, editor.cancel, &tokens);

        for (rect, icon, enabled, hovered) in [
            (
                editor.undo,
                Icon::Undo,
                self.can_undo,
                self.hover == Some(DesignMdButton::RuleUndo),
            ),
            (
                editor.redo,
                Icon::Redo,
                self.can_redo,
                self.hover == Some(DesignMdButton::RuleRedo),
            ),
        ] {
            cx.backend.fill_round_rect(
                rect,
                7.0,
                if enabled {
                    self.theme.muted
                } else {
                    dim(self.theme.muted, 0.6)
                },
            );
            if enabled {
                paint_button_feedback_wash(cx.backend, &self.theme, rect, 7.0, hovered, false);
            }
            let size = editor_layout_metrics::ICON;
            draw_icon(
                cx.backend,
                icon,
                Point2D::new(
                    rect.origin.x + (rect.size.x - size) / 2.0,
                    rect.origin.y + (rect.size.y - size) / 2.0,
                ),
                size,
                if enabled {
                    self.theme.foreground
                } else {
                    dim(self.theme.muted_foreground, 0.5)
                },
                1.6,
            );
        }

        if !valid {
            self.text(
                cx,
                self.t("designMd.rules.form.incomplete"),
                editor.save.origin.x,
                editor.save.origin.y + EDITOR_BUTTON_H + 14.0,
                10.0,
                self.theme.status_warning,
            );
        }
    }

    /// One text field: box, value, caret — the jian `TextArea` does the
    /// wrapping, caret and selection.
    fn paint_field(
        &self,
        cx: &mut PaintCx<'_>,
        rect: Rect,
        input: &jian_core::text_input::TextInputState,
        focused: bool,
        placeholder_key: &'static str,
    ) {
        cx.backend.fill_round_rect(rect, 7.0, self.theme.input);
        cx.backend.stroke_round_rect(
            rect,
            7.0,
            if focused {
                self.theme.ring
            } else {
                self.theme.border
            },
            1.0,
        );
        cx.backend.save();
        cx.backend.clip_rect(rect);
        let mut backend = crate::widgets::text_input_backend::BaselineAdjustingBackend {
            inner: cx.backend,
            baseline_delta_y: BASELINE_ASCENT,
        };
        let area = TextArea {
            state: input,
            placeholder: self.t(placeholder_key),
            focused,
            font_size: FONT,
            now_ms: 0,
            pad_x: PAD_X,
            max_visible_lines: 0,
        };
        area.paint(&mut backend, rect, &tokens_from_theme(&self.theme));
        cx.backend.restore();
    }
}

/// Small metrics used only by the editor chrome.
mod editor_layout_metrics {
    /// Icon size inside an undo / redo button.
    pub(super) const ICON: f32 = 14.0;
}
