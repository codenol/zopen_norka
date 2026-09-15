//! Web arms for the Share dialog (#56).
//!
//! Everything the dialog decides lives in the platform-free half —
//! `op_editor_ui::widgets::share_dialog` (vocabulary, layout, hit-test, paint)
//! and its `share_dialog_flow` sibling (what a press does). This file is the
//! tail only: turn a point into a hit, resolve a level option through the
//! widget that knows where the popover's rows are, and mark the frame dirty.
//!
//! ## Why the press is consumed even when it lands on the scrim
//!
//! The dialog is modal. `ShareDialogHit::Outside` is a control of its own —
//! it closes the open level picker and blurs the invite field — and a press
//! that fell through to the canvas behind a card the user is looking at would
//! both move their selection and leave the dialog claiming a focus it no
//! longer has. Same rule the account entry form follows one tier above.

use super::WidgetHost;
use op_editor_ui::widgets::share_dialog::{apply_share_hit, ShareDialog, ShareDialogHit};
use op_editor_ui::Point2D;

impl WidgetHost {
    /// Whether the Share dialog owns the press. `false` when it is closed.
    pub(in crate::widget_host) fn dispatch_share_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let Some(dialog) =
            ShareDialog::for_editor(&self.editor_state, viewport_width, viewport_height)
        else {
            return false;
        };
        let hit = dialog.hit_test(Point2D::new(x, y));
        // The widget owns the popover rows, so it is the layer that can say
        // which level an option press names; `apply_share_hit` deliberately
        // does not guess one from the index alone.
        let level = match hit {
            ShareDialogHit::LevelOption(index) => dialog.level_option(index),
            _ => None,
        };
        let consumed = apply_share_hit(&mut self.editor_state.editor_ui.share, hit, level);
        self.mark_dirty();
        consumed
    }

    /// The hover tint for the row under the pointer. `false` when nothing moved.
    pub(in crate::widget_host) fn update_share_hover(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let Some(dialog) =
            ShareDialog::for_editor(&self.editor_state, viewport_width, viewport_height)
        else {
            return false;
        };
        let hover = dialog.row_at(Point2D::new(x, y));
        if self.editor_state.editor_ui.share.hover == hover {
            return false;
        }
        self.editor_state.editor_ui.share.hover = hover;
        self.mark_dirty();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::WidgetHost;
    use op_editor_core::editor_ui_state::share::{ShareRow, ShareUiState};
    use op_editor_ui::widgets::share_dialog::ShareDialog;
    use op_editor_ui::Point2D;

    const VIEWPORT_W: f32 = 1200.0;
    const VIEWPORT_H: f32 = 800.0;

    fn host_with_share(share: ShareUiState) -> WidgetHost {
        let mut host = WidgetHost::new();
        host.editor_state.editor_ui.share = share;
        host
    }

    fn open_host() -> WidgetHost {
        let mut share = ShareUiState::default();
        share.open_with(Some("http://host/f/key".to_string()));
        // The rights the host fills in before the chip opens the dialog: the
        // owner of the document. Without them the dialog is the fail-closed
        // default and every invite press is a refusal — which is what
        // `a_refusal_is_recorded_when_the_rights_are_not_known` asserts.
        share.own_rights = op_editor_core::Rights::EDITOR;
        share.rights_known = true;
        share.is_owner = true;
        host_with_share(share)
    }

    /// The centre of a rect the open dialog's own layout produced.
    fn centre(rect: op_editor_ui::Rect) -> Point2D {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    }

    fn layout_of(
        host: &WidgetHost,
    ) -> op_editor_ui::widgets::share_dialog_layout::ShareDialogLayout {
        ShareDialog::for_editor(&host.editor_state, VIEWPORT_W, VIEWPORT_H)
            .expect("the dialog is open")
            .layout()
    }

    #[test]
    fn a_closed_dialog_does_not_take_the_press() {
        let mut host = host_with_share(ShareUiState::default());

        assert!(!host.dispatch_share_press(600.0, 400.0, VIEWPORT_W, VIEWPORT_H));
    }

    #[test]
    fn the_close_control_closes_the_dialog() {
        let mut host = open_host();
        let close = centre(layout_of(&host).close);

        assert!(host.dispatch_share_press(close.x, close.y, VIEWPORT_W, VIEWPORT_H));

        assert!(!host.editor_state.editor_ui.share.open);
    }

    #[test]
    fn a_press_on_the_scrim_is_consumed_without_closing_the_dialog() {
        let mut host = open_host();
        // A corner is outside the centred card, so the hit is the scrim.
        let point = Point2D::new(4.0, 4.0);
        assert_eq!(
            ShareDialog::for_editor(&host.editor_state, VIEWPORT_W, VIEWPORT_H)
                .expect("open")
                .hit_test(point),
            op_editor_ui::widgets::share_dialog::ShareDialogHit::Outside
        );

        assert!(host.dispatch_share_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H));

        assert!(
            host.editor_state.editor_ui.share.open,
            "an accidental click beside the card must not discard a half-typed invite"
        );
    }

    #[test]
    fn an_invite_press_queues_the_action_for_the_host_to_drain() {
        let mut host = open_host();
        host.editor_state
            .editor_ui
            .share
            .invite_input
            .set_text("userB");
        let invite = centre(layout_of(&host).invite_button);

        assert!(host.dispatch_share_press(invite.x, invite.y, VIEWPORT_W, VIEWPORT_H));

        assert_eq!(
            host.editor_state.editor_ui.share.pending,
            vec![op_editor_core::editor_ui_state::share::ShareAction::Grant {
                account: "userB".to_string(),
                level: op_editor_core::ShareLevel::DEFAULT,
            }]
        );
    }

    #[test]
    fn hover_reports_the_row_under_the_pointer_and_only_when_it_moves() {
        let mut host = open_host();
        let copy_link = centre(layout_of(&host).copy_link);

        assert!(host.update_share_hover(copy_link.x, copy_link.y, VIEWPORT_W, VIEWPORT_H));
        assert_eq!(
            host.editor_state.editor_ui.share.hover,
            Some(ShareRow::CopyLink)
        );

        // The same point again is not a change, so it costs no frame.
        assert!(!host.update_share_hover(copy_link.x, copy_link.y, VIEWPORT_W, VIEWPORT_H));

        assert!(host.update_share_hover(4.0, 4.0, VIEWPORT_W, VIEWPORT_H));
        assert_eq!(host.editor_state.editor_ui.share.hover, None);
    }

    #[test]
    fn a_closed_dialog_owns_no_hover() {
        let mut host = host_with_share(ShareUiState::default());

        assert!(!host.update_share_hover(600.0, 400.0, VIEWPORT_W, VIEWPORT_H));
        assert_eq!(host.editor_state.editor_ui.share.hover, None);
    }

    #[test]
    fn the_footer_row_closes_the_dialog_and_asks_for_the_live_session() {
        // The chip was the collaboration panel's only entrance before this
        // dialog replaced it, so the footer row is what keeps it reachable.
        let mut host = open_host();
        let footer = centre(layout_of(&host).footer);

        assert!(host.dispatch_share_press(footer.x, footer.y, VIEWPORT_W, VIEWPORT_H));

        assert!(!host.editor_state.editor_ui.share.open);
        assert_eq!(
            host.editor_state.editor_ui.share.pending,
            vec![op_editor_core::editor_ui_state::share::ShareAction::OpenSession]
        );
    }

    #[test]
    fn a_refusal_is_recorded_when_the_rights_are_not_known() {
        // The fail-closed default: the host has not told the dialog what the
        // caller may do, so the press is answered with a sentence rather than a
        // request the server would refuse.
        let mut share = ShareUiState::default();
        share.open_with(Some("http://host/f/key".to_string()));
        let mut host = host_with_share(share);
        host.editor_state
            .editor_ui
            .share
            .invite_input
            .set_text("userB");
        let invite = centre(layout_of(&host).invite_button);

        assert!(host.dispatch_share_press(invite.x, invite.y, VIEWPORT_W, VIEWPORT_H));

        assert!(host.editor_state.editor_ui.share.pending.is_empty());
        assert_eq!(
            host.editor_state.editor_ui.share.notice,
            Some(
                op_editor_core::editor_ui_state::share::ShareNotice::Refused(
                    op_editor_core::ShareInviteRefusal::NoInviteRight
                )
            )
        );
    }
}
