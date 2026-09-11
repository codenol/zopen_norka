//! `DesignMdPanel` — the floating design-rules panel.
//!
//! The panel is the editor's guidelines center: it lists the rules the
//! active component kit contributes plus the document's own rules and
//! overrides, and it owns the markdown editor for a single rule.
//!
//! There is deliberately **no markdown brief view** any more. Structured
//! rules are the single source of truth for both the author and the AI,
//! so a per-document `design.md` prose block has nothing left to
//! contribute — the schema still carries it for compatibility with older
//! `.op` files, and nothing reads it.
//!
//! The panel is platform-free: the host paints it, maps a click to a
//! [`DesignMdHit`], and owns dragging.

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::{Icon, PaintCx};
use crate::{Point2D, Rect};
use op_editor_core::{
    ButtonPressTarget, DesignMdButton, DesignRuleDraft, EditorState, Locale, PanelRow,
};

mod helpers;
// Shared button chrome + the text primitive, in a sibling file so this
// module stays under the repo's 800-line ceiling.
#[path = "design_md_panel_paint.rs"]
mod paint;

/// The rules view (filters, list, markdown editor) — layout + hit-test
/// here, paint in the `_rules_paint.rs` sibling.
#[path = "design_md_panel_rules.rs"]
mod rules;
#[path = "design_md_panel_rules_paint.rs"]
mod rules_paint;

/// Panel width in logical px.
pub const DESIGN_MD_PANEL_W: f32 = 480.0;
/// Panel height in logical px.
pub const DESIGN_MD_PANEL_H: f32 = 560.0;

const PAD: f32 = 14.0;
const HEADER_H: f32 = 40.0;

/// What a click landed on inside the rules panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignMdHit {
    /// The header ✕ — close the panel.
    Close,
    /// The header bar (not on a button) — start a panel drag.
    DragHeader,
    /// The "new rule" button — open a blank author rule.
    NewRule,
    /// The on/off switch of rule row `index`.
    RuleToggle(u16),
    /// The delete button of rule row `index`.
    RuleDelete(u16),
    /// The body of rule row `index` — open the editor on it.
    RuleEdit(u16),
    /// A click inside the editor's body — the byte offset for the caret.
    RuleBodyCaret(u32),
    /// A click inside an author rule's title field.
    RuleTitleCaret(u32),
    /// Save the editor — writes the rule into the document.
    RuleSave,
    /// Undo the last document change without leaving the editor.
    RuleUndo,
    /// Redo the last undone change without leaving the editor.
    RuleRedo,
    /// Close the editor without writing anything.
    RuleCancel,
    /// Inside the panel but not on an interactive target.
    Inside,
}

/// The floating design-rules panel, built from an [`EditorState`].
pub struct DesignMdPanel<'a> {
    pub(in crate::widgets) theme: Theme,
    locale: Locale,
    /// Which target the cursor is over — drives the hover wash.
    hover: Option<DesignMdButton>,
    /// Which target is actively pressed.
    pub(in crate::widgets) pressed: Option<DesignMdButton>,
    /// The rows the list shows: one document per component, then the
    /// author's own rules.
    pub(in crate::widgets) rows: Vec<PanelRow>,
    /// Vertical scroll offset of the rule list.
    rules_scroll: f32,
    /// The open create / edit editor, if any.
    pub(in crate::widgets) draft: Option<&'a DesignRuleDraft>,
    /// Whether the document history can undo / redo — drives the editor's
    /// own undo / redo buttons.
    pub(in crate::widgets) can_undo: bool,
    pub(in crate::widgets) can_redo: bool,
}

impl<'a> DesignMdPanel<'a> {
    /// Build the panel for the editor, or `None` when it is closed.
    pub fn for_editor(state: &'a EditorState) -> Option<DesignMdPanel<'a>> {
        if !state.editor_ui.design_md_panel.open {
            return None;
        }
        let kit = op_editor_core::session_kit();
        let panel_state = &state.editor_ui.design_md_panel;
        let spec = state.doc.design_md.as_ref();
        Some(DesignMdPanel {
            theme: theme_for(&state.editor_ui),
            locale: state.editor_ui.effective_locale(),
            hover: panel_state.hover,
            pressed: match state.editor_ui.pressed_button {
                Some(ButtonPressTarget::DesignMd(button)) => Some(button),
                _ => None,
            },
            rows: op_editor_core::panel_rows(kit, spec),
            rules_scroll: panel_state.rules_scroll.offset,
            draft: panel_state.rule_draft.as_ref(),
            can_undo: state.history.can_undo(),
            can_redo: state.history.can_redo(),
        })
    }

    /// Resolve a pointer to a hoverable button. Reuses [`Self::hit_test`]
    /// and keeps only the button variants (drag-header / inside → None).
    pub fn hover_at(&self, panel: Rect, point: Point2D) -> Option<DesignMdButton> {
        use DesignMdButton as B;
        Some(match self.hit_test(panel, point)? {
            DesignMdHit::Close => B::Close,
            DesignMdHit::NewRule => B::NewRule,
            DesignMdHit::RuleToggle(index) => B::RuleToggle(index),
            DesignMdHit::RuleDelete(index) => B::RuleDelete(index),
            DesignMdHit::RuleEdit(index) => B::RuleEdit(index),
            DesignMdHit::RuleBodyCaret(_) | DesignMdHit::RuleTitleCaret(_) => return None,
            DesignMdHit::RuleSave => B::RuleSave,
            DesignMdHit::RuleUndo => B::RuleUndo,
            DesignMdHit::RuleRedo => B::RuleRedo,
            DesignMdHit::RuleCancel => B::RuleCancel,
            DesignMdHit::DragHeader | DesignMdHit::Inside => return None,
        })
    }

    /// Translate `key` through the active locale tables.
    fn t(&self, key: &'static str) -> &'static str {
        crate::i18n::translate(self.locale, key)
    }

    fn is_pressed(&self, button: DesignMdButton) -> bool {
        self.pressed == Some(button)
    }

    /// How many rows the list currently shows.
    pub fn rule_count(&self) -> usize {
        self.rows.len()
    }

    /// The header's close button — the only chrome the panel still has.
    fn close_button(panel: Rect) -> Rect {
        let size = 24.0;
        Rect {
            origin: Point2D::new(
                panel.origin.x + panel.size.x - PAD - size,
                panel.origin.y + (HEADER_H - size) / 2.0,
            ),
            size: Point2D::new(size, size),
        }
    }

    /// `y` where the panel body begins.
    pub(super) fn body_top(&self, panel: Rect) -> f32 {
        panel.origin.y + HEADER_H
    }

    /// Map a click at `point` onto a [`DesignMdHit`]. `None` when the
    /// click is outside the panel rect.
    pub fn hit_test(&self, panel: Rect, point: Point2D) -> Option<DesignMdHit> {
        if !panel.contains(point) {
            return None;
        }
        if Self::close_button(panel).contains(point) {
            return Some(DesignMdHit::Close);
        }
        // Anywhere else in the header bar starts a drag.
        if point.y <= panel.origin.y + HEADER_H {
            return Some(DesignMdHit::DragHeader);
        }
        Some(self.hit_test_rules(panel, point))
    }

    /// Paint the panel into `rect`.
    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        cx.backend.fill_round_rect(rect, 12.0, self.theme.card);
        cx.backend
            .stroke_round_rect(rect, 12.0, self.theme.border, 1.0);

        // Header — title + close. The label is the rules-tab string the
        // panel used to carry in its tab strip; the panel is rules-only now,
        // so it is the panel's name rather than a tab's.
        self.text(
            cx,
            self.t("designMd.tab.rules"),
            rect.origin.x + PAD,
            rect.origin.y + HEADER_H / 2.0 + 4.0,
            13.0,
            self.theme.foreground,
        );
        self.icon_button(
            cx,
            Self::close_button(rect),
            Icon::Close,
            self.hover == Some(DesignMdButton::Close),
            self.is_pressed(DesignMdButton::Close),
        );
        cx.backend.fill_rect(
            Rect {
                origin: Point2D::new(rect.origin.x, rect.origin.y + HEADER_H),
                size: Point2D::new(rect.size.x, 1.0),
            },
            self.theme.border,
        );

        // Body — clipped so a long list cannot bleed past the panel.
        cx.backend.save();
        cx.backend.clip_rect(Rect {
            origin: Point2D::new(rect.origin.x, self.body_top(rect) + 1.0),
            size: Point2D::new(rect.size.x, rect.size.y - HEADER_H - 1.0),
        });
        self.paint_rules(cx, rect);
        cx.backend.restore();
    }
}
