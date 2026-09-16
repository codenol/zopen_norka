//! Signing in: which name and password pairs open an account, and what is
//! recorded when they do — and when they do not.

use super::*;
use crate::accounts::{AccountsError, NewUser, SignInOutcome, UserStatus};

#[test]
fn the_right_password_signs_an_active_account_in() {
    let (_dir, db) = store();
    let created = active_user(&db, "u1", "alice");

    let outcome = sign_in(&db, "alice", PASSWORD, NOW + 5).expect("authenticate");

    let SignInOutcome::SignedIn(user) = outcome else {
        panic!("expected a signed-in account, got {outcome:?}");
    };
    assert_eq!(user.id, created.id);
    assert_eq!(user.username, "alice");
    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(
        user.last_seen_at,
        Some(NOW + 5),
        "the value handed back is the row as it now stands, not as it was"
    );
    let stored = db.find_user_by_id("u1").expect("find").expect("a row");
    assert_eq!(stored.last_seen_at, Some(NOW + 5));
    assert_eq!(
        stored.updated_at, NOW,
        "signing in is not a change to the account record"
    );
}

#[test]
fn a_name_is_matched_whatever_case_it_is_typed_in() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    // The column's collation, not a lowercase copy of the key: the person may
    // type their own name however they like.
    for typed in ["alice", "ALICE", "  Alice  "] {
        assert!(
            matches!(
                sign_in(&db, typed, PASSWORD, NOW).expect("authenticate"),
                SignInOutcome::SignedIn(_)
            ),
            "{typed} should sign in"
        );
    }
}

#[test]
fn a_wrong_password_and_a_name_that_matches_nothing_answer_the_same_way() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    // One answer for both. A login form built on this cannot be talked into
    // reporting which names exist, because the store never worked it out.
    assert_eq!(
        sign_in(&db, "alice", "not-the-password", NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
    assert_eq!(
        sign_in(&db, "nobody", PASSWORD, NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
    assert_eq!(
        sign_in(&db, "alice", "", NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
}

#[test]
fn an_account_with_no_password_cannot_sign_in() {
    let (_dir, db) = store();
    invited_user(&db, "u1", "bob");

    // The invite has not been accepted: the account exists and there is nothing
    // to check a password against, so every password is equally wrong.
    assert_eq!(
        sign_in(&db, "bob", PASSWORD, NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
    db.set_password("u1", PASSWORD, NOW)
        .expect("set a password");
    assert!(
        matches!(
            sign_in(&db, "bob", PASSWORD, NOW).expect("authenticate"),
            SignInOutcome::SignedIn(_)
        ),
        "once the account has a password, it signs in"
    );
}

#[test]
fn a_blocked_account_is_told_apart_only_after_its_password_is_right() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.set_status("u1", UserStatus::Disabled, NOW)
        .expect("disable");

    // With the password: the caller has proved it holds the account, so the
    // reason is theirs to know — it is the one thing they can act on.
    assert_eq!(
        sign_in(&db, "alice", PASSWORD, NOW).expect("authenticate"),
        SignInOutcome::Blocked(UserStatus::Disabled)
    );
    // Without it: the same answer a name that matches nothing gets. Checking
    // the status first would have made this a way to ask whether an account
    // exists and is disabled.
    assert_eq!(
        sign_in(&db, "alice", "not-the-password", NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
    assert_eq!(
        sign_in(&db, "nobody", "not-the-password", NOW).expect("authenticate"),
        SignInOutcome::Rejected
    );
}

#[test]
fn a_disabled_account_is_not_recorded_as_seen() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.set_status("u1", UserStatus::Disabled, NOW)
        .expect("disable");

    assert_eq!(
        sign_in(&db, "alice", PASSWORD, NOW + 100).expect("authenticate"),
        SignInOutcome::Blocked(UserStatus::Disabled)
    );
    assert_eq!(
        db.find_user_by_id("u1")
            .expect("find")
            .expect("a row")
            .last_seen_at,
        None,
        "a refused attempt is not the account being seen"
    );
}

#[test]
fn an_orphaned_account_is_blocked_too() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.set_status("u1", UserStatus::Orphan, NOW)
        .expect("orphan");

    assert_eq!(
        sign_in(&db, "alice", PASSWORD, NOW).expect("authenticate"),
        SignInOutcome::Blocked(UserStatus::Orphan)
    );
}

#[test]
fn a_failed_attempt_changes_nothing() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    for (name, password) in [("alice", "wrong"), ("nobody", PASSWORD)] {
        assert_eq!(
            sign_in(&db, name, password, NOW + 50).expect("authenticate"),
            SignInOutcome::Rejected
        );
    }

    let stored = db.find_user_by_id("u1").expect("find").expect("a row");
    assert_eq!(stored.last_seen_at, None);
    assert_eq!(stored.updated_at, NOW);
}

#[test]
fn an_account_whose_hash_cannot_be_read_is_a_fault_rather_than_a_rejection() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    {
        // A row a hand repair left unreadable. It must not read as "wrong
        // password": the owner of that account would be told to try again,
        // forever, while the fault is on this side.
        let conn = db.conn();
        conn.execute(
            "UPDATE users SET password_hash = 'truncated' WHERE id = 'u1'",
            [],
        )
        .expect("corrupt the row");
    }

    let error = sign_in(&db, "alice", PASSWORD, NOW).expect_err("a hash this build cannot read");
    assert!(matches!(error, AccountsError::PasswordHash(_)), "{error:?}");
}

#[test]
fn signing_in_does_not_need_an_account_to_have_been_seen_before() {
    let (_dir, db) = store();
    db.create_user(
        &NewUser::active("someone", "Someone", PASSWORD).with_id("u1"),
        NOW,
    )
    .expect("create");

    let outcome = sign_in(&db, "Someone", PASSWORD, NOW + 1).expect("authenticate");
    let SignInOutcome::SignedIn(user) = outcome else {
        panic!("expected a signed-in account");
    };
    // Case-insensitively, and the row's other columns are untouched: the
    // returned user is the whole account, which is what a route hands to the
    // identity layer.
    assert_eq!(user.display_name, "Someone");
    assert_eq!(user.email, None);
    assert_eq!(user.roles, Vec::<String>::new());
}
