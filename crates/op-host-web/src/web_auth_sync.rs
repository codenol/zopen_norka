//! The browser's account session: who this tab is, and how it becomes somebody.
//!
//! The wasm bundle ships no identity code. It asks the daemon
//! (`GET /api/auth/status`) who the request belongs to, shows a password form
//! when the answer is "nobody, but this deployment signs people in", and sends
//! the credentials to `POST /api/auth/login` or `POST /api/auth/invite/accept`.
//! A session lives in an HttpOnly cookie the daemon sets, so nothing in this
//! module ever holds a token — and nothing here holds a password either after
//! the request that carries it has been built
//! (`op_editor_core::account_entry_state`).
//!
//! ## Why this is a poll, not a socket
//!
//! A session can end somewhere this tab cannot see: a sign-out from the desktop
//! app, a revocation by an operator, or an expiry. The interval tick re-reads
//! the status every ~30 s so the tab stops pretending to be signed in; between
//! those reads, a sign-in or sign-out this tab performed re-reads immediately.
//!
//! ## What the daemon decides, and what this module decides
//!
//! Every credential check belongs to the daemon's account store. This module
//! decides only what the ANSWER means for the shell: `200` with the caller's own
//! identity projection becomes `AccountState`, and anything else becomes a typed
//! [`AccountEntryError`] the form paints. It deliberately never guesses which
//! half of a credential pair was wrong — the daemon refuses to say, and a client
//! that inferred it would rebuild the enumeration oracle the store refuses to be.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::account_entry_state::{AccountEntryState, InviteAcceptance, SignInRequest};
use op_editor_core::{auth_routes, AccountEntryError, AccountState};

use crate::live_sync;
use crate::repaint_ctx::RepaintContext;
use crate::widget_host::PendingSessionAction;

/// Poll cadence: each tick drains queued session actions, and every
/// [`STATUS_REFRESH_TICKS`] of them re-reads `/api/auth/status`.
const AUTH_POLL_INTERVAL_MS: i32 = 600;
/// Steady-state session health check — every N ticks (~30 s) re-fetch
/// `/api/auth/status` so a session revoked or expired daemon-side (or a
/// sign-in/out from the desktop GUI or another tab) reaches this shell without
/// a reload. Network failures change nothing (the callback simply never fires),
/// so an offline blip cannot sign the user out.
const STATUS_REFRESH_TICKS: u32 = 50;

thread_local! {
    /// Orders `/api/auth/status` requests. Only the newest request may project
    /// its response into editor state, so a slow pre-sign-in request cannot
    /// hide the account UI after a newer authenticated request has succeeded.
    static STATUS_REQUEST_GATE: StatusRequestGate = const { StatusRequestGate::new() };
}

/// Latest-request-wins gate for the account status projection.
///
/// XHR callbacks can complete out of order. The sequence is assigned before a
/// request starts, and every callback must pass both this ordering check and
/// the HTTP success check before it may touch identity or UI state.
struct StatusRequestGate {
    latest_started: Cell<u64>,
}

impl StatusRequestGate {
    const fn new() -> Self {
        Self {
            latest_started: Cell::new(0),
        }
    }

    fn begin(&self) -> u64 {
        let request_id = self
            .latest_started
            .get()
            .checked_add(1)
            .expect("auth status request sequence exhausted");
        self.latest_started.set(request_id);
        request_id
    }

    fn should_apply(&self, request_id: u64, http_status: u16) -> bool {
        http_status == 200 && self.latest_started.get() == request_id
    }
}

/// Wire the account session onto the mounted shell. Called once from mount;
/// the interval runs for the page lifetime.
pub(crate) fn start<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    read_invite_route(inner);
    refresh_status(inner);

    let ticks = Rc::new(Cell::new(0u32));
    let inner = inner.clone();
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        drain_pending_credentials(&inner);
        drain_session_actions(&inner, &base);
        let count = ticks.get() + 1;
        if count >= STATUS_REFRESH_TICKS {
            ticks.set(0);
            fetch_status(&inner, &base);
        } else {
            ticks.set(count);
        }
    });
    let _ = live_sync::start_interval(AUTH_POLL_INTERVAL_MS, tick);
}

/// Read an invitation out of the address, once, at mount.
///
/// Once: the link is a page, not a mode, and re-reading it every frame would
/// fight the form's own state (a submit clears the token, and a re-read would
/// put it straight back).
fn read_invite_route<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let token = web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .and_then(|path| op_editor_core::route::invite_token(&path).map(str::to_string));
    let Some(token) = token else {
        return;
    };
    let Ok(mut borrowed) = inner.try_borrow_mut() else {
        return;
    };
    borrowed
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .set_invite_token(Some(token));
}

/// The tab is showing an invitation, so the address belongs to it.
///
/// The router must not rewrite `/invite/<token>` into the editor's own address
/// before the invitation has been accepted: the link would be gone from the
/// address bar (and from a refresh) while the form that needs it is still on
/// screen.
pub(crate) fn invitation_address_active(host: &crate::widget_host::WidgetHost) -> bool {
    host.editor_state()
        .editor_ui
        .account_entry
        .invite_token
        .is_some()
}

/// Refresh the account capability/session projection immediately.
///
/// Managed embeds can receive their bridge token after the first bootstrap
/// request has already left without authentication. Waiting for the regular
/// ~30 s health check in that case leaves the account button missing even
/// though the daemon has a working auth backend. The bridge calls this once when
/// it installs a new token; repeated host init messages with the same token
/// remain no-ops at that layer.
pub(crate) fn refresh_status<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    fetch_status(inner, &base);
}

fn fetch_status<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    let request_id = STATUS_REQUEST_GATE.with(StatusRequestGate::begin);
    let inner = inner.clone();
    let _ = live_sync::get_with_status(
        &format!("{base}{}", auth_routes::STATUS),
        Rc::new(move |http_status, body: String| {
            let should_apply =
                STATUS_REQUEST_GATE.with(|gate| gate.should_apply(request_id, http_status));
            if !should_apply {
                return;
            }
            // Identity first: everything below paints for an account, so the
            // account has to be settled before any of it runs.
            let observation = crate::identity_epoch::observe_subject(
                crate::identity_epoch::subject_from_status(&body).as_deref(),
            );
            if observation.requires_reset() {
                crate::live_sync_glue::reset_for_new_identity(&inner);
            }
            if observation.requires_storage_reload() {
                // The shell loaded settings and credentials under `anon` at
                // mount; the account's own partition is a different key, so what
                // is in memory belongs to the wrong one until re-read.
                crate::web_settings::reload_for_active_partition(&inner);
            }
            // Soft borrow — this path is reached from a timer callback that can
            // land while an event holds the shell. The hard borrow panicked
            // every 30 s and killed the wasm instance, freezing the page on its
            // last painted frame (which is how the file list "stuck" on
            // Loading).
            let Ok(mut b) = inner.try_borrow_mut() else {
                return;
            };
            let account = account_from_status(&body);
            let ui = &mut b.host_mut().editor_state_mut().editor_ui;
            if apply_status_body(ui, &body) {
                ui.account = account;
                b.host_mut().mark_editor_state_dirty();
                let _ = b.repaint();
            }
        }),
    );
}

/// Fold a `GET /api/auth/status` body into the chrome state.
///
/// Returns whether anything changed. The account itself is written by the
/// caller so the identity epoch (which must run before any of this) keeps
/// owning the ordering.
fn apply_status_body(ui: &mut op_editor_core::EditorUiState, body: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let available = parsed["available"].as_bool().unwrap_or(false);
    let needs_first_admin = parsed["needs_first_admin"].as_bool().unwrap_or(false);
    let entry = &mut ui.account_entry;
    let changed = ui.account_ui_available != available
        || entry.needs_first_admin != needs_first_admin
        || !entry.status_received
        || ui.account != account_from_status(body);
    ui.account_ui_available = available;
    entry.needs_first_admin = needs_first_admin;
    entry.status_received = true;
    changed
}

/// The account a status (or sign-in) body describes.
pub(crate) fn account_from_status(body: &str) -> AccountState {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return AccountState::Anonymous;
    };
    if !parsed["signed_in"].as_bool().unwrap_or(false) {
        return AccountState::Anonymous;
    }
    AccountState::signed_in_account(
        parsed["display_name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        parsed["username"].as_str().map(str::to_string),
        // The stable key the deployment issued. An access list is keyed by it,
        // not by the handle beside it.
        parsed["subject"].as_str().map(str::to_string),
    )
}

/// Send a name and a password to the daemon.
///
/// The request owns the secret for exactly as long as it takes to serialize the
/// body — see `op_editor_core::account_entry_state` — and the form's password
/// field is already empty by the time this is called.
pub(crate) fn sign_in<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, request: SignInRequest) {
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let ok = live_sync::post_json_with_status(
        &format!("{base}{}", auth_routes::LOGIN),
        &request.body(),
        Rc::new(move |status, body| {
            apply_credential_answer(&inner_for_response, status, &body);
        }),
    );
    if !ok {
        fail_entry(inner, AccountEntryError::Unavailable);
    }
}

/// Send an invitation acceptance to the daemon.
pub(crate) fn accept_invitation<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    acceptance: InviteAcceptance,
) {
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let ok = live_sync::post_json_with_status(
        &format!("{base}{}", auth_routes::INVITE_ACCEPT),
        &acceptance.body(),
        Rc::new(move |status, body| {
            apply_credential_answer(&inner_for_response, status, &body);
        }),
    );
    if !ok {
        fail_entry(inner, AccountEntryError::Unavailable);
    }
}

/// Fold a sign-in or acceptance answer into the chrome.
///
/// `200` means the daemon has set the session cookie, so the answer itself is
/// the new identity — no second round trip, and no window in which the tab is
/// signed in but paints as signed out. Everything else is a refusal with a
/// reason, and the form says which.
fn apply_credential_answer<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    status: u16,
    body: &str,
) {
    if status == 200 {
        // The subject changed, so anything keyed to the previous account has to
        // go before the new one is painted — the same rule the status poll
        // follows, and the reason a sign-out then sign-in in one tab cannot
        // inherit the previous account's document.
        let observation = crate::identity_epoch::observe_subject(
            crate::identity_epoch::subject_from_status(body).as_deref(),
        );
        if observation.requires_reset() {
            crate::live_sync_glue::reset_for_new_identity(inner);
        }
        if observation.requires_storage_reload() {
            crate::web_settings::reload_for_active_partition(inner);
        }
        let account = account_from_status(body);
        let Ok(mut b) = inner.try_borrow_mut() else {
            return;
        };
        let ui = &mut b.host_mut().editor_state_mut().editor_ui;
        ui.account = account;
        ui.account_entry.succeed();
        ui.account_ui_available = true;
        ui.account_entry.status_received = true;
        b.host_mut().mark_editor_state_dirty();
        let _ = b.repaint();
        // The daemon's session is the authority on everything else the tab
        // shows (roles, the MCP-token row, the account menu gate), and reading
        // it now keeps this shell from guessing at any of it.
        refresh_status(inner);
        return;
    }
    fail_entry(inner, AccountEntryError::from_response(status, body));
}

/// Show a refusal on the form.
fn fail_entry<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, error: AccountEntryError) {
    let Ok(mut b) = inner.try_borrow_mut() else {
        return;
    };
    b.host_mut()
        .editor_state_mut()
        .editor_ui
        .account_entry
        .fail(error);
    b.host_mut().mark_editor_state_dirty();
    let _ = b.repaint();
}

/// Send the credentials a submit collected, if any.
///
/// Called from the post-press / post-keypress drain in the mount (immediately,
/// so a sign-in does not wait for the poll tick) and from the tick itself (so a
/// submit that arrived while the shell was borrowed is still sent).
pub(crate) fn drain_pending_credentials<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Ok(mut borrowed) = inner.try_borrow_mut() else {
        return;
    };
    let pending = borrowed.host_mut().take_pending_credential_request();
    drop(borrowed);
    match pending {
        Some(crate::widget_host::PendingCredentialRequest::SignIn(request)) => {
            sign_in(inner, request)
        }
        Some(crate::widget_host::PendingCredentialRequest::Invite(acceptance)) => {
            accept_invitation(inner, acceptance)
        }
        None => {}
    }
}

/// Drain the session actions a press queued.
fn drain_session_actions<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    // `try_borrow_mut`, not `borrow_mut`: this runs from the frame, where the
    // shell may legitimately be borrowed by an event in flight. The panic that
    // `borrow_mut` raised here ("already mutably borrowed") killed the whole
    // wasm instance — after it, nothing on the page updated again.
    let Ok(mut borrowed) = inner.try_borrow_mut() else {
        return;
    };
    let actions = borrowed.host_mut().take_pending_session_actions();
    drop(borrowed);
    for action in actions {
        match action {
            PendingSessionAction::SignOut => sign_out(inner, base),
        }
    }
}

/// End this session on the daemon, then re-read who the tab is.
///
/// The re-read is the point: the cookie is gone, but everything else the shell
/// derived from the session (roles, the settings partition) is not, and the
/// status answer is what moves the tab back to the anonymous partition through
/// the identity epoch.
fn sign_out<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    let inner_for_response = inner.clone();
    let _ = live_sync::post_json_with_status(
        &format!("{base}{}", auth_routes::LOGOUT),
        "{}",
        Rc::new(move |_status, _body| {
            // Whatever the answer, the question "who am I now" is worth asking:
            // a failed sign-out leaves the session in place, and the status
            // answer says so instead of the shell assuming either way.
            refresh_status(&inner_for_response);
        }),
    );
}

/// Forget what the tab typed for an account.
///
/// Called when a session ends: a password half-typed for an account this tab
/// is no longer signed in as has no reason to be on screen, and the form must
/// not be holding a secret while nobody is looking at it.
pub(crate) fn clear_entry_state(entry: &mut AccountEntryState) {
    entry.drop_secrets();
    entry.submitting = false;
    entry.error = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::EditorUiState;

    #[test]
    fn auth_status_gate_rejects_stale_success_after_a_newer_request_starts() {
        let gate = StatusRequestGate::new();
        let pre_token_request = gate.begin();
        let authenticated_request = gate.begin();

        assert!(authenticated_request > pre_token_request);
        assert!(!gate.should_apply(pre_token_request, 200));
        assert!(gate.should_apply(authenticated_request, 200));
    }

    #[test]
    fn auth_status_gate_rejects_non_success_even_for_latest_request() {
        let gate = StatusRequestGate::new();
        let request = gate.begin();

        assert!(!gate.should_apply(request, 0));
        assert!(!gate.should_apply(request, 401));
        assert!(!gate.should_apply(request, 500));
        assert!(gate.should_apply(request, 200));
    }

    #[test]
    fn an_anonymous_status_answer_opens_the_sign_in_form() {
        let mut ui = EditorUiState::default();
        let body = r#"{"available":true,"signed_in":false,"subject":null,"username":null,
                       "display_name":null,"roles":[],"needs_first_admin":false}"#;

        assert!(apply_status_body(&mut ui, body), "the answer changes state");
        ui.account = account_from_status(body);

        assert!(ui.account_ui_available);
        assert!(!ui.account_entry.needs_first_admin);
        assert_eq!(
            ui.account_entry_mode(),
            op_editor_core::AccountEntryMode::SignIn
        );
    }

    #[test]
    fn a_local_deployment_answer_never_opens_a_form() {
        let mut ui = EditorUiState::default();
        let body = r#"{"available":false,"signed_in":false,"needs_first_admin":false}"#;

        assert!(apply_status_body(&mut ui, body));
        assert_eq!(
            ui.account_entry_mode(),
            op_editor_core::AccountEntryMode::Hidden
        );
    }

    #[test]
    fn an_unprovisioned_deployment_shows_the_explanation_instead() {
        let mut ui = EditorUiState::default();
        let body = r#"{"available":true,"signed_in":false,"needs_first_admin":true}"#;

        assert!(apply_status_body(&mut ui, body));
        assert_eq!(
            ui.account_entry_mode(),
            op_editor_core::AccountEntryMode::Unprovisioned
        );
    }

    #[test]
    fn a_signed_in_answer_closes_the_form_and_names_the_account() {
        let mut ui = EditorUiState::default();
        let body = r#"{"available":true,"signed_in":true,"subject":"u1","username":"kay",
                       "display_name":"Kay Shen","roles":["admin"],"needs_first_admin":false}"#;

        assert!(apply_status_body(&mut ui, body));
        ui.account = account_from_status(body);

        assert_eq!(
            ui.account_entry_mode(),
            op_editor_core::AccountEntryMode::Hidden
        );
        assert_eq!(
            ui.account,
            AccountState::SignedIn {
                // The stable key the deployment issued travels with the profile:
                // an access list is keyed by it, and the Share dialog needs it
                // to build a link somebody else can open.
                display_name: "Kay Shen".to_string(),
                username: "kay".to_string(),
                // The stable key the deployment issued travels with the
                // profile: an access list is keyed by it, and the Share dialog
                // needs it to build a link somebody else can open.
                account_id: Some("u1".to_string()),
            }
        );
    }

    #[test]
    fn a_repeat_of_the_same_answer_is_not_a_change() {
        // The status poll runs every 30 s; a repeated answer must not churn the
        // chrome dirty flag (or, worse, reset the document through the epoch).
        let mut ui = EditorUiState::default();
        let body = r#"{"available":true,"signed_in":true,"subject":"u1","username":"kay",
                       "display_name":"Kay Shen","needs_first_admin":false}"#;
        assert!(apply_status_body(&mut ui, body));
        ui.account = account_from_status(body);

        assert!(!apply_status_body(&mut ui, body), "no change to project");
    }

    #[test]
    fn an_unparseable_answer_leaves_the_shell_alone() {
        let mut ui = EditorUiState::default();
        assert!(!apply_status_body(&mut ui, "not json"));
        assert!(!ui.account_entry.status_received);
        assert_eq!(
            ui.account_entry_mode(),
            op_editor_core::AccountEntryMode::Hidden
        );
    }

    #[test]
    fn a_signed_in_body_without_a_display_name_still_names_the_account() {
        let account = account_from_status(r#"{"signed_in":true,"username":"kay"}"#);
        assert_eq!(
            account,
            AccountState::SignedIn {
                display_name: String::new(),
                username: "kay".to_string(),
                account_id: None,
            }
        );
    }

    #[test]
    fn a_signed_out_body_is_anonymous_even_when_it_carries_a_stale_handle() {
        assert_eq!(
            account_from_status(r#"{"signed_in":false,"username":"kay"}"#),
            AccountState::Anonymous
        );
    }

    #[test]
    fn a_tab_on_an_invitation_declares_its_address_for_the_router() {
        // The router must not write the editor's own address over
        // `/invite/<token>` before the invitation has been accepted: the link is
        // the credential the form is about to send, and rewriting the address
        // would throw it away (and lose it on a refresh).
        let mut host = crate::widget_host::WidgetHost::new();
        assert!(!invitation_address_active(&host));

        host.editor_state_mut()
            .editor_ui
            .set_invite_token(Some("tok-1".to_string()));
        assert!(invitation_address_active(&host));

        // Accepting it clears the token, and the address stops belonging to the
        // invitation — which is what lets the next router tick move the tab to
        // the editor it just signed into.
        host.editor_state_mut().editor_ui.account_entry.succeed();
        assert!(!invitation_address_active(&host));
    }

    #[test]
    fn signing_out_forgets_a_half_typed_password() {
        let mut entry = AccountEntryState {
            status_received: true,
            password: "karta-mosta-42".to_string(),
            submitting: true,
            error: Some(AccountEntryError::Rejected),
            ..AccountEntryState::default()
        };
        clear_entry_state(&mut entry);
        assert_eq!(entry.password, "");
        assert!(!entry.submitting);
        assert_eq!(entry.error, None);
    }
}
