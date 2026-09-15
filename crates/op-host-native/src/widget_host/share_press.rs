//! Native arms for the Share dialog (#56).
//!
//! The dialog itself — vocabulary, layout, hit-test, paint, and what a press
//! does — is shared with the browser host (`op_editor_ui::widgets::share_dialog`
//! and its `share_dialog_flow` sibling). What is left for a host is the tail:
//! resolve a point to a hit, resolve a level option through the widget that owns
//! the popover rows, and answer the requests the press queued.
//!
//! ## Why the desktop host answers a request it cannot send
//!
//! Sharing is a daemon-side relationship: `/api/share/*` resolves the CALLER's
//! account and edits the access list of the document the caller owns, and the
//! desktop binary has no such route to call — the local daemon has one document
//! and nobody to share it with, and the online deployment is a different build
//! pointing at a different store. The dialog therefore keeps its questions and
//! is answered with the one honest sentence this host has: the request could
//! not be completed here. Leaving the action in the queue instead would spin
//! the button and call it busy forever, which is worse than a refusal — see
//! issue #107 for the desktop half of sharing.

use super::WidgetHostNative;
use op_editor_core::editor_ui_state::share::{ShareAction, ShareNotice, ShareUiState};
use op_editor_core::ShareInviteRefusal;
use op_editor_ui::widgets::share_dialog::{apply_share_hit, ShareDialog, ShareDialogHit};
use op_editor_ui::Point2D;

/// The code carried by a refusal this host produces for work it cannot send.
const NO_DAEMON_CODE: &str = "online-deployment-required";

impl WidgetHostNative {
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
        let level = match hit {
            ShareDialogHit::LevelOption(index) => dialog.level_option(index),
            _ => None,
        };
        let consumed = apply_share_hit(&mut self.editor_state.editor_ui.share, hit, level);
        self.drain_share_actions();
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

    /// Answer everything the dialog queued. Runs after a press rather than on a
    /// tick: none of these needs a socket, and a desktop host has no frame
    /// cadence of its own to borrow.
    pub(in crate::widget_host) fn drain_share_actions(&mut self) -> bool {
        let queue = std::mem::take(&mut self.editor_state.editor_ui.share.pending);
        if queue.is_empty() {
            return false;
        }
        for action in queue {
            match action {
                ShareAction::CopyLink => {
                    let link = self.editor_state.editor_ui.share.link.clone();
                    match link {
                        Some(link) if !link.is_empty() => {
                            // The runner drains `chat.pending_copy_text` into the
                            // OS clipboard — the same road every other copy in
                            // the chrome takes.
                            self.editor_state.chat.queue_copy_text(link);
                            self.editor_state.editor_ui.share.notice =
                                Some(ShareNotice::CopiedLink);
                        }
                        // Nothing to copy, so nothing is claimed about the
                        // clipboard — the dialog says what is missing instead.
                        _ => {
                            self.editor_state.editor_ui.share.notice =
                                Some(ShareNotice::NoDocumentLink);
                            self.editor_state.editor_ui.share.busy = false;
                        }
                    }
                }
                // An invitation link is only ever produced by a daemon, so this
                // host has none to copy. The code is carried rather than
                // guessed at.
                ShareAction::CopyInviteLink { .. }
                | ShareAction::LoadList
                | ShareAction::Grant { .. }
                | ShareAction::Revoke { .. }
                | ShareAction::SetLinkAccess { .. }
                | ShareAction::IssueInvitation { .. } => {
                    refuse_for_this_host(&mut self.editor_state.editor_ui.share);
                }
                ShareAction::OpenSession => {
                    self.editor_state.editor_ui.share.busy = false;
                    self.editor_state.editor_ui.collab.panel.open = true;
                }
            }
        }
        true
    }
}

/// Record the one refusal this host can give: there is no daemon to ask.
fn refuse_for_this_host(share: &mut ShareUiState) {
    share.busy = false;
    share.notice = Some(ShareNotice::Refused(ShareInviteRefusal::RefusedByServer {
        code: NO_DAEMON_CODE.to_string(),
    }));
}

#[cfg(test)]
mod tests {
    use super::super::WidgetHostNative;
    use op_editor_core::editor_ui_state::share::{ShareAction, ShareNotice, ShareUiState};
    use op_editor_core::{Rights, ShareInviteRefusal};
    use op_editor_ui::widgets::share_dialog::ShareDialog;
    use op_editor_ui::Point2D;

    const VIEWPORT_W: f32 = 1200.0;
    const VIEWPORT_H: f32 = 800.0;

    fn open_host() -> WidgetHostNative {
        let mut host = WidgetHostNative::new();
        let share = &mut host.editor_state.editor_ui.share;
        share.open_with(Some("https://example.test/f/key".to_string()));
        share.own_rights = Rights::EDITOR;
        share.rights_known = true;
        share.is_owner = true;
        host
    }

    fn layout_of(
        host: &WidgetHostNative,
    ) -> op_editor_ui::widgets::share_dialog_layout::ShareDialogLayout {
        ShareDialog::for_editor(&host.editor_state, VIEWPORT_W, VIEWPORT_H)
            .expect("the dialog is open")
            .layout()
    }

    fn centre(rect: op_editor_ui::Rect) -> Point2D {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    }

    #[test]
    fn a_closed_dialog_does_not_take_the_press() {
        let mut host = WidgetHostNative::new();

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
    fn copy_link_queues_the_document_link_for_the_runner() {
        let mut host = open_host();
        let copy = centre(layout_of(&host).copy_link);

        assert!(host.dispatch_share_press(copy.x, copy.y, VIEWPORT_W, VIEWPORT_H));

        assert_eq!(
            host.editor_state.chat.pending_copy_text.as_deref(),
            Some("https://example.test/f/key")
        );
        assert_eq!(
            host.editor_state.editor_ui.share.notice,
            Some(ShareNotice::CopiedLink)
        );
    }

    #[test]
    fn a_request_this_host_cannot_send_is_refused_rather_than_left_queued() {
        let mut host = open_host();
        host.editor_state.editor_ui.share.pending = vec![
            ShareAction::LoadList,
            ShareAction::Revoke {
                account: "userB".to_string(),
            },
        ];

        assert!(host.drain_share_actions());

        assert!(
            host.editor_state.editor_ui.share.pending.is_empty(),
            "a request left in the queue would keep the dialog busy forever"
        );
        assert_eq!(
            host.editor_state.editor_ui.share.notice,
            Some(ShareNotice::Refused(ShareInviteRefusal::RefusedByServer {
                code: "online-deployment-required".to_string()
            }))
        );
        assert!(!host.editor_state.editor_ui.share.busy);
    }

    #[test]
    fn the_footer_row_opens_the_live_session_panel() {
        let mut host = open_host();
        host.editor_state.editor_ui.share.pending = vec![ShareAction::OpenSession];

        assert!(host.drain_share_actions());

        assert!(host.editor_state.editor_ui.collab.panel.open);
    }

    #[test]
    fn hover_reports_the_row_under_the_pointer_and_only_when_it_moves() {
        let mut host = open_host();
        let copy = centre(layout_of(&host).copy_link);

        assert!(host.update_share_hover(copy.x, copy.y, VIEWPORT_W, VIEWPORT_H));
        assert_eq!(
            host.editor_state.editor_ui.share.hover,
            Some(op_editor_core::editor_ui_state::share::ShareRow::CopyLink)
        );
        assert!(!host.update_share_hover(copy.x, copy.y, VIEWPORT_W, VIEWPORT_H));
    }

    #[test]
    fn a_scrim_press_is_consumed_without_closing() {
        let mut host = open_host();

        assert!(host.dispatch_share_press(4.0, 4.0, VIEWPORT_W, VIEWPORT_H));

        assert!(host.editor_state.editor_ui.share.open);
        assert_eq!(host.editor_state.editor_ui.share.hover, None);
    }

    /// The state a host hands the dialog before it opens: nothing is claimed
    /// about rights until the host has said what they are.
    #[test]
    fn the_default_state_is_fail_closed() {
        let share = ShareUiState::default();

        assert!(!share.rights_known);
        assert!(!share.can_invite());
        assert!(share.own_level().is_none());
    }
}
