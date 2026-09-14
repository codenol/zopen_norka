//! The account entry form: geometry, hit-testing, and what each state paints.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
#[allow(unused_imports)]
use crate::widgets::Widget as _;
use op_editor_core::{AccountEntryError, AccountState};

const VW: f32 = 1200.0;
const VH: f32 = 800.0;

fn signed_out_state() -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.account_ui_available = true;
    state.editor_ui.account_entry.status_received = true;
    state
}

fn form(state: &EditorState, mode: AccountEntryMode) -> AccountEntryForm<'_> {
    let form = AccountEntryForm::for_editor(state, VW, VH).expect("the state shows a form");
    assert_eq!(form.mode(), mode, "unexpected body");
    form
}

/// Every string the backend was asked to draw, in paint order.
fn painted_text_of(backend: &CaptureBackend) -> String {
    backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn paint(state: &EditorState) -> (AccountEntryLayout, CaptureBackend) {
    let form = AccountEntryForm::for_editor(state, VW, VH).expect("the state shows a form");
    let layout = form.layout();
    let mut backend = CaptureBackend::default();
    form.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        layout.card,
    );
    (layout, backend)
}

// --- which state shows what ----------------------------------------------

#[test]
fn a_local_deployment_shows_no_form_at_all() {
    // `available: false` — the desktop and managed runs — has no sign-in, so
    // there is nothing for a host to paint and nothing to hit-test.
    let mut state = EditorState::new();
    state.editor_ui.account_entry.status_received = true;
    assert!(AccountEntryForm::for_editor(&state, VW, VH).is_none());
}

#[test]
fn a_signed_in_tab_shows_no_form() {
    let mut state = signed_out_state();
    state.editor_ui.account = AccountState::dev_fake_signed_in();
    assert!(AccountEntryForm::for_editor(&state, VW, VH).is_none());
}

#[test]
fn an_unanswered_status_shows_no_form_yet() {
    // The frame between mount and the first `/api/auth/status` answer. A form
    // here would flash on a local deployment too, where it never belongs.
    let mut state = EditorState::new();
    state.editor_ui.account_ui_available = true;
    assert!(AccountEntryForm::for_editor(&state, VW, VH).is_none());
}

#[test]
fn an_available_signed_out_deployment_shows_the_password_form() {
    let state = signed_out_state();
    let form = form(&state, AccountEntryMode::SignIn);
    assert_eq!(
        form.layout()
            .fields
            .iter()
            .map(|field| field.field)
            .collect::<Vec<_>>(),
        vec![AccountField::Username, AccountField::Password]
    );
}

#[test]
fn a_deployment_with_no_accounts_explains_and_offers_nothing_to_submit() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.needs_first_admin = true;
    let form = form(&state, AccountEntryMode::Unprovisioned);
    let layout = form.layout();

    assert!(layout.fields.is_empty(), "nothing to type into");
    assert!(layout.submit.is_none(), "nothing to submit");
    assert!(layout.body.size.y > 0.0, "the explanation needs its band");
}

#[test]
fn an_invitation_address_shows_the_acceptance_form() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.invite_token = Some("tok-1".to_string());
    let form = form(&state, AccountEntryMode::Invite);
    assert_eq!(
        form.layout()
            .fields
            .iter()
            .map(|field| field.field)
            .collect::<Vec<_>>(),
        vec![
            AccountField::InviteUsername,
            AccountField::InvitePassword,
            AccountField::InvitePasswordConfirm,
            AccountField::InviteDisplayName
        ]
    );
}

// --- geometry ------------------------------------------------------------

#[test]
fn every_rect_of_every_body_sits_inside_the_card() {
    for state in [
        signed_out_state(),
        {
            let mut state = signed_out_state();
            state.editor_ui.account_entry.needs_first_admin = true;
            state
        },
        {
            let mut state = signed_out_state();
            state.editor_ui.account_entry.invite_token = Some("tok-1".to_string());
            state
        },
    ] {
        let form = AccountEntryForm::for_editor(&state, VW, VH).expect("a form is shown");
        let layout = form.layout();
        let inside = |rect: Rect| {
            rect.origin.x >= layout.card.origin.x - 0.01
                && rect.origin.y >= layout.card.origin.y - 0.01
                && rect.origin.x + rect.size.x <= layout.card.origin.x + layout.card.size.x + 0.01
                && rect.origin.y + rect.size.y <= layout.card.origin.y + layout.card.size.y + 0.01
        };
        assert!(inside(layout.title), "{:?}", layout.title);
        assert!(inside(layout.subtitle));
        assert!(inside(layout.error));
        for field in &layout.fields {
            assert!(inside(field.label), "{:?}", field.label);
            assert!(inside(field.input), "{:?}", field.input);
            assert!(field.label.origin.y + field.label.size.y <= field.input.origin.y);
        }
        if let Some(submit) = layout.submit {
            assert!(inside(submit), "{:?}", submit);
        }
    }
}

#[test]
fn fields_and_the_button_never_overlap() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.invite_token = Some("tok-1".to_string());
    let layout = AccountEntryForm::for_editor(&state, VW, VH)
        .expect("a form is shown")
        .layout();

    let mut previous_bottom = layout.subtitle.origin.y + layout.subtitle.size.y;
    for field in &layout.fields {
        assert!(
            field.label.origin.y >= previous_bottom,
            "a field overlaps what is above it"
        );
        previous_bottom = field.input.origin.y + field.input.size.y;
    }
    let submit = layout.submit.expect("the invitation form submits");
    assert!(
        submit.origin.y >= previous_bottom,
        "the button would sit on the last field"
    );
    // The error line lives between the last field and the button, where a
    // refusal can be read without moving the button.
    assert!(layout.error.origin.y >= previous_bottom);
    assert!(layout.error.origin.y + layout.error.size.y <= submit.origin.y);
}

#[test]
fn the_card_reserves_the_same_error_band_whether_or_not_one_is_shown() {
    // A form that grows when it refuses an attempt moves the button out from
    // under the pointer that just pressed it.
    let clean = AccountEntryForm::for_editor(&signed_out_state(), VW, VH)
        .expect("a form is shown")
        .layout();
    let mut refused_state = signed_out_state();
    refused_state.editor_ui.account_entry.error = Some(AccountEntryError::Rejected);
    let refused = AccountEntryForm::for_editor(&refused_state, VW, VH)
        .expect("a form is shown")
        .layout();
    assert_eq!(clean.card, refused.card);
    assert_eq!(clean.submit, refused.submit);
}

#[test]
fn a_narrow_viewport_keeps_the_card_on_screen() {
    let state = signed_out_state();
    let form = AccountEntryForm::for_editor(&state, 320.0, 480.0).expect("a form is shown");
    let card = form.rect();
    assert!(card.origin.x >= 0.0);
    assert!(card.origin.x + card.size.x <= 320.0);
    assert!(card.size.x > 0.0);
}

// --- hit-testing ---------------------------------------------------------

#[test]
fn a_press_on_a_field_lands_on_that_field() {
    let state = signed_out_state();
    let form = form(&state, AccountEntryMode::SignIn);
    let layout = form.layout();
    for field in &layout.fields {
        let centre = Point2D::new(
            field.input.origin.x + field.input.size.x / 2.0,
            field.input.origin.y + field.input.size.y / 2.0,
        );
        assert_eq!(form.hit_test(centre), AccountEntryHit::Field(field.field));
    }
}

#[test]
fn a_press_on_the_button_submits_and_the_scrim_swallows_the_rest() {
    let state = signed_out_state();
    let form = form(&state, AccountEntryMode::SignIn);
    let layout = form.layout();
    let submit = layout.submit.expect("the sign-in form submits");
    assert_eq!(
        form.hit_test(Point2D::new(
            submit.origin.x + submit.size.x / 2.0,
            submit.origin.y + submit.size.y / 2.0,
        )),
        AccountEntryHit::Submit
    );
    // The scrim is consumed: a press beside the card must not reach the editor
    // the visitor cannot use anyway.
    assert_eq!(
        form.hit_test(Point2D::new(layout.card.origin.x - 40.0, 10.0)),
        AccountEntryHit::Outside
    );
    // Card chrome that is neither a field nor the button is consumed quietly.
    assert_eq!(
        form.hit_test(Point2D::new(
            layout.title.origin.x + 2.0,
            layout.title.origin.y + 2.0
        )),
        AccountEntryHit::Inside
    );
}

#[test]
fn a_button_pressed_twice_does_not_submit_twice() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.submitting = true;
    let form = form(&state, AccountEntryMode::SignIn);
    let submit = form.layout().submit.expect("the form still shows a button");
    assert_eq!(
        form.hit_test(Point2D::new(
            submit.origin.x + submit.size.x / 2.0,
            submit.origin.y + submit.size.y / 2.0,
        )),
        AccountEntryHit::Inside,
        "the round trip already in flight is the only one"
    );
}

#[test]
fn the_explanation_body_has_nothing_to_press() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.needs_first_admin = true;
    let form = form(&state, AccountEntryMode::Unprovisioned);
    let layout = form.layout();
    assert_eq!(
        form.hit_test(Point2D::new(
            layout.body.origin.x + layout.body.size.x / 2.0,
            layout.body.origin.y + layout.body.size.y / 2.0,
        )),
        AccountEntryHit::Inside
    );
}

// --- what is painted -----------------------------------------------------

#[test]
fn a_typed_password_is_never_painted_in_the_clear() {
    let mut state = signed_out_state();
    state.editor_ui.account_entry.focus = Some(AccountField::Password);
    state.editor_ui.account_entry.password = "karta-mosta-42".to_string();

    let (_, backend) = paint(&state);

    assert!(
        !backend
            .texts
            .iter()
            .any(|(text, _)| text.contains("karta-mosta-42")),
        "the secret reached the screen: {:?}",
        backend.texts
    );
    // One bullet per typed character: the field still shows HOW MUCH was
    // typed, which is what a person needs to see while typing it.
    let bullets = "•".repeat("karta-mosta-42".chars().count());
    assert!(
        backend.texts.iter().any(|(text, _)| *text == bullets),
        "the field still has to show that something was typed: {:?}",
        backend.texts
    );
}

#[test]
fn a_refusal_is_painted_under_the_form_in_the_reader_s_language() {
    let mut state = signed_out_state();
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    state.editor_ui.account_entry.error = Some(AccountEntryError::Rejected);
    let (_, backend) = paint(&state);
    assert!(backend
        .texts
        .iter()
        .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, "account.entry.errorRejected")));
    // And nothing about the failure says WHICH half was wrong: the two
    // outcomes share one key (asserted in `op_editor_core`), so the surface
    // cannot tell a wrong name from a wrong password even by accident.
    assert_eq!(
        error_key(AccountEntryError::Rejected),
        "account.entry.errorRejected"
    );
}

#[test]
fn a_disabled_account_is_told_apart_from_a_wrong_password() {
    let mut state = signed_out_state();
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    state.editor_ui.account_entry.error = Some(AccountEntryError::Disabled);
    let (_, backend) = paint(&state);
    assert!(backend
        .texts
        .iter()
        .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, "account.entry.errorDisabled")));
    assert!(!backend
        .texts
        .iter()
        .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, "account.entry.errorRejected")));
}

#[test]
fn the_unprovisioned_surface_names_both_ways_to_provision_an_admin() {
    let mut state = signed_out_state();
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    state.editor_ui.account_entry.needs_first_admin = true;
    let (_, backend) = paint(&state);
    // Wrapped, so the two halves of the fix are checked as words rather than
    // as one run: a line break lands wherever the width puts it.
    let painted = painted_text_of(&backend);
    assert!(
        painted.contains("NORKA_ADMIN_USERNAME") && painted.contains("admin"),
        "the explanation has to say what to do: {painted}"
    );
    // And it offers no sign-in button, because there is nothing to sign in to.
    assert!(!backend
        .texts
        .iter()
        .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, "account.entry.signIn")));
}

#[test]
fn the_button_says_what_is_happening_while_a_request_is_in_flight() {
    let mut state = signed_out_state();
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    state.editor_ui.account_entry.submitting = true;
    let (_, backend) = paint(&state);
    assert!(backend
        .texts
        .iter()
        .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, "account.entry.signingIn")));

    let mut invite = signed_out_state();
    invite.editor_ui.locale = op_editor_core::Locale::EnUs;
    invite.editor_ui.account_entry.invite_token = Some("tok-1".to_string());
    invite.editor_ui.account_entry.submitting = true;
    let (_, backend) = paint(&invite);
    assert!(backend.texts.iter().any(|(text, _)| text
        == t(
            op_editor_core::Locale::EnUs,
            "account.entry.acceptingInvite"
        )));
}

#[test]
fn the_invitation_form_asks_for_the_password_twice_and_for_a_name() {
    let mut state = signed_out_state();
    state.editor_ui.locale = op_editor_core::Locale::EnUs;
    state.editor_ui.account_entry.invite_token = Some("tok-1".to_string());
    let (_, backend) = paint(&state);
    // The two passwords share the "Password" label; what tells them apart is
    // that the second one says it is a repeat.
    for key in [
        "account.entry.username",
        "account.entry.password",
        "account.entry.inviteConfirm",
        "account.entry.inviteDisplayName",
    ] {
        assert!(
            backend
                .texts
                .iter()
                .any(|(text, _)| text == t(op_editor_core::Locale::EnUs, key)),
            "missing label for {key}: {:?}",
            backend.texts
        );
    }
}

#[test]
fn every_refusal_reason_has_its_own_string() {
    use op_editor_core::AccountEntryError as E;
    let reasons = [
        E::Rejected,
        E::Disabled,
        E::Unprovisioned,
        E::WeakPassword,
        E::InvalidInput,
        E::UsernameTaken,
        E::InviteNotFound,
        E::InviteExpired,
        E::InviteAlreadyAccepted,
        E::EmptyFields,
        E::PasswordMismatch,
        E::Unavailable,
    ];
    for reason in reasons {
        let key = error_key(reason);
        assert!(
            key.starts_with("account.entry.error"),
            "{reason:?} has no key"
        );
        assert_ne!(
            t(op_editor_core::Locale::EnUs, key),
            key,
            "no English string for {key}"
        );
    }
    // The two answers a person can confuse must not share a sentence.
    assert_ne!(error_key(E::Rejected), error_key(E::Disabled));
    assert_ne!(
        error_key(E::InviteExpired),
        error_key(E::InviteAlreadyAccepted)
    );
}
