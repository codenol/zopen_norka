//! The account session's response handling, tested without a DOM.
//!
//! Split from `web_auth_sync.rs` at the 800-line cap. These tests drive the
//! real functions the XHR callbacks call — the status projection, the identity
//! gate, and the refusal mapping that issue #151 is about — against a real
//! `WidgetHost` and a minimal [`RepaintContext`], rather than asserting on
//! source text.

use super::*;
use op_editor_core::EditorUiState;

use crate::repaint_ctx::RepaintContext;

/// The smallest shell [`apply_credential_answer`] runs against: a host and a
/// repaint tally.
///
/// This double exists so the refusal path can be driven through the real
/// function the XHR callback calls. A test that re-implemented the mapping
/// would have passed while the shell still showed "unavailable" — which is
/// exactly what issue #151 was.
struct RefusalContext {
    host: crate::widget_host::WidgetHost,
    repaints: usize,
}

impl RefusalContext {
    fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            host: crate::widget_host::WidgetHost::new(),
            repaints: 0,
        }))
    }
}

impl RepaintContext for RefusalContext {
    fn host(&self) -> &crate::widget_host::WidgetHost {
        &self.host
    }

    fn host_mut(&mut self) -> &mut crate::widget_host::WidgetHost {
        &mut self.host
    }

    fn viewport_size(&self) -> (f32, f32) {
        (1440.0, 900.0)
    }

    fn register_system_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font_from_bytes(&mut self, _bytes: &[u8]) -> Option<String> {
        None
    }

    fn imported_family_list(&self) -> Vec<String> {
        Vec::new()
    }

    fn remove_imported_font(&mut self, _family: &str) {}

    fn repaint(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        self.repaints += 1;
        Ok(())
    }
}

/// The refusal the form is holding, as the shell's own state sees it.
fn shown_error(context: &Rc<RefCell<RefusalContext>>) -> Option<AccountEntryError> {
    context
        .borrow()
        .host
        .editor_state()
        .editor_ui
        .account_entry
        .error
}

/// What the person reads: the sentence the widget resolves for that refusal.
///
/// The widget facade itself is only reachable from `widget_host`
/// (`tools/check-widget-boundary.sh`, spec §1.4), so the sentence is resolved
/// through that module's own reader rather than pulled in here.
fn shown_sentence(context: &Rc<RefCell<RefusalContext>>) -> String {
    let error = shown_error(context).expect("a refusal is on screen");
    crate::widget_host::account_entry::refusal_sentence(error)
}

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

/// Issue #151, end to end through the path the browser actually takes.
///
/// A `429` from `POST /api/auth/login` — the answer a person who has spent
/// their attempts gets — must put the "wait" sentence on the form and not
/// the generic "unavailable" one. This test drives [`apply_credential_answer`],
/// the same function the XHR callback calls, with the same body the daemon
/// sends and the `Retry-After` the header carried.
#[test]
fn a_locked_out_sign_in_shows_how_long_to_wait_and_not_unavailable() {
    let context = RefusalContext::new();
    // The daemon's own bytes: no count, no name, one shared body for every
    // ceiling that can fire — see `AccountReply::throttled`.
    let throttled = r#"{"ok":false,"error":"too-many-attempts","message":"too many failed sign-in attempts; wait before trying again"}"#;

    apply_credential_answer(&context, 429, throttled, Some(900));

    assert_eq!(
        shown_error(&context),
        Some(AccountEntryError::TooManyAttempts {
            retry_after_secs: Some(900)
        }),
        "a spent budget is its own answer, not a generic failure"
    );
    assert!(
        !context
            .borrow()
            .host
            .editor_state()
            .editor_ui
            .account_entry
            .submitting,
        "the form is usable again after the refusal"
    );

    let sentence = shown_sentence(&context);
    assert!(
        sentence.contains("900"),
        "the person has to be told how long the wait is: {sentence}"
    );
    assert_ne!(
        sentence,
        op_i18n::translate(
            op_editor_core::Locale::EnUs,
            "account.entry.errorUnavailable"
        ),
        "and must not be told the sign-in is unavailable"
    );
}

/// The same refusal when the `Retry-After` did not survive the trip.
///
/// A proxy may strip an unexposed header, and a browser on another origin
/// cannot read one the deployment does not expose. The reason for the
/// refusal is still certain — the status says it — so the form keeps the
/// honest "too many attempts" sentence without a figure, rather than
/// falling back to "unavailable" or inventing a number.
#[test]
fn a_locked_out_sign_in_without_a_readable_wait_still_says_what_happened() {
    let context = RefusalContext::new();
    apply_credential_answer(
        &context,
        429,
        r#"{"ok":false,"error":"too-many-attempts","message":"wait"}"#,
        None,
    );

    assert_eq!(
        shown_error(&context),
        Some(AccountEntryError::TooManyAttempts {
            retry_after_secs: None
        })
    );
    let sentence = shown_sentence(&context);
    assert_ne!(
        sentence,
        op_i18n::translate(
            op_editor_core::Locale::EnUs,
            "account.entry.errorUnavailable"
        ),
        "an unreadable header does not make the service unavailable: {sentence}"
    );
    assert!(
        !sentence.contains("{{"),
        "an unsubstituted placeholder must never reach the screen: {sentence}"
    );
}
