//! Sessions: what the database holds, what resolves, and what removes them.

use rusqlite::{params, Connection};

use super::*;
use crate::accounts::accounts_migrations::MIGRATIONS;
use crate::accounts::{hash_token, AccountsError, NewSession, META_SCHEMA_VERSION};

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

/// The bound the column has, and the two ways a client can meet it: an agent
/// string longer than the limit is kept in part rather than refused (a padded
/// header must not cost anybody their sign-in), and one that said nothing is
/// NULL rather than `''` (which would read as a client that identified itself
/// as nothing).
#[test]
fn a_long_user_agent_is_kept_in_part_and_an_empty_one_is_no_client_at_all() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    // The column's limit, written out here so that raising it is a decision
    // somebody makes on purpose rather than a constant that drifted.
    const LIMIT: usize = 512;

    let padded = format!("{}tail", "Norka-Test/1.0 ".repeat(100));
    let long = db
        .create_session(
            &NewSession::new("u1", 3600).with_client(Some(&padded), None),
            NOW,
        )
        .expect("create");
    let stored = long.session.user_agent.expect("a stored agent");
    assert_eq!(stored.chars().count(), LIMIT);
    assert!(
        padded.starts_with(&stored),
        "the HEAD of an agent string is what names the browser, so that is what is kept"
    );
    assert!(!stored.contains("tail"), "the padding is not the part kept");

    let silent = db
        .create_session(
            &NewSession::new("u1", 3600).with_client(Some("   "), None),
            NOW,
        )
        .expect("create");
    assert_eq!(
        silent.session.user_agent, None,
        "a header that named nothing is 'nothing known', which the column spells NULL"
    );
    assert_eq!(
        count(
            &db.conn(),
            "SELECT COUNT(*) FROM sessions WHERE user_agent = ''"
        ),
        0,
        "and never the empty string"
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

/// A token for the session this test plants "before" the newest migration.
/// Not a secret and not random: nothing signs in with it, it only has to name
/// the row whose survival is being checked.
const LEGACY_TOKEN: &str = "a-session-issued-before-issue-76";

/// Issue #76: recording the client on a session is an ADDITION, so a database
/// that already holds sessions has to come through the migration with every
/// row intact — including the ones written when nothing recorded a client.
///
/// That is the shape the change actually has: `user_agent` and `ip` have been
/// columns since migration 1 (step 1 of #55 declared them) and nothing had ever
/// written them, so there is no step that adds them, and a database in the
/// field is full of sessions whose two client columns are NULL. NULL is what
/// this store means by "nothing was known about the client", and the one thing
/// a migration must never do is turn it into a value somebody invented.
///
/// The database is built the way the other migration tests build one: the
/// steps BELOW the newest applied by hand, the version recorded to match, and
/// the rows of an older build already in the tables — so the reopen has real
/// work to do and a step that lost the rows would be caught here.
#[test]
fn a_session_written_before_the_newest_migration_survives_it_and_still_resolves() {
    let dir = TempDir::new("accounts-sessions-migration");
    let latest = MIGRATIONS.last().expect("a migration").version;
    let earlier = latest - 1;
    let legacy_hash = hash_token(LEGACY_TOKEN);
    {
        let conn = Connection::open(dir.join(DB_FILE)).expect("open");
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .expect("meta");
        for migration in MIGRATIONS.iter().filter(|m| m.version <= earlier) {
            conn.execute_batch(migration.sql)
                .expect("apply an earlier step");
        }
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![META_SCHEMA_VERSION, earlier.to_string()],
        )
        .expect("record the earlier version");
        // An account and a signed-in browser, as the earlier build left them.
        // The session names no client, which is every row that build wrote.
        conn.execute(
            "INSERT INTO users (id, username, display_name, hash_algo, roles, status,
                                created_at, updated_at)
             VALUES ('u1', 'alice', 'Alice', 'argon2id', '', 'active', ?1, ?1)",
            params![NOW],
        )
        .expect("a user from before the migration");
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at, last_seen_at,
                                   user_agent, ip)
             VALUES (?1, 'u1', ?2, ?3, NULL, NULL, NULL)",
            params![&legacy_hash[..], NOW, NOW + 3600],
        )
        .expect("a session from before the migration");
    }

    let db = reopen(&dir);

    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("version"),
        Some(latest.to_string()),
        "the reopen is what applies the pending step"
    );
    let carried = db
        .resolve_session(LEGACY_TOKEN, NOW + 1)
        .expect("resolve")
        .expect("the session written before the migration must still be there");
    assert_eq!(carried.user_id, "u1");
    assert_eq!(carried.created_at, NOW);
    assert_eq!(carried.expires_at, NOW + 3600);
    assert_eq!(
        carried.user_agent, None,
        "NULL is 'nothing was known'; a migration that filled it in would be inventing a client"
    );
    assert_eq!(carried.ip, None);

    // And the rows written now sit beside it, carrying what the request knew:
    // one table, both shapes, no rewriting of the old row.
    let issued = db
        .create_session(
            &NewSession::new("u1", 3600).with_client(Some("Norka/0.9"), Some("203.0.113.7")),
            NOW,
        )
        .expect("create");
    assert_eq!(issued.session.user_agent.as_deref(), Some("Norka/0.9"));
    assert_eq!(count(&db.conn(), "SELECT COUNT(*) FROM sessions"), 2);
    assert_eq!(
        count(
            &db.conn(),
            "SELECT COUNT(*) FROM sessions WHERE user_agent IS NULL AND ip IS NULL"
        ),
        1,
        "the carried row is untouched, and only the new one has a client"
    );
}
