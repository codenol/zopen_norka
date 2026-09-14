//! The default lifetimes, checked by using them: a number in a constant is a
//! claim about how long something lives, and these are the claims.

use super::*;
use crate::accounts::{
    NewInvite, NewSession, OneTimePurpose, EMAIL_VERIFY_TTL_SECS, INVITE_TTL_SECS,
    PASSWORD_RESET_TTL_SECS, SESSION_TTL_SECS,
};

/// The moment a thing created at [`NOW`] stops being accepted.
///
/// Every one of these lifetimes means "live before this, dead at it": the
/// boundary is stated once, in the operations, and these tests read it from the
/// outside so a constant and the code that uses it cannot drift.
fn dead_at(ttl: i64) -> i64 {
    NOW + ttl
}

#[test]
fn a_session_lasts_the_stated_month() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let issued = db
        .create_session(&NewSession::new("u1", SESSION_TTL_SECS), NOW)
        .expect("create");

    assert_eq!(issued.session.expires_at, dead_at(SESSION_TTL_SECS));
    assert!(db
        .resolve_session(&issued.token, dead_at(SESSION_TTL_SECS) - 1)
        .expect("resolve")
        .is_some());
    assert_eq!(
        db.resolve_session(&issued.token, dead_at(SESSION_TTL_SECS))
            .expect("resolve"),
        None
    );
}

#[test]
fn a_one_time_link_lasts_what_its_purpose_says() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    for (purpose, ttl) in [
        (OneTimePurpose::PasswordReset, PASSWORD_RESET_TTL_SECS),
        (OneTimePurpose::EmailVerify, EMAIL_VERIFY_TTL_SECS),
    ] {
        assert_eq!(purpose.default_ttl_secs(), ttl);
        let issued = db
            .issue_one_time_token("u1", purpose, purpose.default_ttl_secs(), NOW)
            .expect("issue");
        assert_eq!(issued.expires_at, dead_at(ttl));
        assert!(db
            .find_valid_one_time_token(&issued.token, purpose, dead_at(ttl) - 1)
            .expect("find")
            .is_some());
        assert_eq!(
            db.find_valid_one_time_token(&issued.token, purpose, dead_at(ttl))
                .expect("find"),
            None
        );
    }

    // The two differ, which is why the default lives on the purpose rather than
    // being one number every caller reuses: a reset link is a password with
    // extra steps, a verification link only proves an address.
    assert!(PASSWORD_RESET_TTL_SECS < EMAIL_VERIFY_TTL_SECS);
}

#[test]
fn an_invite_lasts_the_stated_week() {
    let (_dir, db) = store();
    invited_user(&db, "u1", "bob");

    let issued = db
        .create_invite(&NewInvite::new(&[], None, INVITE_TTL_SECS), NOW)
        .expect("issue");
    let with_ttl = |accepted_by: &str, at: i64| db.redeem_invite(&issued.token, accepted_by, at);

    assert_eq!(issued.invite.expires_at, dead_at(INVITE_TTL_SECS));
    assert!(issued.invite.is_redeemable_at(dead_at(INVITE_TTL_SECS) - 1));
    assert_eq!(
        with_ttl("u1", dead_at(INVITE_TTL_SECS)),
        Err(crate::accounts::AccountsError::InviteExpired)
    );
}

#[test]
fn the_lifetimes_are_ordered_the_way_the_policy_module_says() {
    // Stated as an ordering rather than as four numbers: whoever changes one of
    // them has to say why a reset link now outlives an invite.
    assert!(0 < PASSWORD_RESET_TTL_SECS);
    assert!(PASSWORD_RESET_TTL_SECS < EMAIL_VERIFY_TTL_SECS);
    assert!(EMAIL_VERIFY_TTL_SECS < INVITE_TTL_SECS);
    assert!(INVITE_TTL_SECS < SESSION_TTL_SECS);
}
