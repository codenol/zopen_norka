//! Web keyboard arms for the comment composer.
//!
//! The comment field is the editor's own input — there is no DOM element behind
//! it — so a typed character reaches it only if the host routes it here. The
//! rule is the one every other in-canvas field follows: while
//! `comments.composer_focused` is set the composer owns the keystroke, and it is
//! checked before the canvas shortcuts, because otherwise a bare `r` would
//! switch the tool while somebody is typing a review comment.
//!
//! Nothing here re-implements the draft: the text goes into the state's own
//! draft and the widget paints it, exactly as the popover reads it.

use super::WidgetHost;

impl WidgetHost {
    /// Whether the comment composer owns the keyboard right now.
    ///
    /// The state answers it (see `CommentsUiState::takes_keyboard`) so this arm
    /// and the host's `input_active` rule — which decides whether the hidden IME
    /// capture input is focused at all — can never disagree about it.
    fn comment_composer_takes_text(&self) -> bool {
        self.editor_state.editor_ui.comments.takes_keyboard()
    }

    /// A character typed into the comment field.
    ///
    /// Returns `true` when the composer took it. Control characters are refused
    /// rather than stored: `\n` and `\t` inside a single-line field would be
    /// invisible and would ride along in the comment body the server stores.
    pub(in crate::widget_host) fn comment_text(&mut self, c: char) -> bool {
        if !self.comment_composer_takes_text() {
            return false;
        }
        if !c.is_control() {
            let draft = self.editor_state.editor_ui.comments.draft_mut();
            // The same ceiling the send path enforces, applied at the keystroke:
            // stopping at the limit is kinder than letting the reviewer type a
            // paragraph the server will refuse.
            if draft.chars().count() < op_editor_core::editor_ui_state::MAX_COMMENT_CHARS {
                draft.push(c);
            }
        }
        self.mark_dirty();
        true
    }

    /// Backspace inside the comment field.
    pub(in crate::widget_host) fn comment_backspace(&mut self) -> bool {
        if !self.comment_composer_takes_text() {
            return false;
        }
        self.editor_state.editor_ui.comments.draft_mut().pop();
        self.mark_dirty();
        true
    }

    /// Enter inside the comment field: send it.
    ///
    /// Returns whether the composer consumed the key — always `true` while it
    /// owns the keyboard, because a half-typed comment must not also be an
    /// Enter for whatever is underneath.
    pub(in crate::widget_host) fn comment_send(&mut self) -> bool {
        if !self.comment_composer_takes_text() {
            return false;
        }
        // An answer-less draft is a no-op here; the button says why (it is
        // disabled), and the request is queued by the frame tick either way.
        self.editor_state.editor_ui.comments.send();
        self.mark_dirty();
        true
    }
}
