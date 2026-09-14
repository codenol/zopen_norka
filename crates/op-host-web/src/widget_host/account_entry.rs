//! The account entry form's keyboard (web).
//!
//! The form is a gate, not a dialog somebody can type behind: while it is on
//! screen every keystroke belongs to it. That is why the three entry points
//! below are checked at the TOP of `apply_text` / `apply_backspace` /
//! `apply_send` rather than among the other text inputs — a character that fell
//! through would reach the canvas shortcuts and switch tools behind the form.
//!
//! Nothing here talks to the daemon: a submit only queues the credentials
//! (`WidgetHost::submit_account_entry`), and the mount's post-keypress drain
//! sends them.

use super::WidgetHost;
use op_editor_core::AccountEntryMode;

impl WidgetHost {
    /// Whether the entry form currently owns the keyboard.
    ///
    /// Every body that is on screen does, including the unprovisioned
    /// explanation: it paints the same full-viewport scrim, so a bare letter
    /// that reached the editor would switch the tool of an editor the visitor
    /// cannot see or use. The explanation has no field to type into, so the
    /// keystroke is consumed and nothing happens — which is what a scrim means.
    pub(in crate::widget_host) fn account_entry_takes_keyboard(&self) -> bool {
        !matches!(
            self.editor_state.editor_ui.account_entry_mode(),
            AccountEntryMode::Hidden
        )
    }

    /// Type into the focused entry field. Returns whether the key was consumed.
    pub(in crate::widget_host) fn apply_account_entry_text(&mut self, c: char) -> bool {
        if !self.account_entry_takes_keyboard() {
            return false;
        }
        // A keypress with no field focused is still consumed: it must not reach
        // the editor behind the form. Enter is the exception the caller handles
        // (`apply_account_entry_send`), which is why this returns true either
        // way rather than reporting whether the field changed.
        if self.editor_state.editor_ui.account_entry.focus.is_none() {
            return true;
        }
        if self.editor_state.editor_ui.account_entry.push_char(c) {
            self.mark_dirty();
        }
        true
    }

    /// Delete into the focused entry field. Returns whether the key was consumed.
    pub(in crate::widget_host) fn apply_account_entry_backspace(&mut self) -> bool {
        if !self.account_entry_takes_keyboard() {
            return false;
        }
        if self.editor_state.editor_ui.account_entry.backspace() {
            self.mark_dirty();
        }
        true
    }

    /// Enter in the entry form walks it, then submits it. Returns whether the
    /// key was consumed.
    ///
    /// Enter moves the caret on instead of sending a half-filled form, which is
    /// also what makes the form usable at all for somebody driving it from the
    /// keyboard: the fields are separate rows with nothing to Tab between them.
    /// On the last field — and with no field focused at all — the form is what
    /// was meant.
    pub(in crate::widget_host) fn apply_account_entry_send(&mut self) -> bool {
        if !self.account_entry_takes_keyboard() {
            return false;
        }
        let entry = &self.editor_state.editor_ui.account_entry;
        if entry.error.is_some() || entry.submitting {
            // A refusal is on screen, or a round trip is already in flight:
            // Enter belongs to the message, not to a second submission.
            self.mark_dirty();
            return true;
        }
        let mode = self.editor_state.editor_ui.account_entry_mode();
        // Walking starts at the first field: Enter before anything has been
        // clicked is how somebody driving the form from the keyboard begins.
        let next = match entry.focus {
            Some(field) => next_field(mode, field),
            None => fields_for(mode).first().copied(),
        };
        match next {
            Some(next) => {
                self.editor_state.editor_ui.account_entry.focus_field(next);
                self.mark_dirty();
            }
            // Past the last field there is nothing left to walk: this is a
            // submit.
            None => self.submit_account_entry(),
        }
        true
    }
}

/// The fields one body collects, top to bottom.
fn fields_for(mode: AccountEntryMode) -> &'static [op_editor_core::AccountField] {
    op_editor_ui::widgets::account_entry_form::fields_for(mode)
}

/// The field after `field`, or `None` when it was the last one.
fn next_field(
    mode: AccountEntryMode,
    field: op_editor_core::AccountField,
) -> Option<op_editor_core::AccountField> {
    let fields = fields_for(mode);
    let index = fields.iter().position(|candidate| *candidate == field)?;
    fields.get(index + 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::AccountField;

    fn host_showing_the_form() -> WidgetHost {
        let mut host = WidgetHost::new();
        let ui = &mut host.editor_state.editor_ui;
        ui.account_ui_available = true;
        ui.account_entry.status_received = true;
        host
    }

    #[test]
    fn the_form_is_a_text_input_the_host_knows_about() {
        // Issue #87, found by signing in to the deployed build: the form had
        // its own idea of owning the keyboard (`account_entry_takes_keyboard`)
        // and the host's single rule did not know about it — so `r`, `t`, `v`,
        // `p`, `y`, `o`, `l` and `h` switched canvas tools instead of reaching
        // the field. A correct password containing any of them was refused with
        // no way for the person to see why.
        let host = host_showing_the_form();
        assert!(
            host.account_entry_takes_keyboard(),
            "the form says it takes the keyboard"
        );
        assert!(
            host.input_active(),
            "and the host's one rule agrees — otherwise the shortcuts win"
        );

        let mut local = WidgetHost::new();
        local.editor_state.editor_ui.account_ui_available = false;
        local.editor_state.editor_ui.account_entry.status_received = true;
        assert!(
            !local.input_active(),
            "a local deployment shows no form and keeps its shortcuts"
        );
    }

    #[test]
    fn typing_goes_into_the_focused_field_and_nowhere_else() {
        let mut host = host_showing_the_form();
        host.editor_state.editor_ui.account_entry.focus = Some(AccountField::Username);

        assert!(host.apply_account_entry_text('k'));
        assert_eq!(host.editor_state.editor_ui.account_entry.username, "k");
        assert!(
            host.editor_state.selection.is_empty(),
            "no tool was switched"
        );
    }

    #[test]
    fn a_keystroke_with_no_field_focused_never_reaches_the_canvas() {
        let mut host = host_showing_the_form();
        host.editor_state.editor_ui.account_entry.focus = None;
        let tool_before = host.editor_state.tool;

        assert!(
            host.apply_account_entry_text('r'),
            "the form is on screen, so it owns the key"
        );
        assert_eq!(host.editor_state.tool, tool_before);
        assert_eq!(host.editor_state.editor_ui.account_entry.username, "");
    }

    #[test]
    fn the_form_takes_the_keyboard_for_every_body_it_shows() {
        let mut host = WidgetHost::new();
        assert!(!host.account_entry_takes_keyboard(), "no status answer yet");

        host.editor_state.editor_ui.account_ui_available = true;
        host.editor_state.editor_ui.account_entry.status_received = true;
        assert!(host.account_entry_takes_keyboard());

        // The explanation paints the same scrim, so it consumes keystrokes too
        // — there is simply no field for them to land in.
        host.editor_state.editor_ui.account_entry.needs_first_admin = true;
        assert!(host.account_entry_takes_keyboard());
        let tool_before = host.editor_state.tool;
        assert!(host.apply_account_entry_text('r'));
        assert_eq!(host.editor_state.tool, tool_before);

        // A local deployment shows nothing at all and keeps its shortcuts.
        host.editor_state.editor_ui.account_entry.needs_first_admin = false;
        host.editor_state.editor_ui.account_ui_available = false;
        assert!(!host.account_entry_takes_keyboard());
    }

    #[test]
    fn enter_walks_the_form_before_it_submits_it() {
        let mut host = host_showing_the_form();
        host.editor_state.editor_ui.account_entry.focus = Some(AccountField::Username);
        host.editor_state.editor_ui.account_entry.username = "kay".to_string();

        assert!(host.apply_account_entry_send());
        assert_eq!(
            host.editor_state.editor_ui.account_entry.focus,
            Some(AccountField::Password),
            "the caret moves on instead of sending a half-filled form"
        );
        assert!(host.pending_credential_request.is_none());
    }

    #[test]
    fn enter_on_the_last_field_submits_and_the_password_leaves_the_state() {
        let mut host = host_showing_the_form();
        let entry = &mut host.editor_state.editor_ui.account_entry;
        entry.username = "kay".to_string();
        entry.password = "karta-mosta-42".to_string();
        entry.focus = Some(AccountField::Password);

        assert!(host.apply_account_entry_send());

        assert!(
            host.editor_state
                .editor_ui
                .account_entry
                .password
                .is_empty(),
            "the secret is out of editor state before the request is built"
        );
        assert!(host.pending_credential_request.is_some());
    }

    #[test]
    fn enter_while_a_refusal_is_on_screen_does_not_resend() {
        let mut host = host_showing_the_form();
        let entry = &mut host.editor_state.editor_ui.account_entry;
        entry.username = "kay".to_string();
        entry.password = "karta-mosta-42".to_string();
        entry.focus = Some(AccountField::Password);
        entry.error = Some(op_editor_core::AccountEntryError::Rejected);

        assert!(host.apply_account_entry_send());
        assert!(host.pending_credential_request.is_none());
        assert!(!host
            .editor_state
            .editor_ui
            .account_entry
            .password
            .is_empty());
    }

    #[test]
    fn a_refused_attempt_leaves_no_secret_on_the_host_either() {
        // After a refusal the host must not be holding the credentials any
        // more: they were taken by the request that was sent.
        let mut host = host_showing_the_form();
        let entry = &mut host.editor_state.editor_ui.account_entry;
        entry.username = "kay".to_string();
        entry.password = "karta-mosta-42".to_string();
        entry.focus = Some(AccountField::Password);
        assert!(host.apply_account_entry_send());

        let taken = host.take_pending_credential_request();
        assert!(taken.is_some());
        assert!(host.pending_credential_request.is_none());
        // Nothing is left in the state to leak into a snapshot.
        let rendered = format!("{:?}", host.editor_state.editor_ui.account_entry);
        assert!(!rendered.contains("karta-mosta-42"), "{rendered}");
    }

    #[test]
    fn the_invitation_form_walks_all_four_fields_before_it_submits() {
        let mut host = host_showing_the_form();
        host.editor_state.editor_ui.account_entry.invite_token = Some("tok".to_string());
        let mut seen = Vec::new();
        for _ in 0..4 {
            assert!(host.apply_account_entry_send());
            seen.push(host.editor_state.editor_ui.account_entry.focus);
        }
        assert_eq!(
            seen,
            vec![
                Some(AccountField::InviteUsername),
                Some(AccountField::InvitePassword),
                Some(AccountField::InvitePasswordConfirm),
                Some(AccountField::InviteDisplayName),
            ]
        );
        assert!(
            host.pending_credential_request.is_none(),
            "an untouched form is never sent"
        );

        // One more, from the last field, is a submit — and an empty form is
        // refused in place rather than sent.
        assert!(host.apply_account_entry_send());
        assert!(host.pending_credential_request.is_none());
        assert_eq!(
            host.editor_state.editor_ui.account_entry.error,
            Some(op_editor_core::AccountEntryError::EmptyFields)
        );
    }

    #[test]
    fn the_form_does_not_steal_the_caret_when_it_appears() {
        // A visitor who only wanted to read it is not silently typing into it.
        let host = host_showing_the_form();
        assert_eq!(host.editor_state.editor_ui.account_entry.focus, None);
    }
}
