//! Sessions: what the database holds, what resolves, and what removes them.

use super::*;
use crate::accounts::{hash_token, AccountsError, NewSession};

/// A session for a freshly created account, and the token that opens it.
fn issue(db: &AccountsDb, user_id: &str, ttl_secs: i64) -> String {
    active_user(db, user_id, user_id);
    db.create_session(&NewSession::new(user_id, ttl_secs), NOW)
        .expect("create a session")
        .token
}

#[test]
fn a_session_resolves_with_the_token_it_was_issued_with() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let issued = db
        .create_session(&NewSession::new("u1", 3600), NOW)
        .expect("create");

    assert_eq!(issued.session.user_id, "u1");
    assert_eq!(issued.session.created_at, NOW);
    assert_eq!(issued.session.expires_at, NOW + 3600);
    assert_eq!(issued.session.last_seen_at, None);
    assert!(issued.session.is_live_at(NOW));
    assert_eq!(
        db.resolve_session(&issued.token, NOW).expect("resolve"),
        Some(issued.session.clone())
    );
    // One second before the expiry it works; at the expiry it does not. The
    // boundary is stated once, here, because "expires at t" reads two ways.
    assert!(db
        .resolve_session(&issued.token, NOW + 3599)
        .expect("resolve")
        .is_some());
    assert_eq!(
        db.resolve_session(&issued.token, NOW + 3600)
            .expect("resolve"),
        None
    );
}

#[test]
fn a_token_that_was_never_issued_resolves_to_nothing() {
    let (_dir, db) = store();
    issue(&db, "u1", 3600);

    assert_eq!(
        db.resolve_session("not-a-token", NOW).expect("resolve"),
        None
    );
    assert_eq!(db.resolve_session("", NOW).expect("resolve"), None);
}

#[test]
fn the_database_holds_the_hash_of_the_token_and_not_the_token() {
    let (dir, db) = store();
    let token = issue(&db, "u1", 3600);

    assert_eq!(
        stored_session_hash(&db),
        hash_token(&token).to_vec(),
        "the stored value is SHA-256 of the token the client holds"
    );
    assert!(
        !appears_in(&file_bytes(&dir), &token),
        "the token must not be anywhere in accounts.db or its WAL"
    );
}

#[test]
fn an_expired_session_does_not_resolve() {
    let (_dir, db) = store();
    let token = issue(&db, "u1", 60);

    assert_eq!(db.resolve_session(&token, NOW + 60).expect("resolve"), None);
    // The row is still there: expiry is enforced on the read, and removing the
    // row is the sweep's job rather than a side effect of somebody presenting a
    // stale cookie.
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM sessions"), 1);
}

#[test]
fn touching_a_session_records_when_it_was_last_seen() {
    let (_dir, db) = store();
    let token = issue(&db, "u1", 3600);

    assert!(db.touch_session(&token, NOW + 10).expect("touch"));
    assert_eq!(
        db.resolve_session(&token, NOW + 10)
            .expect("resolve")
            .expect("a session")
            .last_seen_at,
        Some(NOW + 10)
    );
    assert!(!db.touch_session("not-a-token", NOW).expect("touch"));
}

#[test]
fn a_revoked_session_stops_resolving_and_the_row_is_gone() {
    let (_dir, db) = store();
    let token = issue(&db, "u1", 3600);

    assert!(db.revoke_session(&token).expect("revoke"));
    assert_eq!(db.resolve_session(&token, NOW).expect("resolve"), None);
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM sessions"), 0);
    assert!(!db.revoke_session(&token).expect("revoke again"));
}

#[test]
fn ending_every_session_of_an_account_leaves_the_others_alone() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    active_user(&db, "u2", "bob");
    let alice = db
        .create_session(&NewSession::new("u1", 3600), NOW)
        .expect("create")
        .token;
    let alice_again = db
        .create_session(&NewSession::new("u1", 3600), NOW)
        .expect("create")
        .token;
    let bob = db
        .create_session(&NewSession::new("u2", 3600), NOW)
        .expect("create")
        .token;

    assert_eq!(db.revoke_user_sessions("u1").expect("revoke"), 2);

    assert_eq!(db.resolve_session(&alice, NOW).expect("resolve"), None);
    assert_eq!(
        db.resolve_session(&alice_again, NOW).expect("resolve"),
        None
    );
    assert!(
        db.resolve_session(&bob, NOW).expect("resolve").is_some(),
        "another account's session is not this account's to end"
    );
}

#[test]
fn deleting_an_account_takes_its_sessions_with_it() {
    let (_dir, db) = store();
    let token = issue(&db, "u1", 3600);
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM sessions"), 1);

    db.delete_user("u1").expect("delete");

    assert_eq!(
        count(&db.conn(), "SELECT COUNT(*) FROM sessions"),
        0,
        "the cascade is why `foreign_keys=ON` is set for the life of the connection"
    );
    assert_eq!(
        db.resolve_session(&token, NOW).expect("resolve"),
        None,
        "a deleted account's browser is not signed in"
    );
}

#[test]
fn a_session_for_an_account_that_does_not_exist_is_refused() {
    let (_dir, db) = store();

    assert_eq!(
        db.create_session(&NewSession::new("nobody", 3600), NOW)
            .expect_err("no such account"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
}

#[test]
fn a_session_remembers_what_the_request_knew_about_its_client() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let issued = db
        .create_session(
            &NewSession::new("u1", 3600).with_client(Some("Norka/0.9"), Some("203.0.113.7")),
            NOW,
        )
        .expect("create");

    assert_eq!(issued.session.user_agent.as_deref(), Some("Norka/0.9"));
    assert_eq!(issued.session.ip.as_deref(), Some("203.0.113.7"));
    assert_eq!(
        db.resolve_session(&issued.token, NOW).expect("resolve"),
        Some(issued.session)
    );
}

#[test]
fn the_sweep_removes_the_expired_sessions_and_keeps_the_live_ones() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.create_session(&NewSession::new("u1", 60), NOW)
        .expect("create");
    db.create_session(&NewSession::new("u1", 7200), NOW)
        .expect("create");

    assert_eq!(db.purge_expired_sessions(NOW + 60).expect("sweep"), 1);

    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM sessions"), 1);
    assert_eq!(db.purge_expired_sessions(NOW + 60).expect("sweep"), 0);
}
