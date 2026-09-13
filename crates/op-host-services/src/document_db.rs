//! The document index, in SQLite: the schema, its migrations, and the one-time
//! import of the JSON files it replaced.
//!
//! ## Why a database instead of `index.json`
//!
//! `index.json` was one array rewritten whole on every change: a create, a
//! rename and a thumbnail flag each serialized every document the daemon had
//! ever stored. Three things followed from that shape. The cost of a save grew
//! with the length of the list. A crash between the temp file and the rename
//! lost every change since the previous good write. And an array of records
//! with no relations has nowhere to put what the next steps ask — who owns a
//! document, who it is shared with, what a comment hangs off.
//!
//! One row per document answers all three. The files themselves do not move:
//! `<key>.op` and `<key>.thumb.png` stay exactly where they were, and so does
//! the drafts file (`recovery.op`) — this module accounts for documents, it
//! does not become a content store.
//!
//! ## Why one connection behind a mutex
//!
//! The daemon answers each request on its own thread, and every stored-document
//! route runs while the daemon holds its state lock (`WebCanvasState`), so
//! there is one writer at a time by construction. The mutex is what makes that
//! true for the type: a `&DocumentDb` can be shared by any number of threads
//! without their statements interleaving. A connection per request, or an async
//! pool, would buy concurrency this path cannot use — the file is local and
//! every query is a single row.
//!
//! Lock order in the daemon is
//! `registry.tenants -> tenant.state -> db -> hub`. Every operation here is
//! called with the state lock already held, and none of them takes another
//! lock, so a db-lock acquisition can never be the second half of a cycle.
//!
//! ## Why the directory is an argument, not the environment
//!
//! [`DocumentDb::open`] is given the directory it accounts for, and nothing
//! here reads `NORKA_DOCUMENTS_DIR`. A store cached in a process and keyed by
//! an environment variable is a store whose contents depend on when the
//! variable was last written — which is exactly what a test would trip over,
//! and the reason the store's `&Path` API was removed rather than wrapped.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::document_store::{
    key_is_valid, now_secs, DocumentEntry, DocumentStoreError, INDEX_FILE, LAST_DOCUMENT_FILE,
};

/// The database file, beside the documents it accounts for.
///
/// Beside, not under a shared data directory: a deployment that moves the
/// documents (`NORKA_DOCUMENTS_DIR`) must move the index with them, or the list
/// describes a directory that is no longer there. The draft file already
/// follows that rule.
const DB_FILE: &str = "documents.db";

/// Owner id of the operator of this machine.
///
/// `documents.owner_id` is nullable — a row may belong to no account yet — but
/// `last_opened` is keyed by an owner, and SQLite treats NULLs as distinct in a
/// primary key. A NULL owner there would let a second "last opened" row appear
/// instead of replacing the first, so the local operator has an explicit id:
/// the empty string, which no account id can be.
pub(crate) const LOCAL_OWNER: &str = "";

/// The columns a [`DocumentEntry`] is built from, in one place so every reader
/// agrees about the order.
const ENTRY_COLUMNS: &str = "key, name, owner_id, created_at, updated_at, size, has_thumbnail";

/// The same columns, qualified for the join that reads the last-opened row.
/// Kept beside [`ENTRY_COLUMNS`] so the two cannot drift apart unnoticed.
const JOINED_ENTRY_COLUMNS: &str =
    "d.key, d.name, d.owner_id, d.created_at, d.updated_at, d.size, d.has_thumbnail";

/// `meta` key holding the schema version this database is at.
const META_SCHEMA_VERSION: &str = "schema_version";
/// Set once the legacy `index.json` has been brought over (or found absent).
const META_INDEX_IMPORT_DONE: &str = "index_import_done";
const META_INDEX_IMPORT_ROWS: &str = "index_import_rows";
const META_INDEX_IMPORT_BYTES: &str = "index_import_bytes";
const META_INDEX_IMPORT_AT: &str = "index_import_at";
const META_INDEX_IMPORT_SKIPPED: &str = "index_import_skipped";
/// Why the import did not finish; written while the legacy file is unreadable
/// and cleared once it has been read.
const META_INDEX_IMPORT_ERROR: &str = "index_import_error";
/// Set once the legacy `last.json` has been considered (read or found absent).
const META_LAST_IMPORT_DONE: &str = "last_import_done";

/// One step of the schema's history.
struct Migration {
    /// The version this step brings the database TO. Versions are consecutive
    /// and never reused, so a database at version N has exactly the migrations
    /// `1..=N` applied.
    version: i64,
    sql: &'static str,
}

/// Every step, in order.
///
/// Add one by appending — never by editing an earlier entry, because databases
/// in the field have already run it. A step that cannot be expressed as SQL (a
/// backfill that has to read the documents, say) belongs in its own function
/// called from `open`, guarded by its own `meta` key.
///
/// The SQL strings are history: their text is what a field database recorded as
/// having run, so they are not edited, comments included. A comment that has
/// been overtaken by later work (migration 1 says every row is ownerless
/// "today") is corrected where the behaviour lives now, not in the record of
/// what the database was.
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "
        CREATE TABLE documents (
            key           TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            -- NULL means 'not attributed to an account', which is every row
            -- the local daemon creates or imports today (#10).
            owner_id      TEXT,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL,
            size          INTEGER NOT NULL,
            has_thumbnail INTEGER NOT NULL
        );

        -- The file list is a recency list, and an index in the file list's own
        -- order makes that ORDER BY a scan instead of a sort.
        CREATE INDEX documents_by_recency ON documents (updated_at DESC, key DESC);

        CREATE TABLE last_opened (
            owner_id TEXT PRIMARY KEY NOT NULL,
            -- The pointer is worthless without its document, and deleting a
            -- document should not leave one behind: ON DELETE CASCADE is why
            -- `foreign_keys=ON` is set below rather than merely available.
            key      TEXT NOT NULL REFERENCES documents (key) ON DELETE CASCADE
        );
    ",
}];

/// A store's database, plus the directory whose files it accounts for.
#[derive(Clone)]
pub struct DocumentDb {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for DocumentDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection is not printable and holds no useful identity; the
        // directory is what names this store in a log line or a failure.
        f.debug_struct("DocumentDb")
            .field("dir", &self.inner.dir)
            .finish()
    }
}

impl DocumentDb {
    /// Open (creating when absent) the database for `dir`, bring its schema up
    /// to date and import the legacy JSON files once.
    ///
    /// Every step is idempotent, so this is safe on a directory that has been
    /// in use for years and on one created a second ago.
    pub fn open(dir: &Path) -> Result<Self, DocumentStoreError> {
        std::fs::create_dir_all(dir).map_err(|error| {
            DocumentStoreError::Io(format!("create {}: {error}", dir.display()))
        })?;
        let conn = Connection::open(dir.join(DB_FILE)).map_err(db_error)?;
        apply_pragmas(&conn)?;
        migrate(&conn)?;
        import_legacy_files(&conn, dir)?;
        Ok(Self {
            inner: Arc::new(Inner {
                dir: dir.to_path_buf(),
                conn: Mutex::new(conn),
            }),
        })
    }

    /// The directory this store accounts for.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// One `meta` value — the migration and import bookkeeping, readable for
    /// diagnostics and by the tests that pin it.
    pub fn meta(&self, key: &str) -> Result<Option<String>, DocumentStoreError> {
        meta_get(&self.conn(), key)
    }

    /// The single connection, for the length of one statement or transaction.
    ///
    /// A poisoned lock means some other request panicked while writing. The
    /// connection is still usable — SQLite rolls back what was in flight — so
    /// the guard is recovered rather than propagated: turning every later
    /// request into a panic would take the file list down for a fault that has
    /// already been contained.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.inner
            .conn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The local daemon's store for this machine's documents directory, opened on
/// first use and remembered in `slot`.
///
/// Lazy on purpose. A daemon that never touches the file list — an MCP-only
/// run, a request that only reads a version — should not create a database, and
/// a failed open should be retried by the next request instead of being cached
/// as a permanently broken store.
pub(crate) fn local_store(slot: &mut Option<DocumentDb>) -> Result<DocumentDb, DocumentStoreError> {
    if let Some(db) = slot.as_ref() {
        return Ok(db.clone());
    }
    let db = DocumentDb::open(&crate::document_store::documents_dir())?;
    *slot = Some(db.clone());
    Ok(db)
}

/// PRAGMAs that must hold for the life of the connection.
fn apply_pragmas(conn: &Connection) -> Result<(), DocumentStoreError> {
    // WAL: a reader (the file list) never blocks the writer (a save), and the
    // writer never blocks a reader. The one-writer guarantee comes from the
    // daemon's state lock; WAL is what keeps an SSE poll's read from queueing
    // behind a document write.
    //
    // Read back rather than assumed: SQLite silently stays in `delete` mode
    // when it cannot create the `-wal` file (a read-only directory, some
    // network filesystems), and a store that quietly lost the property it was
    // configured for is worse than one that refuses to open.
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(db_error)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DocumentStoreError::Database(format!(
            "journal_mode is {mode}, not WAL"
        )));
    }
    // NORMAL is the pair WAL is designed for: durable across a process crash,
    // at risk only in a power loss that would take the filesystem with it.
    // `foreign_keys` is per-connection and OFF by default, and
    // `last_opened`'s cascade depends on it.
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")
        .map_err(db_error)?;
    // One connection in this process, but another process (a CLI, a backup
    // script) may be reading the file: wait for its lock rather than failing a
    // save with SQLITE_BUSY.
    conn.busy_timeout(Duration::from_millis(5000))
        .map_err(db_error)
}

/// Bring the schema up to [`MIGRATIONS`]'s latest version.
fn migrate(conn: &Connection) -> Result<(), DocumentStoreError> {
    // The bookkeeping table IS the migration system, so it is created before
    // any migration runs and sits outside the version numbering.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )
    .map_err(db_error)?;
    let current: i64 = meta_get(conn, META_SCHEMA_VERSION)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let pending: Vec<&Migration> = MIGRATIONS
        .iter()
        .filter(|migration| migration.version > current)
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    // One transaction for the whole pending list: an open either lands on the
    // previous version or on the newest one, never in between. A step too
    // expensive to hold one write lock can be split into its own `meta`-guarded
    // pass later; atomic should be the default.
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    for migration in pending {
        tx.execute_batch(migration.sql).map_err(db_error)?;
        meta_set(&tx, META_SCHEMA_VERSION, &migration.version.to_string())?;
    }
    tx.commit().map_err(db_error)
}

/// Bring the file-based index over, once.
///
/// Both legacy files stay exactly where they are: `index.json` is the only
/// record of what this directory held before the migration, and deleting the
/// evidence is not a thing a migration gets to do on its own.
fn import_legacy_files(conn: &Connection, dir: &Path) -> Result<(), DocumentStoreError> {
    import_legacy_index(conn, dir)?;
    import_legacy_last(conn, dir)
}

/// Import `index.json` into `documents`.
///
/// Every row lands with a NULL owner: the rows describe files this machine
/// held, and no account stood behind them.
fn import_legacy_index(conn: &Connection, dir: &Path) -> Result<(), DocumentStoreError> {
    if meta_get(conn, META_INDEX_IMPORT_DONE)?.as_deref() == Some("1") {
        return Ok(());
    }
    let path = dir.join(INDEX_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A directory that never had a file index (a fresh install) has
            // nothing to bring over.
            //
            // Recorded as done, so an `index.json` that appears LATER is not
            // read: the database is the record now, and re-importing a file
            // somebody dropped in would resurrect documents the operator
            // deleted. Restoring an old-layout backup means restoring its
            // database too.
            return finish_import(conn, 0, 0, 0);
        }
        Err(error) => {
            return Err(DocumentStoreError::Io(format!(
                "read {}: {error}",
                path.display()
            )))
        }
    };
    let entries: Vec<DocumentEntry> = match serde_json::from_str(&raw) {
        Ok(entries) => entries,
        Err(error) => {
            // Deliberately unfinished: the file is kept, so a repaired one
            // brings its rows over on the next open, and running the import
            // again cannot duplicate anything (every insert is OR IGNORE on the
            // primary key). The reason is recorded where an operator looks,
            // instead of printed once at start-up and forgotten.
            meta_set(conn, META_INDEX_IMPORT_ERROR, &error.to_string())?;
            return Ok(());
        }
    };
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let mut rows = 0u64;
    let mut skipped = 0u64;
    for entry in &entries {
        // A key that fails validation could never be opened again — every path
        // is built from a validated key — so it stays in the file rather than
        // becoming a row that only the list would ever see.
        if !key_is_valid(&entry.key) {
            skipped += 1;
            continue;
        }
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO documents
                     (key, name, owner_id, created_at, updated_at, size, has_thumbnail)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
                params![
                    entry.key,
                    entry.name,
                    entry.created_at,
                    entry.updated_at,
                    entry.size,
                    entry.has_thumbnail,
                ],
            )
            .map_err(db_error)?;
        if inserted > 0 {
            rows += 1;
        } else {
            skipped += 1;
        }
    }
    // The rows, the counts and the "done" flag commit together, so an
    // interrupted import is simply repeated rather than half-remembered.
    finish_import(&tx, rows, raw.len() as u64, skipped)?;
    tx.commit().map_err(db_error)
}

/// Import `last.json` into `last_opened`.
///
/// Not required by the move itself — the table replaces the file — but without
/// it an upgrade silently forgets which document was open, which is the one
/// thing that record exists to prevent. Considered once and then never again:
/// unlike the index it holds a single pointer, so there is no history worth
/// retrying when it turns out to be unreadable.
fn import_legacy_last(conn: &Connection, dir: &Path) -> Result<(), DocumentStoreError> {
    if meta_get(conn, META_LAST_IMPORT_DONE)?.as_deref() == Some("1") {
        return Ok(());
    }
    let key = std::fs::read_to_string(dir.join(LAST_DOCUMENT_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str::<DocumentEntry>(&raw).ok())
        .map(|entry| entry.key)
        .filter(|key| key_is_valid(key));
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    if let Some(key) = key {
        // A pointer to a document the import did not bring over would be
        // dangling; the same rule the store applies when it records one.
        if find_by_conn(&tx, &key)?.is_some() {
            write_last_opened(&tx, LOCAL_OWNER, &key)?;
        }
    }
    meta_set(&tx, META_LAST_IMPORT_DONE, "1")?;
    tx.commit().map_err(db_error)
}

/// Record that the index import is finished, and what it found.
fn finish_import(
    conn: &Connection,
    rows: u64,
    bytes: u64,
    skipped: u64,
) -> Result<(), DocumentStoreError> {
    meta_set(conn, META_INDEX_IMPORT_DONE, "1")?;
    meta_set(conn, META_INDEX_IMPORT_ROWS, &rows.to_string())?;
    meta_set(conn, META_INDEX_IMPORT_BYTES, &bytes.to_string())?;
    meta_set(conn, META_INDEX_IMPORT_AT, &now_secs().to_string())?;
    if skipped > 0 {
        meta_set(conn, META_INDEX_IMPORT_SKIPPED, &skipped.to_string())?;
    }
    // A previous attempt's failure is cleared rather than left to read as
    // current.
    conn.execute(
        "DELETE FROM meta WHERE key = ?1",
        params![META_INDEX_IMPORT_ERROR],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Read one `meta` value.
fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>, DocumentStoreError> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// Write one `meta` value.
fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), DocumentStoreError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Read a [`DocumentEntry`] out of the columns [`ENTRY_COLUMNS`] names.
fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<DocumentEntry> {
    Ok(DocumentEntry {
        key: row.get(0)?,
        name: row.get(1)?,
        // NULL is the operator's own row: no account stands behind it. See
        // `DocumentEntry::owner_id`.
        owner_id: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        size: row.get(5)?,
        has_thumbnail: row.get(6)?,
    })
}

/// Every document, most recently touched first — the file list's order.
pub(crate) fn list_entries(db: &DocumentDb) -> Result<Vec<DocumentEntry>, DocumentStoreError> {
    let conn = db.conn();
    let mut statement = conn
        .prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM documents ORDER BY updated_at DESC, key DESC"
        ))
        .map_err(db_error)?;
    let rows = statement.query_map([], entry_from_row).map_err(db_error)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
}

/// One account's documents, most recently touched first.
///
/// The filter is the whole point of the owner column: the directory is shared
/// by every account of a deployment, so the list a caller sees has to be asked
/// for BY caller. A row whose `owner_id` is NULL matches no account and is
/// therefore in nobody's list — the fail-closed direction, and the reason the
/// legacy import's rows do not appear online. The same index serves this query
/// as the unfiltered list: `documents_by_recency` is scanned in order and the
/// owner is tested per row, which for a directory of one account's documents is
/// what it would do anyway.
pub(crate) fn list_entries_owned_by(
    db: &DocumentDb,
    owner: &str,
) -> Result<Vec<DocumentEntry>, DocumentStoreError> {
    let conn = db.conn();
    let mut statement = conn
        .prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM documents
             WHERE owner_id = ?1 ORDER BY updated_at DESC, key DESC"
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![owner], entry_from_row)
        .map_err(db_error)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
}

/// One document, when it is there.
pub(crate) fn find_entry(
    db: &DocumentDb,
    key: &str,
) -> Result<Option<DocumentEntry>, DocumentStoreError> {
    find_by_conn(&db.conn(), key)
}

/// Add a document's row.
///
/// Written AFTER the file: a row always describes a document that exists on
/// disk, where the reverse order would leave a list entry whose file never
/// arrived.
///
/// The owner comes from the entry and is written as given: `None` becomes SQL
/// NULL, which is the local operator's own row and matches no account.
pub(crate) fn insert_entry(
    db: &DocumentDb,
    entry: &DocumentEntry,
) -> Result<(), DocumentStoreError> {
    db.conn()
        .execute(
            "INSERT INTO documents
                 (key, name, owner_id, created_at, updated_at, size, has_thumbnail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.key,
                entry.name,
                entry.owner_id,
                entry.created_at,
                entry.updated_at,
                entry.size,
                entry.has_thumbnail,
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

/// Refresh a row after its file was rewritten, creating it when the index never
/// had one — a document that arrived outside the store (restored from a backup,
/// moved in by hand) is still a document.
pub(crate) fn touch_entry(
    db: &DocumentDb,
    key: &str,
    updated_at: u64,
    size: u64,
    fallback_name: &str,
) -> Result<DocumentEntry, DocumentStoreError> {
    let conn = db.conn();
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let changed = tx
        .execute(
            "UPDATE documents SET updated_at = ?2, size = ?3 WHERE key = ?1",
            params![key, updated_at, size],
        )
        .map_err(db_error)?;
    if changed == 0 {
        // A row for a document that arrived outside the store keeps a NULL
        // owner. That is not a placeholder for "somebody will claim it": the
        // row describes a file this daemon did not make, and no account can
        // demonstrate a right to it. Online, the routes require the row to
        // exist before they write, so this branch is the local operator's
        // (an `.op` dropped into their directory by hand) — where a NULL owner
        // means exactly what it says.
        tx.execute(
            "INSERT INTO documents
                 (key, name, owner_id, created_at, updated_at, size, has_thumbnail)
             VALUES (?1, ?2, NULL, ?3, ?3, ?4, 0)",
            params![key, fallback_name, updated_at, size],
        )
        .map_err(db_error)?;
    }
    let entry = tx
        .query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM documents WHERE key = ?1"),
            params![key],
            entry_from_row,
        )
        .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(entry)
}

/// Set a document's name. `None` when no such document is stored.
pub(crate) fn rename_entry(
    db: &DocumentDb,
    key: &str,
    name: &str,
    updated_at: u64,
) -> Result<Option<DocumentEntry>, DocumentStoreError> {
    let conn = db.conn();
    let changed = conn
        .execute(
            "UPDATE documents SET name = ?2, updated_at = ?3 WHERE key = ?1",
            params![key, name, updated_at],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Ok(None);
    }
    find_by_conn(&conn, key)
}

/// Drop a document's row, and with it whatever pointed at it. `false` when
/// there was no row to drop.
pub(crate) fn delete_entry(db: &DocumentDb, key: &str) -> Result<bool, DocumentStoreError> {
    let removed = db
        .conn()
        .execute("DELETE FROM documents WHERE key = ?1", params![key])
        .map_err(db_error)?;
    Ok(removed > 0)
}

/// Record whether a preview exists for a document. `false` when no such
/// document is stored.
///
/// Writing the same value again is a no-op in SQL rather than a
/// read-modify-write in Rust: the flag is written from more than one place (an
/// explicit save, an autosave), and a compare-then-set pair is exactly where
/// two of them would lose each other's update.
pub(crate) fn set_thumbnail(
    db: &DocumentDb,
    key: &str,
    exists: bool,
) -> Result<bool, DocumentStoreError> {
    let conn = db.conn();
    let changed = conn
        .execute(
            "UPDATE documents SET has_thumbnail = ?2 WHERE key = ?1 AND has_thumbnail <> ?2",
            params![key, exists],
        )
        .map_err(db_error)?;
    if changed > 0 {
        return Ok(true);
    }
    // Nothing written: either the flag was already right, or there is no row.
    Ok(find_by_conn(&conn, key)?.is_some())
}

/// Point `owner`'s single "reopen this" slot at `key`.
///
/// Whether the document exists is the caller's question ([`find_entry`] answers
/// it); this writes the pointer, replacing whatever that owner had.
pub(crate) fn remember_last_opened(
    db: &DocumentDb,
    owner: &str,
    key: &str,
) -> Result<(), DocumentStoreError> {
    write_last_opened(&db.conn(), owner, key)
}

/// The document `owner` had open, when one is recorded.
pub(crate) fn last_entry(
    db: &DocumentDb,
    owner: &str,
) -> Result<Option<DocumentEntry>, DocumentStoreError> {
    db.conn()
        .query_row(
            &format!(
                "SELECT {JOINED_ENTRY_COLUMNS}
                 FROM last_opened AS l JOIN documents AS d ON d.key = l.key
                 WHERE l.owner_id = ?1"
            ),
            params![owner],
            entry_from_row,
        )
        .optional()
        .map_err(db_error)
}

/// Write one "reopen this" pointer, replacing whatever the same owner had.
///
/// The connection-level half of [`remember_last_opened`], so the import path
/// (which already holds a transaction) can use the same statement.
fn write_last_opened(conn: &Connection, owner: &str, key: &str) -> Result<(), DocumentStoreError> {
    conn.execute(
        "INSERT INTO last_opened (owner_id, key) VALUES (?1, ?2)
         ON CONFLICT (owner_id) DO UPDATE SET key = excluded.key",
        params![owner, key],
    )
    .map_err(db_error)?;
    Ok(())
}

/// [`find_entry`] for a caller that already holds the connection — the mutex is
/// not reentrant, so a function holding the guard must never call back into the
/// database handle.
fn find_by_conn(conn: &Connection, key: &str) -> Result<Option<DocumentEntry>, DocumentStoreError> {
    conn.query_row(
        &format!("SELECT {ENTRY_COLUMNS} FROM documents WHERE key = ?1"),
        params![key],
        entry_from_row,
    )
    .optional()
    .map_err(db_error)
}

/// Every rusqlite failure becomes one variant: the caller's decision is the
/// same for all of them (the store is unusable for this request), and the
/// message carries what SQLite said.
fn db_error(error: rusqlite::Error) -> DocumentStoreError {
    DocumentStoreError::Database(error.to_string())
}

#[cfg(test)]
#[path = "document_db_tests.rs"]
mod tests;
