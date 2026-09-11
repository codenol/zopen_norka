//! Rules panel — list and editor layout, hit-testing.
//!
//! Two states, one panel: the **list** (one row per component document,
//! then the author's own rules) and the **editor** (a markdown body plus
//! save / cancel / undo / redo). Layout lives here and is shared by paint
//! and hit-testing, so a click can never land somewhere paint did not draw.

use jian_widgets::components::text_area::TextArea;

use super::{DesignMdHit, DesignMdPanel, HEADER_H, PAD};
use crate::{Point2D, Rect};

/// Height of one list row.
pub(super) const ROW_H: f32 = 32.0;
/// Gap between list rows.
pub(super) const ROW_GAP: f32 = 4.0;
/// Size of a row's on/off switch.
pub(super) const ROW_TOGGLE_W: f32 = 30.0;
/// Size of a row's delete button (author rules only).
pub(super) const ROW_ACTION: f32 = 22.0;
/// Font size of the list and the editor.
pub(super) const FONT: f32 = 11.0;
/// Horizontal padding inside the editor fields.
pub(super) const PAD_X: f32 = 10.0;
/// Height of the editor's title field (author rules only).
pub(super) const EDITOR_TITLE_H: f32 = 26.0;
/// Height of the editor's body field.
pub(super) const EDITOR_BODY_H: f32 = 300.0;
/// Height of the editor's action buttons.
pub(super) const EDITOR_BUTTON_H: f32 = 26.0;

/// One list row plus its interactive sub-rects.
pub(in crate::widgets) struct RowLayout {
    pub(super) index: u16,
    /// The row body — clicking it opens the document.
    pub(super) rect: Rect,
    pub(super) toggle: Rect,
    /// `None` for component documents, which cannot be deleted.
    pub(super) delete: Option<Rect>,
}

/// The editor's field and button rects.
pub(in crate::widgets) struct EditorLayout {
    /// Present only for author rules — a component document's name is the
    /// component's own and is never edited here.
    pub(super) title: Option<Rect>,
    pub(in crate::widgets) body: Rect,
    pub(super) save: Rect,
    pub(super) cancel: Rect,
    pub(super) undo: Rect,
    pub(super) redo: Rect,
}

impl DesignMdPanel<'_> {
    /// `y` where the list starts.
    pub(super) fn list_top(panel: Rect) -> f32 {
        panel.origin.y + HEADER_H + 8.0
    }

    /// The "new rule" button, in a bar above the list.
    pub(super) fn new_rule_rect(panel: Rect) -> Rect {
        Rect::xywh(
            panel.origin.x + panel.size.x - PAD - 112.0,
            panel.origin.y + HEADER_H + 6.0,
            112.0,
            24.0,
        )
    }

    /// `y` where the rows start (below the new-rule bar).
    pub(crate) fn rows_top(panel: Rect) -> f32 {
        Self::list_top(panel) + 32.0
    }

    /// Every row, in list order.
    pub(super) fn row_rects(&self, panel: Rect) -> Vec<RowLayout> {
        (0..self.rows.len())
            .map(|index| {
                let rect = Rect::xywh(
                    panel.origin.x + PAD,
                    Self::rows_top(panel) + index as f32 * (ROW_H + ROW_GAP),
                    panel.size.x - PAD * 2.0,
                    ROW_H,
                );
                let toggle = Rect::xywh(
                    rect.origin.x + rect.size.x - PAD - ROW_ACTION - 8.0 - ROW_TOGGLE_W,
                    rect.origin.y + (ROW_H - ROW_ACTION) / 2.0,
                    ROW_TOGGLE_W,
                    ROW_ACTION,
                );
                let delete = self
                    .rows
                    .get(index)
                    .is_some_and(|row| row.removable)
                    .then(|| {
                        Rect::xywh(
                            rect.origin.x + rect.size.x - PAD - ROW_ACTION,
                            rect.origin.y + (ROW_H - ROW_ACTION) / 2.0,
                            ROW_ACTION,
                            ROW_ACTION,
                        )
                    });
                RowLayout {
                    index: index as u16,
                    rect,
                    toggle,
                    delete,
                }
            })
            .collect()
    }

    /// The editor's layout, or `None` while the list is showing.
    pub(in crate::widgets) fn editor_layout(&self, panel: Rect) -> Option<EditorLayout> {
        let draft = self.draft.as_ref()?;
        let left = panel.origin.x + PAD;
        let width = panel.size.x - PAD * 2.0;
        // Room for the document's name above the fields: a component's own
        // name is shown, never edited.
        let mut y = Self::list_top(panel) + 18.0;
        let title = draft.title_editable.then(|| {
            let rect = Rect::xywh(left, y, width, EDITOR_TITLE_H);
            y += EDITOR_TITLE_H + 8.0;
            rect
        });
        let body = Rect::xywh(left, y, width, EDITOR_BODY_H);
        y += EDITOR_BODY_H + 12.0;
        let button_w = 92.0;
        let save = Rect::xywh(left, y, button_w, EDITOR_BUTTON_H);
        let cancel = Rect::xywh(left + button_w + 8.0, y, button_w, EDITOR_BUTTON_H);
        let redo = Rect::xywh(
            panel.origin.x + panel.size.x - PAD - EDITOR_BUTTON_H,
            y,
            EDITOR_BUTTON_H,
            EDITOR_BUTTON_H,
        );
        let undo = Rect::xywh(
            redo.origin.x - EDITOR_BUTTON_H - 6.0,
            y,
            EDITOR_BUTTON_H,
            EDITOR_BUTTON_H,
        );
        Some(EditorLayout {
            title,
            body,
            save,
            cancel,
            undo,
            redo,
        })
    }

    /// Bottom of the list content, for the scroll clamp.
    pub(super) fn content_bottom(&self, panel: Rect) -> f32 {
        let rows = self.rows.len();
        Self::rows_top(panel) + rows as f32 * (ROW_H + ROW_GAP) + PAD
    }

    /// Clamped list scroll offset.
    pub(super) fn effective_scroll(&self, panel: Rect) -> f32 {
        let max = (self.content_bottom(panel) - (panel.origin.y + panel.size.y)).max(0.0);
        self.rules_scroll.clamp(0.0, max)
    }

    /// Public entry point for the host wheel-scroll clamp.
    pub fn max_rules_scroll(&self, panel: Rect) -> f32 {
        (self.content_bottom(panel) - (panel.origin.y + panel.size.y)).max(0.0)
    }

    /// Map a click inside the panel body onto a [`DesignMdHit`].
    pub(super) fn hit_test_rules(&self, panel: Rect, point: Point2D) -> DesignMdHit {
        if self.draft.is_some() {
            return self.hit_test_editor(panel, point);
        }
        if Self::new_rule_rect(panel).contains(point) {
            return DesignMdHit::NewRule;
        }
        let content = Point2D::new(point.x, point.y + self.effective_scroll(panel));
        for row in self.row_rects(panel) {
            if row.toggle.contains(content) {
                return DesignMdHit::RuleToggle(row.index);
            }
            if row.delete.is_some_and(|rect| rect.contains(content)) {
                return DesignMdHit::RuleDelete(row.index);
            }
            if row.rect.contains(content) {
                return DesignMdHit::RuleEdit(row.index);
            }
        }
        DesignMdHit::Inside
    }

    fn hit_test_editor(&self, panel: Rect, point: Point2D) -> DesignMdHit {
        let Some(editor) = self.editor_layout(panel) else {
            return DesignMdHit::Inside;
        };
        if let Some(title) = editor.title {
            if title.contains(point) {
                return DesignMdHit::RuleTitleCaret(self.title_offset_at(title, point) as u32);
            }
        }
        if editor.body.contains(point) {
            return DesignMdHit::RuleBodyCaret(self.body_offset_at(editor.body, point) as u32);
        }
        if editor.save.contains(point) {
            return DesignMdHit::RuleSave;
        }
        if editor.cancel.contains(point) {
            return DesignMdHit::RuleCancel;
        }
        if editor.undo.contains(point) {
            return DesignMdHit::RuleUndo;
        }
        if editor.redo.contains(point) {
            return DesignMdHit::RuleRedo;
        }
        DesignMdHit::Inside
    }

    /// Byte offset inside the body field for a click at `point`.
    ///
    /// Measured through the same jian `TextArea` the field paints with, so
    /// a click lands exactly where the glyph under the cursor is.
    pub(super) fn body_offset_at(&self, field: Rect, point: Point2D) -> usize {
        let Some(draft) = self.draft.as_ref() else {
            return 0;
        };
        self.offset_in(&draft.body, field, point)
    }

    /// Byte offset inside the title field for a click at `point`.
    pub(super) fn title_offset_at(&self, field: Rect, point: Point2D) -> usize {
        let Some(draft) = self.draft.as_ref() else {
            return 0;
        };
        self.offset_in(&draft.title_input, field, point)
    }

    fn offset_in(
        &self,
        input: &jian_core::text_input::TextInputState,
        field: Rect,
        point: Point2D,
    ) -> usize {
        let mut backend = crate::widgets::ai_chat_input_text::MeasureOnlyBackend;
        let area = TextArea {
            state: input,
            placeholder: "",
            focused: true,
            font_size: FONT,
            now_ms: 0,
            pad_x: PAD_X,
            max_visible_lines: 0,
        };
        area.byte_offset_at(
            &mut backend,
            field,
            point,
            &crate::widgets::button::tokens_from_theme(&self.theme),
        )
    }
}
