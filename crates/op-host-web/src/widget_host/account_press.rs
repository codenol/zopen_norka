//! Account press dispatchers (web): the entry form and the signed-in dropdown.
//!
//! The hit-test + state walk lives in `op_editor_ui` — `account_entry_form` for
//! the geometry, `account_press_flow` for the dropdown — and this file keeps
//! only the web platform arms. The browser cannot check a password itself (the
//! session is an HttpOnly cookie the daemon sets), so a press either puts the
//! caret in a field or queues what the form collected; `web_auth_sync`, which
//! owns the `/api/auth/*` calls, sends it from the drain that runs after the
//! press instead of from inside the frame that handled it.

use super::{PendingCredentialRequest, PendingSessionAction, WidgetHost};
use op_editor_core::AccountField;
use op_editor_ui::widgets::account_entry_form::{AccountEntryForm, AccountEntryHit};
use op_editor_ui::widgets::account_press_flow::{self as account_flow, AccountMenuPress};
use op_editor_ui::Point2D;

impl WidgetHost {
    /// The account entry surface, when the daemon's status answer says one is
    /// due. `None` for a local deployment, a signed-in tab, and the frames
    /// before the first answer — see `EditorUiState::account_entry_mode`.
    pub(in crate::widget_host) fn account_entry_form(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<AccountEntryForm<'_>> {
        AccountEntryForm::for_editor(&self.editor_state, viewport_w, viewport_h)
    }

    /// Account-entry press dispatcher: caret into a field, or submit.
    ///
    /// Returns whether the press was consumed. A press anywhere while the form
    /// is up is consumed — including the scrim beside it — because the editor
    /// behind it has no session to work with and must not receive clicks that
    /// look like they landed on a canvas.
    pub(in crate::widget_host) fn dispatch_account_entry_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        // The hit is taken before the flags are cleared: the form borrows the
        // editor state it was built from.
        let hit = match self.account_entry_form(viewport_w, viewport_h) {
            Some(form) => form.hit_test(Point2D::new(x, y)),
            None => return false,
        };
        self.clear_legacy_login_modal();
        match hit {
            AccountEntryHit::Field(field) => {
                self.editor_state.editor_ui.account_entry.focus_field(field);
                self.blur_text_inputs_on_blank_press();
            }
            AccountEntryHit::Submit => self.submit_account_entry(),
            // A press on the card, or on the scrim beside it, drops the caret:
            // the field a visitor clicked away from is no longer the one they
            // are typing into.
            AccountEntryHit::Inside | AccountEntryHit::Outside => {
                self.editor_state.editor_ui.account_entry.focus = None;
                self.blur_text_inputs_on_blank_press();
            }
        }
        self.mark_dirty();
        true
    }

    /// Validate the on-screen form and send it.
    ///
    /// The form owns the validation that does not need a round trip (an empty
    /// field, two passwords that differ); a refusal is painted on the same
    /// surface. What leaves this function is a request that owns the secret —
    /// the password field is already empty (`AccountEntryState::begin_sign_in`).
    pub(in crate::widget_host) fn submit_account_entry(&mut self) {
        let mode = self.editor_state.editor_ui.account_entry_mode();
        match mode {
            op_editor_core::AccountEntryMode::Invite => {
                let outcome = self
                    .editor_state
                    .editor_ui
                    .account_entry
                    .begin_invite_acceptance();
                match outcome {
                    Ok(acceptance) => {
                        self.pending_credential_request =
                            Some(PendingCredentialRequest::Invite(acceptance));
                    }
                    Err(error) => self.editor_state.editor_ui.account_entry.fail(error),
                }
            }
            op_editor_core::AccountEntryMode::SignIn => {
                let outcome = self.editor_state.editor_ui.account_entry.begin_sign_in();
                match outcome {
                    Ok(request) => {
                        self.pending_credential_request =
                            Some(PendingCredentialRequest::SignIn(request));
                    }
                    Err(error) => self.editor_state.editor_ui.account_entry.fail(error),
                }
            }
            // Nothing to submit: the explanation has no form, and a hidden
            // surface has no press to reach this.
            op_editor_core::AccountEntryMode::Unprovisioned
            | op_editor_core::AccountEntryMode::Hidden => {}
        }
        self.mark_dirty();
    }

    /// Signed-in account-dropdown press dispatcher.
    pub(in crate::widget_host) fn dispatch_account_menu_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_w: f32,
        _viewport_h: f32,
    ) {
        match account_flow::press_account_menu(
            &mut self.editor_state,
            x,
            y,
            viewport_w,
            self.now_ms,
        ) {
            AccountMenuPress::Vanished => return,
            AccountMenuPress::OpenMcpTokens => {
                // The hub serves this editor at its own origin, so the portal's
                // per-account MCP-token page lives at `/mcp-tokens` relative to
                // the current origin. A new tab opened synchronously inside the
                // click's user-activation window is not popup-blocked.
                crate::web_mcp_tokens::open_mcp_tokens_page();
            }
            AccountMenuPress::SignOut => {
                // The display state is already anonymous (the shared flow did
                // that); what is left is the daemon's session and whatever the
                // form was holding for it.
                crate::web_auth_sync::clear_entry_state(
                    &mut self.editor_state.editor_ui.account_entry,
                );
                self.pending_session_actions
                    .push(PendingSessionAction::SignOut);
            }
            AccountMenuPress::Dismissed => {
                self.blur_text_inputs_on_blank_press();
            }
            AccountMenuPress::Handled | AccountMenuPress::Ignored => {}
        }
        self.mark_dirty();
    }

    /// Drop the native host's browser-popup sign-in flags.
    ///
    /// The web host has no popup modal and no device-login pairing, so the two
    /// fields the shared chrome still sets (`login_modal_open` from the
    /// collaboration panel and the settings modal's Account tab, and a status
    /// note left by a desktop flow) must never survive a frame here: a flag that
    /// nothing paints is a flag that eventually paints something.
    pub(in crate::widget_host) fn clear_legacy_login_modal(&mut self) {
        let ui = &mut self.editor_state.editor_ui;
        ui.login_modal_open = false;
        ui.login_modal_hover = None;
        ui.login_modal_status = None;
        ui.login_modal_stub_hint_shown = false;
    }

    /// Turn a requested sign-in surface into the caret in the entry form.
    ///
    /// The shared collaboration-panel flow (and the native host with it) opens
    /// the browser-popup sign-in modal by setting `login_modal_open`. The web
    /// host has no such modal any more: a password form is painted for the
    /// whole time the deployment is sign-in-capable and the tab is signed out,
    /// so the request is answered by pointing at the field that was asked for.
    /// When no form is on screen (a local deployment, or already signed in) the
    /// flag is cleared rather than left set, so nothing can paint a stale modal
    /// on a later frame.
    pub(in crate::widget_host) fn absorb_sign_in_request_into_entry_form(&mut self) {
        if !self.editor_state.editor_ui.login_modal_open
            && self.editor_state.editor_ui.login_modal_status.is_none()
        {
            return;
        }
        let focused = self.focus_account_entry_field(AccountField::Username);
        self.clear_legacy_login_modal();
        if !focused {
            self.mark_dirty();
        }
    }

    /// Put the caret in a field of the entry form, if one is on screen.
    ///
    /// Used by the chrome entry points that used to open the browser-popup
    /// sign-in modal: in an online deployment the form is already up, so the
    /// honest answer to "sign in" is to point at it rather than to open a
    /// second, older surface on top of it.
    pub(in crate::widget_host) fn focus_account_entry_field(
        &mut self,
        field: AccountField,
    ) -> bool {
        if self.editor_state.editor_ui.account_entry_mode()
            == op_editor_core::AccountEntryMode::Hidden
        {
            return false;
        }
        self.editor_state.editor_ui.account_entry.focus_field(field);
        self.mark_dirty();
        true
    }
}
