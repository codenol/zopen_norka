//! One-time links: issuing, finding, and the single-use rule.

use super::*;
use crate::accounts::{hash_token, AccountsError, OneTimePurpose};

#[test]
fn a_token_is_found_while_its_window_is_open() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let issued = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");

    let found = db
        .find_valid_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 10)
        .expect("find")
        .expect("a token");
    assert_eq!(found.id, issued.id);
    assert_eq!(found.user_id, "u1");
    assert_eq!(found.purpose, OneTimePurpose::PasswordReset);
    assert_eq!(found.created_at, NOW);
    assert_eq!(found.expires_at, NOW + 3600);
    assert_eq!(found.used_at, None);
    assert!(found.is_live_at(NOW + 10));
    assert_eq!(issued.expires_at, NOW + 3600);
}

#[test]
fn the_database_holds_the_hash_of_the_token_and_not_the_token() {
    let (dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .issue_one_time_token("u1", OneTimePurpose::EmailVerify, 3600, NOW)
        .expect("issue");

    assert_eq!(
        stored_one_time_hash(&db),
        hash_token(&issued.token).to_vec()
    );
    assert!(
        !appears_in(&file_bytes(&dir), &issued.token),
        "a reset link that can be read out of the database is a reset link anybody can use"
    );
}

#[test]
fn a_token_can_only_be_consumed_once() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");

    let first = db
        .consume_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 5)
        .expect("consume")
        .expect("the first use works");
    assert_eq!(first.used_at, Some(NOW + 5));
    assert_eq!(first.user_id, "u1");

    assert_eq!(
        db.consume_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 6)
            .expect("consume"),
        None,
        "the second use finds nothing to claim"
    );
    // The row stays: a password change that happened is a security event, and
    // the row is the record of which link caused it.
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM one_time_tokens"), 1);
    assert_eq!(
        db.find_valid_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 6)
            .expect("find"),
        None
    );
}

#[test]
fn an_expired_token_cannot_be_consumed() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 600, NOW)
        .expect("issue");

    assert_eq!(
        db.find_valid_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 600)
            .expect("find"),
        None,
        "the window is closed AT the expiry, not a second after it"
    );
    assert_eq!(
        db.consume_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 600)
            .expect("consume"),
        None
    );
    assert!(db
        .find_valid_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW + 599)
        .expect("find")
        .is_some());
}

#[test]
fn a_token_for_one_purpose_is_not_a_token_for_another() {
    // A reset link that verified an address, or the reverse, would be a link
    // doing something other than what its recipient was told it would do.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");

    assert_eq!(
        db.find_valid_one_time_token(&issued.token, OneTimePurpose::EmailVerify, NOW)
            .expect("find"),
        None
    );
    assert_eq!(
        db.consume_one_time_token(&issued.token, OneTimePurpose::EmailVerify, NOW)
            .expect("consume"),
        None
    );
    // And the failed attempt did not spend it.
    assert!(db
        .find_valid_one_time_token(&issued.token, OneTimePurpose::PasswordReset, NOW)
        .expect("find")
        .is_some());
}

#[test]
fn issuing_a_new_token_retires_the_previous_one() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let first = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");
    let second = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW + 30)
        .expect("issue");

    assert_eq!(
        db.find_valid_one_time_token(&first.token, OneTimePurpose::PasswordReset, NOW + 31)
            .expect("find"),
        None,
        "the older link is the one sitting in an inbox; a new one must retire it"
    );
    assert!(db
        .find_valid_one_time_token(&second.token, OneTimePurpose::PasswordReset, NOW + 31)
        .expect("find")
        .is_some());
    // Retired, not deleted: the table still shows a link was issued and
    // withdrawn, which is what an operator asking 'was there an earlier link'
    // needs.
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM one_time_tokens"), 2);
}

#[test]
fn retiring_a_password_link_leaves_a_verification_link_alone() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let verify = db
        .issue_one_time_token("u1", OneTimePurpose::EmailVerify, 3600, NOW)
        .expect("issue");
    db.issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");

    assert!(db
        .find_valid_one_time_token(&verify.token, OneTimePurpose::EmailVerify, NOW)
        .expect("find")
        .is_some());
}

#[test]
fn a_token_for_an_account_that_does_not_exist_is_refused() {
    let (_dir, db) = store();

    assert_eq!(
        db.issue_one_time_token("nobody", OneTimePurpose::PasswordReset, 3600, NOW)
            .expect_err("no such account"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
}

#[test]
fn deleting_an_account_takes_its_one_time_tokens_with_it() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.issue_one_time_token("u1", OneTimePurpose::PasswordReset, 3600, NOW)
        .expect("issue");

    db.delete_user("u1").expect("delete");

    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM one_time_tokens"), 0);
}

#[test]
fn the_sweep_removes_tokens_whose_window_has_closed() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let spent = db
        .issue_one_time_token("u1", OneTimePurpose::PasswordReset, 60, NOW)
        .expect("issue");
    db.consume_one_time_token(&spent.token, OneTimePurpose::PasswordReset, NOW + 1)
        .expect("consume");
    db.issue_one_time_token("u1", OneTimePurpose::EmailVerify, 60, NOW)
        .expect("issue");
    db.issue_one_time_token("u1", OneTimePurpose::PasswordReset, 7200, NOW)
        .expect("issue");

    // A spent token is kept until its window closes — it is the record of a
    // password change — and dropped with the rest once it no longer explains
    // anything a live token could do.
    assert_eq!(
        db.purge_expired_one_time_tokens(NOW + 59).expect("sweep"),
        0
    );
    assert_eq!(
        db.purge_expired_one_time_tokens(NOW + 60).expect("sweep"),
        2
    );
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM one_time_tokens"), 1);
}
