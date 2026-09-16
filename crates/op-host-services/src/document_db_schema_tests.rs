//! The schema a database created by the migrations actually has (issue #57).
//!
//! `document_db_tests` drives the store: the migrations, the one-time JSON
//! import, the operations. These say what the SCHEMA is once every step has run
//! — the table list, and the one table this build's migrations declare twice
//! (`comments`, which migration 3's rebuild has to write a second time). A
//! schema that cannot be reasoned about breaks on the next change, so the facts
//! here are pinned rather than described: the exact table list, the definition
//! `sqlite_master` records for `comments`, and the agreement between its two
//! declarations.
//!
//! A sibling module rather than more of `document_db_tests`: that file is at the
//! 800-line cap, and these are about the MIGRATION LIST rather than about the
//! store that runs it.

use super::*;
use crate::document_test_dir::TempDir;

/// The version whose declaration of `comments` is the live one.
///
/// Named here, and checked below rather than assumed: the live declaration is
/// the one in the LAST migration that declares the table — the copy a database
/// at the newest version has, whether it was created fresh or carried there by
/// a rebuild. If a later migration rebuilds `comments`, its declaration becomes
/// the live one and this number moves with it, which is the decision these
/// tests exist to hold.
const LIVE_COMMENTS_DECLARATION: i64 = 3;

/// Every table a database these migrations created holds, exactly.
///
/// `sqlite_sequence` is SQLite's own bookkeeping for the AUTOINCREMENT primary
/// keys below rather than a table this schema declares, and it is listed for the
/// reason the accounts schema lists it: an exact list is what makes a leftover
/// from a rebuild visible.
const TABLES: &[&str] = &[
    "analytics_assets",
    "comment_threads",
    "comments",
    "documents",
    "last_opened",
    "meta",
    "section_properties",
    "sqlite_sequence",
];

/// The live shape of `comments`: name, type, NOT NULL, part of the primary key —
/// in the order the schema declares them.
const COMMENTS_COLUMNS: &[(&str, &str, bool, bool)] = &[
    ("id", "INTEGER", false, true),
    ("thread_id", "INTEGER", true, false),
    ("author_role", "TEXT", false, false),
    ("author_id", "TEXT", false, false),
    ("author_name", "TEXT", true, false),
    ("body", "TEXT", true, false),
    ("created_at", "INTEGER", true, false),
];

/// The declaration of `comments` written by migration `version`, cut out of the
/// migration list the way SQLite records a statement it has run.
///
/// Read from the migration rather than copied into a fixture: what is compared
/// below is the text in the DATABASE against the text in the MIGRATION, and a
/// fixture of my own would compare the database with itself.
fn declared_comments_ddl(version: i64) -> String {
    let sql = MIGRATIONS
        .iter()
        .find(|migration| migration.version == version)
        .unwrap_or_else(|| panic!("no migration {version}"))
        .sql;
    let start = sql
        .find("CREATE TABLE comments (")
        .unwrap_or_else(|| panic!("migration {version} does not declare comments"));
    // To the bracket that closes the declaration, which is where the statement
    // ends. Counting brackets is exact here: the only other pair in the
    // statement is `comment_threads (id)` in the foreign key.
    let mut depth = 0usize;
    for (offset, character) in sql[start..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return sql[start..start + offset + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("migration {version}'s declaration of comments is never closed");
}

/// The newest schema version this build knows, read from the list rather than
/// written down: a test that names the version by hand has to be edited by every
/// migration.
fn newest_schema_version() -> String {
    MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .max()
        .expect("at least one migration")
        .to_string()
}

/// Every version whose SQL declares `comments`, in list order.
fn comments_declarations() -> Vec<i64> {
    MIGRATIONS
        .iter()
        .filter(|migration| migration.sql.contains("CREATE TABLE comments ("))
        .map(|migration| migration.version)
        .collect()
}

/// The declaration `comments` has in `conn`, as SQLite recorded it.
fn recorded_comments_ddl(conn: &Connection) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'comments'",
        [],
        |row| row.get(0),
    )
    .expect("comments is in the database")
}

/// Every object of one kind in the schema, by name, in a stable order.
fn objects_of_type(conn: &Connection, kind: &str) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")
        .expect("prepare");
    statement
        .query_map([kind], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("collect")
}

/// One table's columns as the database describes them: name, type, NOT NULL,
/// primary key.
fn columns_of(conn: &Connection, table: &str) -> Vec<(String, String, bool, bool)> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare");
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, i64>(5)? != 0,
            ))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect")
}

/// A declaration with its SQL comments and its whitespace removed — what the
/// database would be left with if the two copies were written by different
/// people.
///
/// The comments are the only difference between the two declarations of
/// `comments` today, and they are worth keeping apart from the shape: a comment
/// explains a column, it does not declare it.
fn shape(ddl: &str) -> String {
    ddl.lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A database as an OLDER build of this store left it: every migration up to
/// `through` applied, and the version recorded to match.
///
/// The same fixture `document_db_tests` builds for the upgrade paths, repeated
/// because a sibling module cannot see it.
fn database_at_version(dir: &TempDir, through: i64) -> Connection {
    let conn = Connection::open(dir.join(DB_FILE)).expect("open");
    conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .expect("meta");
    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= through)
    {
        conn.execute_batch(migration.sql)
            .expect("apply an earlier step");
    }
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)",
        params![META_SCHEMA_VERSION, through.to_string()],
    )
    .expect("record the version");
    conn
}

#[test]
fn the_migration_list_is_consecutive_from_one() {
    // The runner takes "every version above the recorded one", so a gap would
    // skip a step silently and a repeat would run one twice. The accounts store
    // states the same property of its own list, and this is the half of it that
    // belongs to the document schema.
    let versions: Vec<i64> = MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .collect();
    let expected: Vec<i64> = (1..=versions.len() as i64).collect();
    assert_eq!(versions, expected);
}

#[test]
fn a_database_the_migrations_created_has_one_comments_table_with_the_live_definition() {
    let dir = TempDir::new("comments-schema");
    let db = dir.open();
    let conn = db.conn();

    assert_eq!(
        objects_of_type(&conn, "table"),
        TABLES,
        "every table the schema claims, and nothing a rebuild left behind"
    );
    // Stated separately from the list above because it is the issue's own
    // question: one `comments`, not one per declaration of it. SQLite cannot
    // hold two objects of one name — the risk is a leftover scratch table, and
    // the two assertions below are what make that a failure rather than a
    // curiosity.
    assert_eq!(
        objects_of_type(&conn, "table")
            .iter()
            .filter(|name| name.as_str() == "comments")
            .count(),
        1
    );
    assert!(!objects_of_type(&conn, "table").contains(&"comments_saved".to_string()));
    assert!(!objects_of_type(&conn, "table").contains(&"comment_threads_v3".to_string()));

    assert_eq!(
        columns_of(&conn, "comments"),
        COMMENTS_COLUMNS
            .iter()
            .map(|(name, kind, not_null, key)| (
                name.to_string(),
                kind.to_string(),
                *not_null,
                *key
            ))
            .collect::<Vec<_>>(),
        "the definition this build claims is live, column for column"
    );
    // The index the cascade and the reads use, recreated by the same migration
    // that recreated the table.
    assert!(objects_of_type(&conn, "index").contains(&"comments_by_thread".to_string()));
    assert!(objects_of_type(&conn, "index").contains(&"comment_threads_by_document".to_string()));

    // WHICH copy is live, which is the part nothing used to say: the text
    // SQLite recorded for the table is the declaration migration 3 writes, and
    // not migration 2's.
    assert_eq!(
        recorded_comments_ddl(&conn),
        declared_comments_ddl(LIVE_COMMENTS_DECLARATION)
    );
}

#[test]
fn comments_is_declared_twice_and_both_copies_describe_the_same_table() {
    let declarations = comments_declarations();
    assert_eq!(
        declarations.first().copied(),
        Some(2),
        "the first declaration is migration 2's"
    );
    assert_eq!(
        declarations.last().copied(),
        Some(LIVE_COMMENTS_DECLARATION),
        "the live declaration is the LAST one that declares the table, which is \
         the constant this file names"
    );
    assert!(
        declarations.len() > 1,
        "this test is about the case where a rebuild declares the table again"
    );

    let first = declared_comments_ddl(declarations[0]);
    let live = declared_comments_ddl(LIVE_COMMENTS_DECLARATION);
    // Two copies of ONE table: the same shape, and different only in the
    // comments that explain the columns. If a later change to either copy made
    // them declare different tables, the rebuild would be reading a table the
    // database no longer has.
    assert_eq!(shape(&first), shape(&live));
    assert_ne!(
        first, live,
        "the copies are distinguishable, which is what makes 'which one is live' \
         a fact the database carries rather than a matter of taste"
    );
}

#[test]
fn a_database_from_before_the_rebuild_has_the_live_declaration_after_the_upgrade() {
    // A deployment that stopped at the version whose copy is the earlier one,
    // opened by this build. This is the path where "which copy is live" is not
    // academic: the table is dropped and written again.
    let dir = TempDir::new("comments-upgrade");
    let before = {
        let conn = database_at_version(&dir, LIVE_COMMENTS_DECLARATION - 1);
        let columns = columns_of(&conn, "comments");
        assert_eq!(
            recorded_comments_ddl(&conn),
            declared_comments_ddl(2),
            "the older database has the earlier declaration, verbatim"
        );
        columns
    };

    let db = dir.open();
    // Read through the handle BEFORE taking its connection: the mutex is not
    // reentrant, and a guard held across a `DocumentDb` call is a deadlock.
    let version = db.meta(META_SCHEMA_VERSION).expect("version");
    let conn = db.conn();

    assert_eq!(
        version,
        Some(newest_schema_version()),
        "an open brings the schema to the newest version"
    );
    assert_eq!(
        columns_of(&conn, "comments"),
        before,
        "the rebuild carries the table over unchanged, so an upgraded database \
         and a fresh one agree about it"
    );
    assert_eq!(
        recorded_comments_ddl(&conn),
        declared_comments_ddl(LIVE_COMMENTS_DECLARATION),
        "after the upgrade the live declaration is in force, as it is on a fresh \
         database"
    );
}
