//! The schema, its migrations, and the one-time import of the JSON files the
//! database replaced.
//!
//! The store's own rules (keys, file names, what each operation means) are
//! pinned in `document_store_tests`; these drive the storage layer directly,
//! including the paths a running daemon would never take on purpose — a legacy
//! file that will not parse, an entry whose key could not be opened.

use super::*;
use crate::document_comments::{self, Author, NewComment};
use crate::document_test_dir::TempDir;

/// Every object the schema holds, by name.
fn schema_objects(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master ORDER BY name")
        .expect("prepare");
    statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("collect")
}

/// A database exactly as a deployment that predates the conversation tables has
/// it: migration 1 applied, and the version recorded to match.
fn open_at_version_one(dir: &TempDir) -> Connection {
    let conn = Connection::open(dir.join(DB_FILE)).expect("open");
    conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .expect("meta");
    conn.execute_batch(MIGRATIONS[0].sql).expect("migration 1");
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, '1')",
        params![META_SCHEMA_VERSION],
    )
    .expect("version 1");
    conn
}

/// A database as the element-anchored build left it: migrations 1 and 2
/// applied, and the version recorded to match.
///
/// Migration 2's `comment_threads` is what this produces — a `node_id TEXT NOT
/// NULL` and no coordinates — which is the shape migration 3 has to carry over
/// without losing a row.
fn open_at_version_two(dir: &TempDir) -> Connection {
    let conn = Connection::open(dir.join(DB_FILE)).expect("open");
    conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .expect("meta");
    conn.execute_batch(MIGRATIONS[0].sql).expect("migration 1");
    conn.execute_batch(MIGRATIONS[1].sql).expect("migration 2");
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, '2')",
        params![META_SCHEMA_VERSION],
    )
    .expect("version 2");
    conn
}

/// A stored document with everything spelled out, so a test can compare whole
/// entries rather than field by field.
fn entry(
    key: &str,
    name: &str,
    created_at: u64,
    updated_at: u64,
    size: u64,
    has_thumbnail: bool,
) -> DocumentEntry {
    DocumentEntry {
        key: key.to_string(),
        name: name.to_string(),
        // Ownerless: what the import writes and what the offline daemon makes.
        // The owned case has its own helper below.
        owner_id: None,
        created_at,
        updated_at,
        size,
        has_thumbnail,
    }
}

/// The same row, belonging to an account.
fn owned_entry(key: &str, name: &str, owner: &str) -> DocumentEntry {
    DocumentEntry {
        owner_id: Some(owner.to_string()),
        ..entry(key, name, 1, 1, 1, false)
    }
}

/// A legacy `index.json`, written the way the file-based store wrote it — so a
/// fixture that stops matching the old format fails here rather than in a
/// deployment.
fn index_json(entries: &[DocumentEntry]) -> String {
    serde_json::to_string(entries).expect("serialize fixture")
}

#[test]
fn the_schema_migrations_and_pragmas_are_all_in_place() {
    let dir = TempDir::new("schema");
    let db = dir.open();

    let latest = MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .max()
        .expect("at least one migration");
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("meta"),
        Some(latest.to_string()),
        "an open brings the schema to the newest version"
    );

    {
        let conn = db.conn();
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");
        assert_eq!(journal.to_lowercase(), "wal");
        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous");
        assert_eq!(synchronous, 1, "NORMAL");
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("foreign keys");
        assert_eq!(foreign_keys, 1);
        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy timeout");
        assert_eq!(busy_timeout, 5000);
        let objects = schema_objects(&conn);
        for name in [
            "documents",
            "last_opened",
            "meta",
            "documents_by_recency",
            // Migration 2: the conversation.
            "comment_threads",
            "comments",
            "comment_threads_by_document",
            "comments_by_thread",
        ] {
            assert!(
                objects.contains(&name.to_string()),
                "{name} missing: {objects:?}"
            );
        }
    }
}

#[test]
fn a_database_from_before_the_conversation_tables_gains_them_on_open() {
    // The upgrade a deployment in the field actually takes: a database at
    // version 1, with the rows it already had, opened by this build.
    let dir = TempDir::new("upgrade-from-v1");
    {
        let conn = open_at_version_one(&dir);
        conn.execute(
            "INSERT INTO documents
                 (key, name, owner_id, created_at, updated_at, size, has_thumbnail)
             VALUES ('aaaaaaaa00000001', 'Older', NULL, 1, 2, 3, 0)",
            [],
        )
        .expect("a document from before the upgrade");
    }

    let db = dir.open();
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("meta").as_deref(),
        Some("3"),
        "an open brings the schema to the newest version"
    );
    {
        let conn = db.conn();
        let objects = schema_objects(&conn);
        for name in [
            "comment_threads",
            "comments",
            "comment_threads_by_document",
            "comments_by_thread",
        ] {
            assert!(
                objects.contains(&name.to_string()),
                "{name} missing after the upgrade: {objects:?}"
            );
        }
    }
    // And what was already there is still there: a migration adds, it does not
    // rebuild.
    assert_eq!(list_entries(&db).expect("list").len(), 1);
}

#[test]
fn opening_the_same_database_again_does_not_migrate_it_a_second_time() {
    let dir = TempDir::new("reopen-v2");
    let key = "aaaaaaaa00000001";
    {
        let db = dir.open();
        insert_entry(&db, &entry(key, "Kept", 1, 1, 1, false)).expect("insert");
        // A thread written through the tables migrations 2 and 3 left. Running
        // either of them again would fail loudly rather than duplicate anything
        // — `CREATE TABLE` with no `IF NOT EXISTS` in the first case, and a
        // `comment_threads_v3` that already exists in the second — which is the
        // point: the version in `meta`, and not any tolerance in the SQL, is
        // what keeps a second open from re-running a step.
        document_comments::create_thread(
            &db,
            key,
            document_comments::Placement {
                page_id: "page-1".to_string(),
                x: 4.0,
                y: 5.0,
            },
            NewComment {
                author: Author { id: None, name: "", role: None },
                body: "hello",
            },
            10,
        )
        .expect("create")
        .expect("the document is stored");
    }

    let db = dir.open();
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("meta").as_deref(),
        Some("3")
    );
    assert_eq!(
        list_entries(&db).expect("list").len(),
        1,
        "no row was written twice"
    );
    {
        let conn = db.conn();
        let objects = schema_objects(&conn);
        assert_eq!(
            objects
                .iter()
                .filter(|name| *name == "comment_threads")
                .count(),
            1,
            "the table exists once: {objects:?}"
        );
    }
    assert_eq!(
        document_comments::list_threads(&db, key)
            .expect("list")
            .expect("the document is stored")
            .len(),
        1,
        "and the thread written before the second open is still there"
    );
}

#[test]
fn a_database_of_element_anchored_threads_is_rebuilt_without_losing_a_comment() {
    // The upgrade every deployment of the previous build takes, and the only
    // dangerous step in it. Migration 3 cannot `ALTER TABLE` the shape it needs
    // — SQLite never drops a `NOT NULL`, and `node_id` has to become nullable —
    // so the threads table is copied and replaced. Its child `comments` then has
    // to be dropped and created again, because `DROP TABLE` on a parent fires
    // the child's `ON DELETE CASCADE`: get that order wrong and every reply in
    // the database is gone, silently, in a transaction that commits.
    let dir = TempDir::new("upgrade-from-v2");
    {
        let conn = open_at_version_two(&dir);
        conn.execute(
            "INSERT INTO documents
                 (key, name, owner_id, created_at, updated_at, size, has_thumbnail)
             VALUES ('aaaaaaaa00000001', 'Reviewed', NULL, 1, 2, 3, 0)",
            [],
        )
        .expect("a document with a conversation");
        conn.execute(
            "INSERT INTO comment_threads (document_key, node_id, created_at, resolved)
             VALUES ('aaaaaaaa00000001', 'n1', 10, 1),
                    ('aaaaaaaa00000001', 'n2', 11, 0)",
            [],
        )
        .expect("a closed thread and an open one");
        conn.execute(
            "INSERT INTO comments (thread_id, author_id, author_name, body, created_at)
             VALUES (1, 'userA', 'Anya', 'one', 10),
                    (1, NULL, '', 'two', 11),
                    (2, 'userB', 'Boris', 'three', 11)",
            [],
        )
        .expect("three comments");
    }

    let db = dir.open();
    assert_eq!(
        db.meta(META_SCHEMA_VERSION).expect("meta").as_deref(),
        Some("3"),
        "the rebuild is one more version, not a special case"
    );

    let threads = document_comments::list_threads(&db, "aaaaaaaa00000001")
        .expect("list")
        .expect("the document is stored");
    assert_eq!(threads.len(), 2, "no thread was lost to the rebuild");
    assert_eq!(
        threads
            .iter()
            .map(|thread| thread.comments.len())
            .sum::<usize>(),
        3,
        "and no comment was cascaded away by it"
    );
    // What the old rows knew is still known, and what they never knew is not
    // invented: no coordinates (a position nobody recorded), and the element
    // each thread was anchored to, kept as a hint.
    assert!(
        threads.iter().all(|thread| thread.placement.is_none()),
        "the migration does not guess a position for a thread that never had one"
    );
    let hints: Vec<Option<&str>> = threads
        .iter()
        .map(|thread| thread.anchor_hint.as_deref())
        .collect();
    assert_eq!(hints, vec![Some("n1"), Some("n2")]);
    assert_eq!(threads[0].comments[0].author_name, "Anya");
    assert_eq!(
        threads[0].comments[1].author_id, None,
        "the local operator's NULL author survived the copy"
    );
    assert!(threads[0].resolved, "the closed thread is still closed");
    assert!(!threads[1].resolved);

    // And the table works at its new shape: a pin written now, with an id above
    // every id already handed out. The copy carries the AUTOINCREMENT sequence
    // across the rename, so the ids the old rows used are not issued again —
    // the property `a_thread_id_is_never_handed_out_twice` exists for.
    let placed = document_comments::create_thread(
        &db,
        "aaaaaaaa00000001",
        document_comments::Placement {
            page_id: "page-1".to_string(),
            x: 12.5,
            y: -3.0,
        },
        NewComment {
            author: Author {
                id: Some("userC"),
                name: "Vera",
                role: None,
            },
            body: "after the migration",
        },
        20,
    )
    .expect("create")
    .expect("the document is stored");
    assert!(
        placed.id > threads[1].id,
        "a rebuilt table must not hand out an id it already gave away: {} vs {}",
        placed.id,
        threads[1].id
    );
    assert_eq!(placed.placement.map(|placement| placement.page_id), Some("page-1".to_string()));

    // The tables the rebuild borrowed are gone: a scratch copy left behind is a
    // table somebody will later mistake for data.
    let conn = db.conn();
    let objects = schema_objects(&conn);
    for name in ["comment_threads_v3", "comments_saved"] {
        assert!(
            !objects.contains(&name.to_string()),
            "{name} was left behind: {objects:?}"
        );
    }
}

#[test]
fn a_fresh_directory_imports_nothing_and_records_that_it_did() {
    let dir = TempDir::new("fresh");
    let db = dir.open();
    assert!(list_entries(&db).expect("list").is_empty());
    assert_eq!(
        db.meta(META_INDEX_IMPORT_DONE).expect("meta").as_deref(),
        Some("1"),
        "an absent index is a completed import, not a pending one"
    );
    assert_eq!(
        db.meta(META_INDEX_IMPORT_ROWS).expect("meta").as_deref(),
        Some("0")
    );
    assert_eq!(
        db.meta(META_INDEX_IMPORT_BYTES).expect("meta").as_deref(),
        Some("0")
    );
    assert!(db.meta(META_INDEX_IMPORT_AT).expect("meta").is_some());
    assert_eq!(db.meta(META_INDEX_IMPORT_ERROR).expect("meta"), None);
    assert_eq!(
        db.meta(META_LAST_IMPORT_DONE).expect("meta").as_deref(),
        Some("1")
    );
}

#[test]
fn an_index_json_that_appears_after_the_first_open_is_not_read() {
    // The database is the record now. Reading a file somebody dropped in later
    // would resurrect documents the operator deleted, so the "nothing to
    // import" answer is final — restoring an old-layout backup means restoring
    // its database too.
    let dir = TempDir::new("late-index");
    drop(dir.open());
    dir.write(
        INDEX_FILE,
        &index_json(&[entry("aaaaaaaa00000001", "Late", 1, 1, 1, false)]),
    );
    let db = dir.open();
    assert!(list_entries(&db).expect("list").is_empty());
}

#[test]
fn a_legacy_index_is_imported_once_with_null_owners() {
    let dir = TempDir::new("import");
    let older = entry("aaaaaaaa00000001", "First", 100, 100, 3, true);
    let newer = entry("aaaaaaaa00000002", "Second", 100, 200, 7, false);
    let raw = index_json(&[older.clone(), newer.clone()]);
    dir.write(INDEX_FILE, &raw);

    let db = dir.open();
    let rows = list_entries(&db).expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], newer, "imported rows keep their list order");
    assert_eq!(rows[1], older);
    assert_eq!(
        db.meta(META_INDEX_IMPORT_ROWS).expect("meta").as_deref(),
        Some("2")
    );
    assert_eq!(
        db.meta(META_INDEX_IMPORT_BYTES).expect("meta").as_deref(),
        Some(raw.len().to_string().as_str())
    );
    assert_eq!(db.meta(META_INDEX_IMPORT_ERROR).expect("meta"), None);

    // Read the owner straight out of the file rather than through an accessor:
    // what the migration promises is a property of the SCHEMA, and the next
    // step (#10) is what starts filling this column in.
    {
        let conn = Connection::open(dir.join(DB_FILE)).expect("second connection");
        let owner: Option<String> = conn
            .query_row(
                "SELECT owner_id FROM documents WHERE key = ?1",
                params![newer.key],
                |row| row.get(0),
            )
            .expect("owner");
        assert_eq!(owner, None, "an imported row belongs to nobody yet");
    }

    // A second open finds the import already done: same rows, not twice as many.
    drop(db);
    let db = dir.open();
    assert_eq!(list_entries(&db).expect("list").len(), 2);
}

#[test]
fn a_broken_index_is_reported_and_the_import_is_retried() {
    let dir = TempDir::new("broken-index");
    dir.write(INDEX_FILE, "{ not json");

    // A legacy file nobody can read must not take the file list down with it.
    // `TempDir::open` asserts the open succeeds: a broken legacy file must not
    // be able to take the store down.
    let db = dir.open();
    assert!(list_entries(&db).expect("list").is_empty());
    assert!(
        db.meta(META_INDEX_IMPORT_ERROR).expect("meta").is_some(),
        "the reason is recorded where an operator can read it"
    );
    assert_eq!(
        db.meta(META_INDEX_IMPORT_DONE).expect("meta"),
        None,
        "an unreadable file leaves the import unfinished"
    );

    // Repairing it brings the rows over on the next open, and the failure is
    // cleared rather than left to read as current.
    drop(db);
    dir.write(
        INDEX_FILE,
        &index_json(&[entry("aaaaaaaa00000001", "Repaired", 5, 5, 2, false)]),
    );
    let db = dir.open();
    assert_eq!(list_entries(&db).expect("list").len(), 1);
    assert_eq!(db.meta(META_INDEX_IMPORT_ERROR).expect("meta"), None);
    assert_eq!(
        db.meta(META_INDEX_IMPORT_ROWS).expect("meta").as_deref(),
        Some("1")
    );
}

#[test]
fn a_legacy_key_that_could_not_be_opened_stays_in_the_file() {
    let dir = TempDir::new("bad-key");
    dir.write(
        INDEX_FILE,
        &index_json(&[
            entry("aaaaaaaa00000001", "Good", 1, 1, 1, false),
            entry("../etc/passwd", "Impossible", 1, 1, 1, false),
            entry("i-l-o", "Also impossible", 1, 1, 1, false),
        ]),
    );
    let db = dir.open();
    let rows = list_entries(&db).expect("list");
    assert_eq!(
        rows.len(),
        1,
        "only the key this store could issue came over"
    );
    assert_eq!(rows[0].key, "aaaaaaaa00000001");
    assert_eq!(
        db.meta(META_INDEX_IMPORT_SKIPPED).expect("meta").as_deref(),
        Some("2")
    );
}

#[test]
fn last_json_is_brought_over_when_it_names_an_imported_document() {
    let dir = TempDir::new("last-import");
    let document = entry("aaaaaaaa00000001", "Work", 10, 20, 4, false);
    dir.write(INDEX_FILE, &index_json(std::slice::from_ref(&document)));
    dir.write(
        LAST_DOCUMENT_FILE,
        &serde_json::to_string(&document).expect("serialize fixture"),
    );

    let db = dir.open();
    let restored = last_entry(&db, LOCAL_OWNER)
        .expect("last")
        .expect("a pointer");
    assert_eq!(restored.key, document.key);
    assert_eq!(restored.name, "Work");
}

#[test]
fn a_last_json_that_names_nothing_stored_is_ignored() {
    // A pointer to a document the import did not bring over would leave the
    // daemon trying to reopen a row that is not there.
    let dir = TempDir::new("last-dangling");
    dir.write(
        LAST_DOCUMENT_FILE,
        &serde_json::to_string(&entry("aaaaaaaa00000009", "Gone", 1, 1, 1, false))
            .expect("serialize fixture"),
    );
    let db = dir.open();
    assert_eq!(last_entry(&db, LOCAL_OWNER).expect("last"), None);
    assert_eq!(
        db.meta(META_LAST_IMPORT_DONE).expect("meta").as_deref(),
        Some("1")
    );
}

#[test]
fn a_last_json_that_will_not_parse_is_ignored() {
    let dir = TempDir::new("last-broken");
    dir.write(LAST_DOCUMENT_FILE, "{ not json");
    let db = dir.open();
    assert_eq!(last_entry(&db, LOCAL_OWNER).expect("last"), None);
}

#[test]
fn deleting_a_document_drops_the_last_opened_pointer() {
    // Proves `foreign_keys=ON` does what the schema says it does, rather than
    // being a pragma that is set and never exercised.
    let dir = TempDir::new("cascade");
    let db = dir.open();
    let document = entry("aaaaaaaa00000001", "Work", 1, 1, 1, false);
    insert_entry(&db, &document).expect("insert");
    remember_last_opened(&db, LOCAL_OWNER, &document.key).expect("remember");
    assert!(last_entry(&db, LOCAL_OWNER).expect("last").is_some());

    assert!(delete_entry(&db, &document.key).expect("delete"));
    assert_eq!(last_entry(&db, LOCAL_OWNER).expect("last"), None);
    assert!(!delete_entry(&db, &document.key).expect("delete twice"));
}

#[test]
fn each_owner_has_their_own_last_opened_slot() {
    // The local operator's row is a real value, not a NULL: SQLite treats NULLs
    // as distinct in a primary key, so a NULL owner would pile up pointers
    // instead of replacing one.
    let dir = TempDir::new("owners");
    let db = dir.open();
    let mine = entry("aaaaaaaa00000001", "Mine", 1, 1, 1, false);
    let theirs = entry("aaaaaaaa00000002", "Theirs", 1, 1, 1, false);
    insert_entry(&db, &mine).expect("insert");
    insert_entry(&db, &theirs).expect("insert");
    remember_last_opened(&db, LOCAL_OWNER, &mine.key).expect("remember mine");
    remember_last_opened(&db, "user-1", &theirs.key).expect("remember theirs");

    assert_eq!(
        last_entry(&db, LOCAL_OWNER).expect("last").map(|e| e.key),
        Some(mine.key)
    );
    assert_eq!(
        last_entry(&db, "user-1").expect("last").map(|e| e.key),
        Some(theirs.key)
    );
    assert_eq!(last_entry(&db, "user-2").expect("last"), None);
}

#[test]
fn an_owner_is_written_with_the_row_and_a_list_can_be_asked_for_one_account() {
    // The column existed from migration 1 and stayed NULL for every row; this
    // is the layer that starts filling it, so what it writes is worth pinning
    // as SQL rather than through an accessor.
    let dir = TempDir::new("owned-rows");
    let db = dir.open();
    let mine = owned_entry("aaaaaaaa00000001", "Mine", "userA");
    let theirs = owned_entry("aaaaaaaa00000002", "Theirs", "userB");
    let operator = entry("aaaaaaaa00000003", "Operator", 1, 1, 1, false);
    for document in [&mine, &theirs, &operator] {
        insert_entry(&db, document).expect("insert");
    }
    {
        let conn = Connection::open(dir.join(DB_FILE)).expect("second connection");
        let owner: Option<String> = conn
            .query_row(
                "SELECT owner_id FROM documents WHERE key = ?1",
                params![mine.key],
                |row| row.get(0),
            )
            .expect("owner");
        assert_eq!(owner.as_deref(), Some("userA"));
    }
    // A row carries its owner back out, and only the asked-for account's rows
    // come back: the NULL row belongs to no account, so it is in nobody's list.
    assert_eq!(
        find_entry(&db, &mine.key)
            .expect("find")
            .map(|e| e.owner_id),
        Some(Some("userA".to_string()))
    );
    let mine_keys: Vec<String> = list_entries_owned_by(&db, "userA")
        .expect("list")
        .into_iter()
        .map(|entry| entry.key)
        .collect();
    assert_eq!(mine_keys, vec![mine.key.clone()]);
    assert!(list_entries_owned_by(&db, "userC")
        .expect("list")
        .is_empty());
    assert_eq!(list_entries(&db).expect("list").len(), 3);
}

#[test]
fn the_list_orders_by_recency_then_by_key() {
    let dir = TempDir::new("order");
    let db = dir.open();
    // Two documents share a timestamp — created in the same second, which is
    // the common case — so the tie-break is what decides the order.
    let low = entry("aaaaaaaa0000000a", "A", 100, 200, 1, false);
    let high = entry("aaaaaaaa0000000b", "B", 100, 200, 1, false);
    let old = entry("aaaaaaaa0000000c", "C", 100, 100, 1, false);
    for document in [&low, &high, &old] {
        insert_entry(&db, document).expect("insert");
    }
    let keys: Vec<String> = list_entries(&db)
        .expect("list")
        .into_iter()
        .map(|entry| entry.key)
        .collect();
    assert_eq!(keys, vec![high.key, low.key, old.key]);
}

#[test]
fn a_row_survives_closing_and_reopening_the_store() {
    let dir = TempDir::new("durable");
    let document = entry("aaaaaaaa00000001", "Kept", 3, 4, 5, true);
    {
        let db = dir.open();
        insert_entry(&db, &document).expect("insert");
    }
    let db = dir.open();
    assert_eq!(
        find_entry(&db, &document.key).expect("find"),
        Some(document)
    );
}

#[test]
fn an_open_that_cannot_create_its_directory_fails_typed() {
    let dir = TempDir::new("not-a-directory");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"a file, not a directory").expect("write");
    let error = DocumentDb::open(&blocker).expect_err("a file is not a documents directory");
    assert!(
        matches!(error, DocumentStoreError::Io(_)),
        "expected a typed IO failure, got {error}"
    );
}
