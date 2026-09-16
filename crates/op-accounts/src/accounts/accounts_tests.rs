//! What the account store promises, driven through its public API — and, where
//! the promise is about what is NOT in the database, driven through the
//! database FILE.
//!
//! The tests that matter most here are the ones that would pass on a store with
//! no security at all: a password "is stored" whether it is hashed or not, and
//! a token "resolves" whether the row holds the token or its hash. So the
//! claims about what is stored are checked against the bytes on disk and
//! against the columns, not against the API's own answers.
//!
//! Split by concern, one file each; the fixtures they share live here.
//! `:memory:` is deliberately not used: the store opens with
//! `journal_mode=WAL` and refuses to continue if it did not get it, and WAL is
//! a property of a database FILE — an in-memory database stays in `memory`
//! journal mode, so a test on one would pass while proving nothing about the
//! mode the product runs in.

use rusqlite::Connection;

use super::*;
use crate::test_dir::TempDir;

/// A moment every test can reason about.
///
/// Nothing in the store reads the clock itself — every operation that compares
/// against time takes `now` — so a test can put any operation before or after
/// any expiry without waiting for one.
const NOW: i64 = 1_700_000_000;

/// A password long and distinctive enough that "is this string anywhere in the
/// file" is a question the tests can answer without false positives.
const PASSWORD: &str = "correct-horse-battery-staple-norka-7";

/// A fresh store, in a directory that deletes itself.
fn store() -> (TempDir, AccountsDb) {
    let dir = TempDir::new("accounts");
    let db = AccountsDb::open(dir.path()).expect("open the account store");
    (dir, db)
}

/// The store for a directory that already holds a database.
fn reopen(dir: &TempDir) -> AccountsDb {
    AccountsDb::open(dir.path()).expect("reopen the account store")
}

/// An account that can sign in.
fn active_user(db: &AccountsDb, id: &str, username: &str) -> User {
    db.create_user(
        &NewUser::active(username, "Test Person", PASSWORD).with_id(id),
        NOW,
    )
    .expect("create an active account")
}

/// An account created from an invite: it exists and cannot sign in yet.
fn invited_user(db: &AccountsDb, id: &str, username: &str) -> User {
    db.create_user(
        &NewUser::invited(username, "Invited Person").with_id(id),
        NOW,
    )
    .expect("create an invited account")
}

/// One attempt to sign in, with the budgets a deployment gets when it says
/// nothing.
///
/// The tests here are about WHICH name and password open an account, not about
/// how often they may be tried, so they go through this and let the throttle
/// take its defaults. The throttle's own tests (`throttle.rs`) call the store
/// directly, because the numbers ARE what they are testing.
fn sign_in(
    db: &AccountsDb,
    username: &str,
    password: &str,
    now: i64,
) -> Result<crate::accounts::SignInOutcome, AccountsError> {
    db.authenticate(
        &crate::accounts::SignInAttempt::new(username, password),
        &crate::accounts::SignInLimits::default(),
        now,
    )
}

/// Every object the schema holds, by name.
fn schema_objects(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare");
    let names = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("collect");
    names
}

/// The column names of one table, in the order the schema declares them.
fn columns_of(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare");
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("collect");
    names
}

/// One scalar as an integer.
fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).expect("count")
}

/// Every byte the store has on disk, WAL included.
///
/// The WAL matters: a write that has not been checkpointed lives only in
/// `accounts.db-wal`, so a test that scanned the main file alone would pass
/// while the value it is looking for sat one file over.
fn file_bytes(dir: &TempDir) -> Vec<u8> {
    let mut bytes = std::fs::read(dir.join(DB_FILE)).expect("read the database file");
    if let Ok(wal) = std::fs::read(dir.join("accounts.db-wal")) {
        bytes.extend_from_slice(&wal);
    }
    bytes
}

/// Whether `needle` appears anywhere in `haystack`.
fn appears_in(haystack: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    !needle.is_empty() && haystack.windows(needle.len()).any(|part| part == needle)
}

/// The stored token hash of a session, straight out of the table.
fn stored_session_hash(db: &AccountsDb) -> Vec<u8> {
    db.conn()
        .query_row("SELECT token_hash FROM sessions LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("a session row")
}

/// The stored token hash of a one-time token, straight out of the table.
fn stored_one_time_hash(db: &AccountsDb) -> Vec<u8> {
    db.conn()
        .query_row(
            "SELECT token_hash FROM one_time_tokens LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("a one-time token row")
}

/// The stored token hash of an invite, straight out of the table.
fn stored_invite_hash(db: &AccountsDb) -> Vec<u8> {
    db.conn()
        .query_row("SELECT token_hash FROM invites LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("an invite row")
}

mod invites;
mod passwords;
mod policy;
mod schema;
mod sessions;
mod signin;
mod throttle;
mod tokens;
mod users;
