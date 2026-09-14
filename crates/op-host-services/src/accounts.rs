//! The account store: users, sessions, one-time tokens and invites, in SQLite.
//!
//! ## Why this module exists
//!
//! Until now a deployment could not say who anybody was. The document index
//! records an `owner_id` per file and the collaboration layer hands out
//! tickets, but the account those names refer to lived nowhere in the product —
//! an "account" was whatever the hub, or a client, asserted. This is the first
//! step of the answer: the schema and the operations over it. Nothing reads it
//! yet. No route opens this file, the daemon does not know it exists, and the
//! product behaves exactly as it did before the module was added.
//!
//! ## Why a database of its own
//!
//! Accounts are a property of the DEPLOYMENT; documents are a property of the
//! documents directory. The document index lives beside the files it accounts
//! for (`NORKA_DOCUMENTS_DIR`), because moving the documents without the index
//! leaves a list describing a directory that is no longer there. An account,
//! by the same argument, belongs beside the deployment's data
//! ([`DATA_DIR_ENV`]) — a directory that has nothing to do with where the
//! artwork is kept, and that stays put when the artwork does not.
//!
//! The two also have to be able to fail independently. A daemon resolves a
//! caller's credential before it knows what the caller asked for, so the
//! verifier runs before any document is touched. Sharing one file and one
//! mutex would put every such resolve and every document write behind the same
//! single connection.
//!
//! ## Why one connection behind a mutex
//!
//! As in the document index: one connection, guarded, because the shape of the
//! caller is one request at a time per process and every statement here is a
//! single row. A pool would buy concurrency this path cannot use, and would
//! make "which connection saw the write" a question. WAL plus a busy timeout is
//! what keeps a second PROCESS — a CLI, a backup, an operator with the sqlite3
//! shell — from turning a login into `SQLITE_BUSY`.
//!
//! ## What is deliberately absent
//!
//! No plaintext credential is ever stored, and no operation here decides who
//! may do what. Passwords are stored as Argon2id PHC strings
//! ([`accounts_password`]), sessions and one-time tokens as SHA-256 of the
//! value the client holds ([`accounts_secret`]), and an account's status is
//! stored, not interpreted: whether a `disabled` account may open a document is
//! a question for the layer that has a request in hand.
//!
//! ## Why the directory is an argument, not the environment
//!
//! [`AccountsDb::open`] is given the directory it lives in, and nothing here
//! reads the environment on its own except [`AccountsDb::open_from_env`], which
//! exists so a deployment can be configured once. A store cached in a process
//! and keyed by an environment variable is a store whose contents depend on
//! when the variable was last written — which is exactly what a test would trip
//! over.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};

mod accounts_bootstrap;
mod accounts_error;
mod accounts_invites;
mod accounts_migrations;
mod accounts_model;
mod accounts_password;
mod accounts_password_strength;
mod accounts_policy;
mod accounts_secret;
mod accounts_sessions;
mod accounts_signin;
mod accounts_tokens;
mod accounts_users;

pub use accounts_bootstrap::FirstAdmin;
pub use accounts_error::AccountsError;
pub use accounts_model::{
    Invite, IssuedInvite, IssuedSession, IssuedToken, NewInvite, NewSession, NewUser,
    OneTimePurpose, OneTimeToken, Session, User, UserStatus,
};
pub use accounts_password::{hash_password, verify_password, HASH_ALGO_ARGON2ID};
pub use accounts_password_strength::{
    check_password_strength, WeakPasswordReason, FIRST_ADMIN_ROLE, MIN_PASSWORD_CHARS,
};
pub use accounts_policy::{
    EMAIL_VERIFY_TTL_SECS, INVITE_TTL_SECS, PASSWORD_RESET_TTL_SECS, SESSION_TTL_SECS,
};
pub use accounts_secret::{hash_token, issue_token, token_hash_eq};
pub use accounts_signin::SignInOutcome;

use accounts_migrations::{Migration, MIGRATIONS};

/// The database file.
///
/// One database per deployment, holding every table in this module: the four
/// tables are read together (a session names a user; a consumed token belongs
/// to one), and a foreign key between separate files is not a thing SQLite can
/// express.
const DB_FILE: &str = "accounts.db";

/// The directory a deployment keeps its own state in — the same variable the
/// tenant store reads, named here rather than shared through a module import so
/// this store has no dependency on the web layer that happens to read it first.
pub const DATA_DIR_ENV: &str = "OPENPENCIL_ONLINE_DATA_DIR";

/// `meta` key holding the schema version this database is at.
const META_SCHEMA_VERSION: &str = "schema_version";

/// An account store, plus the directory it lives in.
#[derive(Clone)]
pub struct AccountsDb {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for AccountsDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection is not printable and holds no useful identity; the
        // directory is what names this store in a log line or a failure.
        f.debug_struct("AccountsDb")
            .field("dir", &self.inner.dir)
            .finish()
    }
}

impl AccountsDb {
    /// Open (creating when absent) `<dir>/accounts.db` and bring its schema up
    /// to date.
    ///
    /// Every step is idempotent, so this is safe on a directory that has been
    /// in use for months and on one created a second ago.
    pub fn open(dir: &Path) -> Result<Self, AccountsError> {
        std::fs::create_dir_all(dir)
            .map_err(|error| AccountsError::Io(format!("create {}: {error}", dir.display())))?;
        let conn = Connection::open(dir.join(DB_FILE))?;
        apply_pragmas(&conn)?;
        migrate(&conn)?;
        Ok(Self {
            inner: Arc::new(Inner {
                dir: dir.to_path_buf(),
                conn: Mutex::new(conn),
            }),
        })
    }

    /// The store for this deployment, or `None` when no data directory is
    /// configured.
    ///
    /// `None` rather than a default path, because there is no honest default: a
    /// store invented next to the working directory would be a second set of
    /// accounts that only a process started from the right place can see. A
    /// caller that gets `None` has to decide what "no accounts" means — for a
    /// route, refusing the request — and cannot do it by accident.
    pub fn open_from_env() -> Result<Option<Self>, AccountsError> {
        match configured_data_dir(std::env::var(DATA_DIR_ENV).ok().as_deref()) {
            Some(dir) => Self::open(&dir).map(Some),
            None => Ok(None),
        }
    }

    /// The directory this store lives in.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// One `meta` value — the migration bookkeeping, readable for diagnostics
    /// and by the tests that pin it.
    pub fn meta(&self, key: &str) -> Result<Option<String>, AccountsError> {
        meta_get(&self.conn(), key)
    }

    /// The single connection, for the length of one statement or transaction.
    ///
    /// A poisoned lock means some other request panicked while writing. The
    /// connection is still usable — SQLite rolls back what was in flight — so
    /// the guard is recovered rather than propagated: turning every later
    /// request into a panic would take sign-in down for a fault that has
    /// already been contained.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.inner
            .conn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The deployment's data directory, when one is configured.
///
/// Blank and whitespace-only count as unset: an environment file that writes
/// `OPENPENCIL_ONLINE_DATA_DIR=` is a deployment that has not decided, not a
/// deployment that decided on the empty path. Surrounding whitespace is
/// trimmed, because `VAR= /srv/norka` in a shell is the path somebody meant.
///
/// Takes the value rather than reading the environment so the rule is testable
/// without a test mutating a process-wide variable that every other test in the
/// binary shares.
fn configured_data_dir(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// PRAGMAs that must hold for the life of the connection.
fn apply_pragmas(conn: &Connection) -> Result<(), AccountsError> {
    // WAL: a credential resolve never blocks a session update, and vice versa.
    // One connection in this process means one writer anyway; WAL is what keeps
    // that writer from stopping a reader in another process.
    //
    // Read back rather than assumed: SQLite silently stays in `delete` mode
    // when it cannot create the `-wal` file (a read-only directory, some
    // network filesystems), and a store that quietly lost the property it was
    // configured for is worse than one that refuses to open.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(AccountsError::Database(format!(
            "journal_mode is {mode}, not WAL"
        )));
    }
    // NORMAL is the pair WAL is designed for: durable across a process crash,
    // at risk only in a power loss that would take the filesystem with it.
    // `foreign_keys` is per-connection and OFF by default, and every cascade in
    // the schema — a deleted account taking its sessions, tokens and invites
    // with it — depends on it.
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
    // Wait for another process's lock rather than failing a sign-in with
    // SQLITE_BUSY.
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    Ok(())
}

/// Bring the schema up to [`MIGRATIONS`]'s latest version.
fn migrate(conn: &Connection) -> Result<(), AccountsError> {
    // The bookkeeping table IS the migration system, so it is created before
    // any migration runs and sits outside the version numbering.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )?;
    // A missing or unparsable version reads as 0 — the state of a database this
    // module has never opened. A version that is not a number is not a version
    // somebody meant to write, and treating it as "start from the beginning"
    // fails loudly on the first `CREATE TABLE` rather than silently skipping a
    // step.
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
    // One transaction for the whole pending list: an open lands on the previous
    // version or on the newest one, never in between — which matters here
    // because a session pointing at a user id requires both tables to exist.
    let tx = conn.unchecked_transaction()?;
    for migration in pending {
        tx.execute_batch(migration.sql)?;
        meta_set(&tx, META_SCHEMA_VERSION, &migration.version.to_string())?;
    }
    tx.commit()?;
    Ok(())
}

/// Read one `meta` value.
fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>, AccountsError> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(AccountsError::from)
}

/// Write one `meta` value.
fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), AccountsError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Seconds since the Unix epoch, as the store's columns are written.
///
/// Offered to callers rather than read inside the store: every operation that
/// compares against time takes `now` as an argument, so expiry is testable at
/// any point in time without a clock abstraction or a sleeping test.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod accounts_tests;
