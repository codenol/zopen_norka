//! Account-entry state: what is shown, what is typed, and what is forgotten.

use super::*;

/// The refusal an attempt earned, if it was refused.
///
/// The success side is deliberately not comparable — a pending request holds a
/// password and has no `PartialEq` — so an assertion asks for the reason.
fn refusal<T>(outcome: Result<T, AccountEntryError>) -> Option<AccountEntryError> {
    outcome.err()
}

fn reachable(status_received: bool) -> AccountEntryState {
    AccountEntryState {
        status_received,
        ..AccountEntryState::default()
    }
}

// --- which surface shows -------------------------------------------------

#[test]
fn a_deployment_without_accounts_shows_no_form() {
    // `available: false` is a local or managed run: there is no sign-in here,
    // and a form would be a promise this deployment cannot keep.
    let entry = reachable(true);
    assert_eq!(entry.mode(false, false), AccountEntryMode::Hidden);
}

#[test]
fn a_signed_in_tab_shows_no_form() {
    let entry = reachable(true);
    assert_eq!(entry.mode(true, true), AccountEntryMode::Hidden);
}

#[test]
fn an_unanswered_status_shows_no_form_yet() {
    // The mount paints before the first `/api/auth/status` answer; a form
    // flashed then would appear in Local mode too and vanish a frame later.
    let entry = reachable(false);
    assert_eq!(entry.mode(true, false), AccountEntryMode::Hidden);
}

#[test]
fn an_available_deployment_with_no_session_shows_the_form() {
    let entry = reachable(true);
    assert_eq!(entry.mode(true, false), AccountEntryMode::SignIn);
}

#[test]
fn a_deployment_nobody_provisioned_explains_instead_of_asking() {
    let mut entry = reachable(true);
    entry.needs_first_admin = true;
    assert_eq!(entry.mode(true, false), AccountEntryMode::Unprovisioned);
    // The explanation does not depend on the address: an invite link into a
    // deployment with no admin would still be the invite form.
    entry.invite_token = Some("tok".to_string());
    assert_eq!(entry.mode(true, false), AccountEntryMode::Invite);
}

#[test]
fn an_invite_link_shows_its_form_before_any_status_answer() {
    // The link IS the credential and nobody is signed in by definition, so it
    // does not wait for the deployment to describe itself.
    let entry = AccountEntryState {
        invite_token: Some("tok".to_string()),
        ..AccountEntryState::default()
    };
    assert_eq!(entry.mode(false, false), AccountEntryMode::Invite);
    assert_eq!(entry.mode(true, true), AccountEntryMode::Invite);
}

// --- typing --------------------------------------------------------------

#[test]
fn typing_goes_to_the_focused_field_only() {
    let mut entry = reachable(true);
    entry.focus_field(AccountField::Username);
    assert!(entry.push_char('a'));
    entry.focus_field(AccountField::Password);
    assert!(entry.push_char('b'));

    assert_eq!(entry.username, "a");
    assert_eq!(entry.password, "b");
}

#[test]
fn typing_without_focus_changes_nothing() {
    let mut entry = reachable(true);
    assert!(!entry.push_char('a'));
    assert_eq!(entry.username, "");
}

#[test]
fn backspace_empties_the_focused_field_and_then_stops_reporting_changes() {
    let mut entry = reachable(true);
    entry.focus_field(AccountField::Username);
    entry.push_char('a');
    assert!(entry.backspace());
    assert!(!entry.backspace());
    assert_eq!(entry.username, "");
}

#[test]
fn the_invitation_form_has_its_own_fields() {
    // A sign-in draft and an invitation draft must not meet: following a link
    // while half-signed-in-somewhere-else would otherwise submit the wrong one.
    let mut entry = reachable(true);
    entry.username = "signin-name".to_string();
    entry.focus_field(AccountField::InviteUsername);
    entry.push_char('i');
    assert_eq!(entry.invite_username, "i");
    assert_eq!(entry.username, "signin-name");
}

// --- submitting the sign-in form -----------------------------------------

#[test]
fn a_submitted_password_leaves_the_state_immediately() {
    let mut entry = reachable(true);
    entry.username = "  kay  ".to_string();
    entry.password = "karta-mosta-42".to_string();
    entry.focus_field(AccountField::Password);

    let request = entry.begin_sign_in().expect("both fields are filled");

    // The whole point: the secret is out of editor state before the request
    // has even been built, so no later snapshot, repaint, or clone holds it.
    assert_eq!(entry.password, "");
    assert!(!entry.username.is_empty());
    assert!(entry.submitting);
    assert_eq!(request.username, "kay", "the name is trimmed");
    assert!(request.body().contains("karta-mosta-42"));
}

#[test]
fn a_debug_rendering_of_a_pending_request_carries_no_secret() {
    let mut entry = reachable(true);
    entry.username = "kay".to_string();
    entry.password = "karta-mosta-42".to_string();
    let request = entry.begin_sign_in().expect("both fields are filled");

    let rendered = format!("{request:?}");
    assert!(!rendered.contains("karta-mosta-42"), "{rendered}");
    assert!(rendered.contains("<redacted>"), "{rendered}");
}

#[test]
fn an_empty_field_is_refused_locally_and_keeps_what_was_typed() {
    let mut entry = reachable(true);
    entry.username = "kay".to_string();
    // No password: a round trip cannot answer this, and the name stays put so
    // the person only has to fill in the missing half.
    assert_eq!(
        refusal(entry.begin_sign_in()),
        Some(AccountEntryError::EmptyFields)
    );
    assert_eq!(entry.username, "kay");
    assert!(!entry.submitting);
}

#[test]
fn a_whitespace_only_name_is_not_a_name() {
    let mut entry = reachable(true);
    entry.username = "   ".to_string();
    entry.password = "karta-mosta-42".to_string();
    assert_eq!(
        refusal(entry.begin_sign_in()),
        Some(AccountEntryError::EmptyFields)
    );
}

#[test]
fn a_refusal_ends_the_submit_and_drops_every_secret() {
    let mut entry = reachable(true);
    entry.username = "kay".to_string();
    entry.password = "karta-mosta-42".to_string();
    entry.invite_password = "another-secret".to_string();
    let _ = entry.begin_sign_in().expect("both fields are filled");

    entry.fail(AccountEntryError::Rejected);

    assert!(!entry.submitting);
    assert_eq!(entry.error, Some(AccountEntryError::Rejected));
    assert_eq!(entry.password, "");
    assert_eq!(entry.invite_password, "");
    assert_eq!(entry.username, "kay", "the name is not a secret");
}

#[test]
fn success_clears_the_form_and_the_spent_invitation() {
    let mut entry = AccountEntryState {
        status_received: true,
        invite_token: Some("spent".to_string()),
        ..AccountEntryState::default()
    };
    entry.invite_username = "kay".to_string();
    entry.invite_password = "karta-mosta-42".to_string();
    entry.invite_confirm = "karta-mosta-42".to_string();
    let _ = entry
        .begin_invite_acceptance()
        .expect("the form is complete");

    entry.succeed();

    assert!(entry.invite_token.is_none(), "the link has been used up");
    assert!(entry.invite_username.is_empty());
    assert!(entry.invite_password.is_empty());
    assert_eq!(entry.error, None);
    // And the surface is gone with it: a spent token must not repaint a form.
    assert_eq!(entry.mode(true, false), AccountEntryMode::SignIn);
}

// --- submitting the invitation form --------------------------------------

#[test]
fn an_acceptance_carries_the_token_and_the_chosen_name() {
    let mut entry = AccountEntryState {
        invite_token: Some("tok-1".to_string()),
        ..AccountEntryState::default()
    };
    entry.invite_username = " kay ".to_string();
    entry.invite_password = "karta-mosta-42".to_string();
    entry.invite_confirm = "karta-mosta-42".to_string();
    entry.invite_display_name = "  Kay Shen  ".to_string();

    let acceptance = entry.begin_invite_acceptance().expect("the form is valid");
    let body = acceptance.body();

    assert!(body.contains("tok-1"));
    assert!(body.contains("kay"));
    assert!(body.contains("Kay Shen"));
    assert_eq!(entry.invite_password, "");
    assert_eq!(entry.invite_confirm, "");
}

#[test]
fn an_optional_display_name_is_left_out_rather_than_sent_blank() {
    let mut entry = AccountEntryState {
        invite_token: Some("tok-1".to_string()),
        ..AccountEntryState::default()
    };
    entry.invite_username = "kay".to_string();
    entry.invite_password = "karta-mosta-42".to_string();
    entry.invite_confirm = "karta-mosta-42".to_string();
    entry.invite_display_name = "   ".to_string();

    let acceptance = entry.begin_invite_acceptance().expect("the form is valid");
    assert!(!acceptance.body().contains("display_name"));
}

#[test]
fn two_different_passwords_never_reach_the_daemon() {
    let mut entry = AccountEntryState {
        invite_token: Some("tok-1".to_string()),
        ..AccountEntryState::default()
    };
    entry.invite_username = "kay".to_string();
    entry.invite_password = "karta-mosta-42".to_string();
    entry.invite_confirm = "karta-mosta-43".to_string();

    assert_eq!(
        refusal(entry.begin_invite_acceptance()),
        Some(AccountEntryError::PasswordMismatch)
    );
    // Refused before anything was written: an account made for a mistyped
    // password is an account nobody can sign in to.
    assert!(!entry.submitting);
}

#[test]
fn an_acceptance_without_a_token_is_never_built() {
    let mut entry = AccountEntryState {
        invite_username: "kay".to_string(),
        invite_password: "karta-mosta-42".to_string(),
        invite_confirm: "karta-mosta-42".to_string(),
        ..AccountEntryState::default()
    };
    assert_eq!(
        refusal(entry.begin_invite_acceptance()),
        Some(AccountEntryError::InviteNotFound)
    );
}

// --- reading the daemon's answers ----------------------------------------

#[test]
fn a_wrong_name_and_a_wrong_password_read_the_same() {
    // The store returns one outcome for both so the form cannot be used to
    // discover which account names exist; the surface must not undo that.
    let wrong_name = AccountEntryError::from_response(
        401,
        r#"{"ok":false,"error":"unauthorized","message":"the name and password do not open an account"}"#,
    );
    let wrong_password = AccountEntryError::from_response(
        401,
        r#"{"ok":false,"error":"unauthorized","message":"the name and password do not open an account"}"#,
    );
    assert_eq!(wrong_name, AccountEntryError::Rejected);
    assert_eq!(wrong_password, AccountEntryError::Rejected);
}

#[test]
fn a_disabled_account_is_its_own_answer_because_the_password_was_right() {
    assert_eq!(
        AccountEntryError::from_response(
            403,
            r#"{"ok":false,"error":"account-disabled","message":"this account is disabled"}"#
        ),
        AccountEntryError::Disabled
    );
}

#[test]
fn an_unprovisioned_deployment_stops_offering_a_form() {
    let error = AccountEntryError::from_response(
        503,
        r#"{"ok":false,"error":"accounts-unprovisioned","message":"this deployment has no accounts yet"}"#,
    );
    assert_eq!(error, AccountEntryError::Unprovisioned);
    assert!(error.leaves_nothing_to_try());
}

#[test]
fn a_store_that_cannot_be_read_is_not_a_rejection() {
    // A disk fault must not read as "your password is wrong": that would sign
    // everybody out in their heads for the duration of the fault.
    assert_eq!(
        AccountEntryError::from_response(
            503,
            r#"{"ok":false,"error":"accounts-unavailable","message":"the account store cannot be read right now"}"#
        ),
        AccountEntryError::Unavailable
    );
}

#[test]
fn the_two_conflicts_are_told_apart_by_the_code_not_the_status() {
    assert_eq!(
        AccountEntryError::from_response(
            409,
            r#"{"ok":false,"error":"username-taken","message":"that name is taken"}"#
        ),
        AccountEntryError::UsernameTaken
    );
    assert_eq!(
        AccountEntryError::from_response(
            409,
            r#"{"ok":false,"error":"invite-already-accepted","message":"already used"}"#
        ),
        AccountEntryError::InviteAlreadyAccepted
    );
}

#[test]
fn a_spent_or_expired_invitation_says_which_one_it_is() {
    assert_eq!(
        AccountEntryError::from_response(
            404,
            r#"{"ok":false,"error":"invite-not-found","message":"this invitation does not exist"}"#
        ),
        AccountEntryError::InviteNotFound
    );
    assert_eq!(
        AccountEntryError::from_response(
            410,
            r#"{"ok":false,"error":"invite-expired","message":"this invitation has expired"}"#
        ),
        AccountEntryError::InviteExpired
    );
}

#[test]
fn a_weak_password_is_told_apart_from_a_malformed_name() {
    for code in [
        "password-too-short",
        "password-too-repetitive",
        "password-names-the-account",
    ] {
        let body = format!(r#"{{"ok":false,"error":"{code}","message":"no"}}"#);
        assert_eq!(
            AccountEntryError::from_response(400, &body),
            AccountEntryError::WeakPassword,
            "{code}"
        );
    }
    assert_eq!(
        AccountEntryError::from_response(
            400,
            r#"{"ok":false,"error":"invalid-account","message":"no"}"#
        ),
        AccountEntryError::InvalidInput
    );
}

#[test]
fn an_answer_outside_the_contract_reads_as_unavailable() {
    for (status, body) in [
        (500u16, r#"{"ok":false}"#),
        (418, "I am a teapot"),
        (0, ""),
        (200, r#"{"ok":true}"#),
    ] {
        assert_eq!(
            AccountEntryError::from_response(status, body),
            AccountEntryError::Unavailable,
            "{status} {body}"
        );
    }
}
