//! The Share dialog — who may open this document, and at what level.
//!
//! ## What it replaces
//!
//! The top bar used to carry a "Collaborate" button that opened the live
//! collaboration panel. Issue #56 asks for a Figma-shaped access dialog
//! instead, and the chip now opens this. The collaboration panel is not gone:
//! the dialog's footer row keeps it reachable, because the chip was its only
//! entrance and quietly closing an entrance is not a trade this change is
//! entitled to make.
//!
//! ## What this dialog decides, and what it does not
//!
//! Nothing here talks to a server. A press maps to a [`ShareAction`] on the
//! state's pending queue and the host drains it; the answers come back through
//! the state (`apply_list`, `record_granted`, `record_issued`,
//! `record_refusal`). That is what lets the whole surface be tested without a
//! socket, and what keeps the one rule that matters — the dialog never says
//! something happened that did not — in a place a test can ask.
//!
//! ## The four levels
//!
//! See [`op_editor_core::ShareLevel`]. They are the operator's four, not
//! Figma's two, because the matrix has a row — comment and invite without
//! editing — that view-or-edit cannot express, and hiding it would hide the
//! thing five of the seven product roles exist to do.
//!
//! ## Why there is no "People from team" row
//!
//! Figma has one because Figma has teams: an organisation grouping that grants
//! access by membership. This deployment has accounts and, per document, an
//! access list — and nothing else. A "team" row here would have to mean one of
//! two things, and both are worse than the missing row: the deployment's whole
//! account list (an administrator's surface, which would turn every share
//! dialog into a directory of everybody) or a membership that does not exist.
//! What the row is actually used for — "who else is in this, and at what
//! level" — is what "Who has access" below answers, with a count per level.
//!
//! ## Why "Anyone with the link" says more than Figma's version
//!
//! Because it means less. There is no anonymous identity on this deployment:
//! access requires a signed-in account, so the switch can only ever grant to
//! somebody who can sign in. The caption says so rather than letting the label
//! imply a public URL that would not work.

use op_editor_core::editor_ui_state::share::{ShareLevelTarget, ShareRow, ShareUiState};
use op_editor_core::{EditorState, ShareLevel};
use op_i18n::Locale;

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::share_dialog_layout::{
    self, card_width, layout, ShareDialogLayout, ShareLayoutSpec,
};
use crate::widgets::share_dialog_model::ShareDialogModel;
use crate::widgets::{LayoutBox, LayoutCx, PaintCx, Widget, WidgetId};
use crate::{Point2D, Rect};

#[path = "share_dialog_flow.rs"]
mod flow;
#[path = "share_dialog_paint.rs"]
mod paint;
pub use flow::{
    apply_share_hit, invite_char_allowed, invite_field_backspace, invite_field_paste,
    invite_field_submit, invite_field_text, MAX_INVITE_FIELD_CHARS,
};

/// What a press on the dialog landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareDialogHit {
    Close,
    CopyLink,
    /// The comma-separated invite field.
    InviteField,
    Invite,
    /// The "Anyone with the link" switch.
    LinkAccess,
    /// The level control of the link row.
    LinkLevel,
    /// The level control of the Nth person row.
    PersonLevel(usize),
    /// The trash control of the Nth person row.
    PersonRemove(usize),
    /// Copy the Nth issued invitation link.
    CopyInviteLink(usize),
    /// The Nth option of an open level picker.
    LevelOption(usize),
    /// The footer row that opens the live collaboration panel.
    OpenSession,
    /// The card, but not a control on it.
    Inside,
    /// The scrim. Consumed as well: the dialog is modal, so a press outside it
    /// must not reach the canvas behind.
    Outside,
}

/// The drawn dialog.
pub struct ShareDialog<'a> {
    pub id: WidgetId,
    pub theme: Theme,
    locale: Locale,
    ui: &'a ShareUiState,
    model: ShareDialogModel,
    viewport: (f32, f32),
}

impl<'a> ShareDialog<'a> {
    /// The dialog to paint, or `None` when it is closed.
    ///
    /// A closed dialog returns `None` rather than an empty one so a host that
    /// paints whatever it is handed cannot paint a scrim over the editor for a
    /// dialog the user dismissed.
    pub fn for_editor(state: &'a EditorState, viewport_w: f32, viewport_h: f32) -> Option<Self> {
        if !state.editor_ui.share.open {
            return None;
        }
        Some(Self {
            id: WidgetId::new(5800),
            theme: theme_for(&state.editor_ui),
            locale: state.editor_ui.effective_locale(),
            ui: &state.editor_ui.share,
            model: ShareDialogModel::for_state(state),
            viewport: (viewport_w, viewport_h),
        })
    }

    /// Where the card sits.
    pub fn rect(&self) -> Rect {
        share_dialog_layout::card_rect(self.viewport.0, self.viewport.1, self.layout_spec())
    }

    /// The layout paint and hit-test both walk.
    pub fn layout(&self) -> ShareDialogLayout {
        layout(self.viewport.0, self.viewport.1, self.layout_spec())
    }

    /// The spec the layout is built from.
    pub fn layout_spec(&self) -> ShareLayoutSpec {
        self.model.spec
    }

    /// The strings and rows this frame paints.
    pub fn model(&self) -> &ShareDialogModel {
        &self.model
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// What a press landed on.
    ///
    /// The picker is checked first: it is painted over the card, and a press
    /// inside it must never fall through to the control it is covering.
    pub fn hit_test(&self, point: Point2D) -> ShareDialogHit {
        let layout = self.layout();
        for (index, (_, rect)) in layout.level_options.iter().enumerate() {
            if rect.contains(point) {
                return ShareDialogHit::LevelOption(index);
            }
        }
        if let Some(menu) = layout.level_menu {
            if menu.contains(point) {
                // Inside the popover but between its rows: consumed, no action.
                return ShareDialogHit::Inside;
            }
        }
        if layout.copy_link.contains(point) {
            return ShareDialogHit::CopyLink;
        }
        if layout.close.contains(point) {
            return ShareDialogHit::Close;
        }
        if layout.invite_field.contains(point) {
            return ShareDialogHit::InviteField;
        }
        if layout.invite_button.contains(point) {
            return ShareDialogHit::Invite;
        }
        if layout.invite_level.contains(point) {
            return ShareDialogHit::LinkLevel;
        }
        if layout.link_switch.contains(point) {
            return ShareDialogHit::LinkAccess;
        }
        if layout.link_level.contains(point) {
            return ShareDialogHit::LinkLevel;
        }
        if layout.footer.contains(point) {
            return ShareDialogHit::OpenSession;
        }
        for link in &layout.invite_links {
            if link.copy.contains(point) {
                return ShareDialogHit::CopyInviteLink(link.index);
            }
        }
        for row in &layout.people {
            if row.level.contains(point) {
                return ShareDialogHit::PersonLevel(row.index);
            }
            if row.remove.contains(point) {
                return ShareDialogHit::PersonRemove(row.index);
            }
        }
        if layout.card.contains(point) {
            return ShareDialogHit::Inside;
        }
        ShareDialogHit::Outside
    }

    /// Which row the pointer is over, for the hover tint.
    ///
    /// A `ShareRow` rather than a hit so the paint and the press share one
    /// vocabulary — a control that highlights but does not respond is exactly
    /// the bug this prevents.
    pub fn row_at(&self, point: Point2D) -> Option<ShareRow> {
        match self.hit_test(point) {
            ShareDialogHit::Close => Some(ShareRow::Close),
            ShareDialogHit::CopyLink => Some(ShareRow::CopyLink),
            ShareDialogHit::InviteField => Some(ShareRow::InviteField),
            ShareDialogHit::Invite => Some(ShareRow::Invite),
            ShareDialogHit::LinkAccess => Some(ShareRow::LinkAccess),
            ShareDialogHit::LinkLevel => Some(ShareRow::LinkLevel),
            ShareDialogHit::PersonLevel(index) => Some(ShareRow::PersonLevel(index)),
            ShareDialogHit::PersonRemove(index) => Some(ShareRow::PersonRemove(index)),
            ShareDialogHit::LevelOption(index) => Some(ShareRow::LevelOption(index)),
            ShareDialogHit::OpenSession => Some(ShareRow::OpenSession),
            ShareDialogHit::CopyInviteLink(_)
            | ShareDialogHit::Inside
            | ShareDialogHit::Outside => None,
        }
    }

    /// The level an option press would set on the control the picker opened for.
    pub fn level_option(&self, index: usize) -> Option<ShareLevel> {
        self.model
            .level_options
            .get(index)
            .map(|(level, _, _)| level.to_owned())
    }

    /// The picker target this dialog is currently open for.
    pub fn level_target(&self) -> Option<ShareLevelTarget> {
        self.ui.level_picker
    }
}

impl Widget for ShareDialog<'_> {
    fn id(&self) -> WidgetId {
        self.id
    }

    fn layout(&self, _cx: &LayoutCx) -> LayoutBox {
        LayoutBox {
            rect: Rect::xywh(0.0, 0.0, card_width(self.viewport.0), self.rect().size.y),
        }
    }

    fn paint(&self, cx: &mut PaintCx<'_>, _rect: Rect) {
        paint::paint(
            cx,
            &self.theme,
            self.locale,
            self.ui,
            &self.model,
            &self.layout(),
            self.viewport,
        );
    }

    fn access_node(&self) -> accesskit::Node {
        let mut node = accesskit::Node::new(accesskit::Role::Dialog);
        node.set_label(self.model.title.clone());
        node
    }
}

#[cfg(test)]
#[path = "share_dialog_tests.rs"]
mod tests;
