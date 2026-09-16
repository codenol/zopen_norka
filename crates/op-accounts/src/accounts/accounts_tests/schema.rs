//! The schema, the migration runner, and what the store does with a row it
//! cannot read.

use rusqlite::{params, Connection};

use super::*;
use crate::accounts::accounts_migrations::MIGRATIONS;
use crate::accounts::{AccountsError, META_SCHEMA_VERSION};

#[test]
fn a_fresh_directory_gets_the_whole_schema_and_records_its_version() {
    let (dir, db) = store();

    assert_eq!(
        schema_objects(&db.conn()),
        vec![
            "invites",
            "meta",
            "one_time_tokens",
            "sessions",
            // The sign-in budget (issue #77): one row per name or address whose
            // failures are still being counted.
            "sign_in_attempts",
            // SQLite's own bookkeeping for `one_time_tokens.id`, which is
            // AUTOINCREMENT so that an id a log line names is never handed to a
            // different event later.
            "sqlite_sequence",
            "users",
        ],
        "every table the store reads must exist after the first open"
    );
    assert_eq!(
        columns_of(&db.conn(), "users"),
        vec![
            "id",
            "username",
            "display_name",
            "email",
            "email_verified_at",
            "password_hash",
            "hash_algo",
            "roles",
            "status",
            "created_at",
            "updated_at",
            "last_seen_at",
        ]
    );
    assert_eq!(
        columns_of(&db.conn(), "invites"),
        vec![
            "token_hash",
            "email",
            "roles",
            "created_by",
            "created_at",
            "expires_at",
            "accepted_by",
            "accepted_at",
        ]
    );
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("read the version"),
        Some(MIGRATIONS.last().expect("a migration").version.to_string())
    );
    assert_eq!(db.dir(), dir.path());
    assert!(
        dir.join(DB_FILE).is_file(),
        "the database file is in the directory it was given"
    );
}

#[test]
fn the_migration_list_is_consecutive_from_one() {
    // The runner takes "every version above the recorded one", so a gap would
    // skip a step silently and a repeat would run one twice. Cheap to state,
    // and it is the property that lets the version number mean anything.
    let versions: Vec<i64> = MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .collect();
    let expected: Vec<i64> = (1..=versions.len() as i64).collect();
    assert_eq!(versions, expected);
}

#[test]
fn reopening_a_migrated_store_runs_nothing_again_and_keeps_what_was_written() {
    let (dir, db) = store();
    let user = active_user(&db, "u1", "alice");
    let version = db.meta(META_SCHEMA_VERSION).expect("version");
    drop(db);

    // A second run of migration 1 would fail here — `CREATE TABLE users` on a
    // database that has one — so an `expect` on the reopen is itself the proof
    // that nothing was applied twice.
    let reopened = reopen(&dir);

    assert_eq!(
        reopened.meta(META_SCHEMA_VERSION).expect("version"),
        version
    );
    assert_eq!(reopened.count_users().expect("count"), 1);
    assert_eq!(
        reopened.find_user_by_id("u1").expect("find"),
        Some(user),
        "a migration that ran again would have replaced the row it wrote"
    );
}

#[test]
fn a_store_left_at_an_earlier_version_is_brought_to_the_latest() {
    let dir = TempDir::new("accounts-earlier");
    let latest = MIGRATIONS.last().expect("a migration").version;
    let earlier = latest - 1;
    {
        // A database as an OLDER BUILD of this store left it: the migrations
        // below the newest applied, and the version recorded to match.
        //
        // With two migrations in the list, `earlier` is 1 and this is a
        // database with the tables of #55 and rows in them, which is the shape
        // a deployment upgrading across the sign-in budget (issue #77) actually
        // has: the new step must land on a database that is already in use. The
        // assertions below are written for whichever list is current.
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
    }

    let db = reopen(&dir);

    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("version"),
        Some(latest.to_string())
    );
    assert!(schema_objects(&db.conn()).contains(&"users".to_string()));
    // Present is not the same as usable: the tables have to be the ones this
    // build writes.
    let user = active_user(&db, "u1", "alice");
    assert_eq!(db.find_user_by_username("alice").expect("find"), Some(user));
}

#[test]
fn a_store_with_no_recorded_version_is_migrated_from_the_start() {
    let dir = TempDir::new("accounts-unversioned");
    {
        // A `meta` table and nothing else: what an interrupted first open, or a
        // file somebody copied in by hand, looks like.
        let conn = Connection::open(dir.join(DB_FILE)).expect("open");
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .expect("meta");
    }

    let db = reopen(&dir);

    assert!(
        schema_objects(&db.conn()).contains(&"users".to_string()),
        "an unrecorded version reads as 0, so every step is pending"
    );
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("version"),
        Some(MIGRATIONS.last().expect("a migration").version.to_string())
    );
}

#[test]
fn an_unknown_status_in_a_row_is_refused_rather_than_defaulted() {
    let (_dir, db) = store();
    {
        let conn = db.conn();
        // The CHECK refuses an unknown status on the way IN. It is switched off
        // to plant the row an OLDER binary meets when a NEWER one added a
        // status: the read has to fail closed by itself, because that is the
        // direction the failure travels in the field.
        conn.execute_batch("PRAGMA ignore_check_constraints = ON")
            .expect("pragma");
        conn.execute(
            "INSERT INTO users (id, username, display_name, password_hash, hash_algo,
                                roles, status, created_at, updated_at)
             VALUES ('u1', 'alice', 'Alice', NULL, 'argon2id', '', 'frozen', 1, 1)",
            [],
        )
        .expect("plant the row");
    }

    assert_eq!(
        db.find_user_by_id("u1").expect_err("refuse the row"),
        AccountsError::UnknownStatus {
            status: "frozen".to_string()
        }
    );
    assert!(
        db.list_users(10, 0).is_err(),
        "a page that cannot be read in full is not a page with one row missing"
    );
    // The count never decodes a row, so it still answers — which is what makes
    // the failure above about the row rather than about the table.
    assert_eq!(db.count_users().expect("count"), 1);
}

#[test]
fn an_unknown_purpose_in_a_row_is_refused_rather_than_defaulted() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let (_token, hash) = crate::accounts::issue_token().expect("issue");
    {
        let conn = db.conn();
        conn.execute_batch("PRAGMA ignore_check_constraints = ON")
            .expect("pragma");
        conn.execute(
            "INSERT INTO one_time_tokens (user_id, purpose, token_hash, created_at, expires_at,
                                          used_at)
             VALUES ('u1', 'magic_link', ?1, 1, 2, NULL)",
            params![&hash[..]],
        )
        .expect("plant the row");
        // Read through the decoder directly. The store's own reads filter by
        // purpose in SQL — a link for one purpose must never be found by
        // another — so this row is one only a future reader, or an operator's
        // repair, can meet.
        let decoded = conn
            .query_row(
                "SELECT id, user_id, purpose, created_at, expires_at, used_at FROM one_time_tokens",
                [],
                crate::accounts::accounts_model::one_time_token_from_row,
            )
            .map_err(AccountsError::from);
        assert_eq!(
            decoded.expect_err("refuse the row"),
            AccountsError::UnknownPurpose {
                purpose: "magic_link".to_string()
            }
        );
    }
}

#[test]
fn a_data_directory_has_to_be_actually_set() {
    // `open_from_env` is two lines over this; the parsing is what has a rule
    // worth pinning, and it is checked here rather than through the process
    // environment, which the tests of a whole binary share.
    assert_eq!(configured_data_dir(None), None);
    assert_eq!(configured_data_dir(Some("")), None);
    assert_eq!(configured_data_dir(Some("   ")), None);
    assert_eq!(
        configured_data_dir(Some(" /var/lib/norka ")),
        Some(std::path::PathBuf::from("/var/lib/norka")),
        "a padded path is the path somebody meant"
    );
}

#[test]
fn the_data_directory_variable_is_the_one_the_deployment_already_uses() {
    // Named in this module rather than imported from the tenant store, so the
    // accounts store does not depend on the web layer. Named TWICE, then, and
    // this is what keeps the two names one setting: a deployment that sets the
    // variable must find both stores in the same place.
    //
    // The other half of that promise — that the tenant store's copy really
    // spells it the same way — can no longer be asked from here: the store
    // lives in its own crate (issue #75) and must not depend on the daemon to
    // answer a question about a name. It is pinned beside the tenant store
    // instead (`op-host-services::accounts_data_dir_tests`). What is checked
    // here is the literal itself, which is what a rename has to get past.
    assert_eq!(crate::accounts::DATA_DIR_ENV, "OPENPENCIL_ONLINE_DATA_DIR");
}

#[test]
fn the_schema_refuses_a_row_it_could_not_read_back() {
    // The guards are in the schema, not only in the code that writes: a hand
    // repair, an import, or a future writer that forgets one of them meets the
    // same refusal. Each value here is one that would make a row mean something
    // its readers cannot interpret.
    let (_dir, db) = store();
    let conn = db.conn();
    let insert = |status: &str, roles: &str| {
        conn.execute(
            "INSERT INTO users (id, username, display_name, hash_algo, roles, status,
                                created_at, updated_at)
             VALUES ('u1', 'alice', 'Alice', 'argon2id', ?1, ?2, 1, 1)",
            params![roles, status],
        )
    };

    assert!(
        insert("frozen", "").is_err(),
        "a status this build did not name"
    );
    assert!(
        insert("active", ",editor").is_err(),
        "a role list with an empty entry, which the decoder would silently drop"
    );
    assert!(
        insert("active", "editor,,admin").is_err(),
        "a hole in the middle of the list"
    );
    assert!(insert("active", "editor").is_ok());

    // The token columns hold 32 bytes or the row is refused: a truncated hash
    // is a credential nothing can ever match, and it would otherwise be stored
    // without complaint.
    let short = conn.execute(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
         VALUES (?1, 'u1', 1, 2)",
        params![&[0u8; 16][..]],
    );
    assert!(short.is_err(), "a hash that is not SHA-256");
    let right = conn.execute(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
         VALUES (?1, 'u1', 1, 2)",
        params![&[0u8; 32][..]],
    );
    assert!(right.is_ok());
}

#[test]
fn one_account_per_username_is_enforced_by_the_database_itself() {
    // The store checks nothing before it inserts — the uniqueness is the index,
    // and this proves the index is collated the way the schema says. A test
    // through the API alone would pass against a case-sensitive column, which
    // is the mistake it exists to catch.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let conn = db.conn();
    let error = conn
        .execute(
            "INSERT INTO users (id, username, display_name, hash_algo, roles, status,
                                created_at, updated_at)
             VALUES ('u2', 'ALICE', 'Other', 'argon2id', '', 'active', 1, 1)",
            [],
        )
        .expect_err("the same name in another case");
    assert!(
        error.to_string().contains("username"),
        "the constraint names the column it refused: {error}"
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM users"), 1);
}
