//! Server-side documents: a directory of `.op` files, a SQLite index, and short
//! keys.
//!
//! The browser has no filesystem, so "the file" has to live somewhere the
//! daemon can reach. This is that somewhere: one `.op` per document, one
//! `.thumb.png` preview beside it, one draft per workspace for work that has no
//! document yet — and the accounting (name, owner, timestamps, size, thumbnail
//! flag) in a SQLite database owned by `crate::document_db`.
//!
//! This module is the rules, not the storage: it validates keys, decides where
//! a file lives, refuses a write to a document that is not there, and hands the
//! row work to the database. Nothing here reads `index.json` any more. The two
//! JSON files the accounting used to live in (`index.json`, `last.json`) are
//! still on disk, untouched: they are the record of what this directory held
//! before the move, and the database imported them once.
//!
//! ## One directory, every account — so every row names its owner
//!
//! The directory is per process, not per account, and a public deployment
//! serves many accounts from one process. What a row says about who it belongs
//! to ([`DocumentEntry::owner_id`]) is therefore what a route decides on: the
//! list is asked for by owner, and a document addressed by key is only reached
//! by the account that owns it or by a visitor the owner admitted. Before the
//! column was filled, the daemon had no answer to that question and refused the
//! whole stored-document family in a shared deployment instead (#20).
//!
//! Keys are short, opaque and validated before they touch a path. The address
//! bar shows them (`/f/<key>`), people paste them into chat, so they must be
//! safe to hand around — which is also why nothing here trusts a key that did
//! not come out of [`new_key`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::document_db::{self, DocumentDb};

/// Environment variable naming the documents directory, for tests and for a
/// deployment that wants documents on another volume.
pub const DOCUMENTS_DIR_ENV: &str = "NORKA_DOCUMENTS_DIR";

/// Longest key this store issues; also the acceptance bound for pasted ones.
const MAX_KEY_LEN: usize = 32;
/// Shortest key accepted — short enough for a URL, long enough not to collide.
const MIN_KEY_LEN: usize = 8;

/// Name a document gets when whoever made it did not give one.
const DEFAULT_NAME: &str = "Untitled";

/// The JSON index this store used before the database did the accounting.
///
/// Never deleted and never renamed: it is the only record of what the documents
/// directory held before the move, and `crate::document_db` reads it once.
pub(crate) const INDEX_FILE: &str = "index.json";

/// The single-key "which document was open" record, replaced by the database's
/// `last_opened` table and likewise left where it is.
pub(crate) const LAST_DOCUMENT_FILE: &str = "last.json";

/// One document as the file list sees it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocumentEntry {
    pub key: String,
    pub name: String,
    /// The account this document belongs to, or `None` when no account stands
    /// behind it.
    ///
    /// `None` is what the offline daemon writes: it has no accounts, so there
    /// is nobody to name, and every row the legacy `index.json` import brought
    /// over arrives the same way. That is also what makes the column usable as
    /// an authorization input — "belongs to no account" and "belongs to this
    /// account" are different answers, and a store that could not tell them
    /// apart is the reason a shared deployment had to refuse the whole
    /// stored-document family (#20).
    ///
    /// Defaulted so an `index.json` written before this field existed still
    /// deserializes; the import inserts an explicit NULL for those rows.
    #[serde(default)]
    pub owner_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub size: u64,
    /// Whether a thumbnail has been rendered for this document.
    ///
    /// Defaulted so an index written before thumbnails existed still loads.
    #[serde(default)]
    pub has_thumbnail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentStoreError {
    /// The key is not one this store issued (wrong shape, traversal, empty).
    InvalidKey,
    /// No document carries that key.
    NotFound,
    /// The database could not be opened, read or written. Carries what SQLite
    /// said, which is the only part a reader can act on.
    Database(String),
    /// A file beside the database could not be read, created or removed.
    Io(String),
}

impl std::fmt::Display for DocumentStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => write!(f, "invalid document key"),
            Self::NotFound => write!(f, "document not found"),
            Self::Database(detail) => write!(f, "document database: {detail}"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for DocumentStoreError {}

/// Where documents live: `$NORKA_DOCUMENTS_DIR`, else `~/.norka/files`.
///
/// The fallback mirrors `~/.openpencil/templates/` — the daemon already keeps
/// per-user state under the home directory, and a VPS deployment points the
/// variable at a mounted volume instead.
pub fn documents_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DOCUMENTS_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".norka").join("files")
}

/// A fresh key: time-ordered, base32, no padding.
///
/// Time-ordered matters more than it looks: the file list sorts by recency,
/// and a key that already carries the creation instant makes a tie-break
/// cheap and makes two documents created in the same second distinguishable.
pub fn new_key() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let tick = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = Vec::with_capacity(13);
    bytes.extend_from_slice(&millis.to_be_bytes()[2..]); // 48 bits of millis
    bytes.extend_from_slice(&tick.to_be_bytes()[4..]); // 32 bits of counter
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 0x1f) as usize;
            out.push(BASE32[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(BASE32[index] as char);
    }
    out
}

/// Crockford base32, lowercased: no `i`, `l`, `o` or `u`, so a key read aloud
/// or retyped from a chat message has fewer ways to go wrong.
const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Whether `key` is a shape this store could have issued.
///
/// Deliberately strict: `..`, a slash, a leading dot or a Windows drive
/// letter all fail here, which is the only thing standing between a pasted
/// URL and the filesystem.
pub fn key_is_valid(key: &str) -> bool {
    let len = key.len();
    (MIN_KEY_LEN..=MAX_KEY_LEN).contains(&len)
        && key
            .bytes()
            .all(|b| BASE32.contains(&b) || b.is_ascii_digit())
}

fn document_path(dir: &Path, key: &str) -> Result<PathBuf, DocumentStoreError> {
    if !key_is_valid(key) {
        return Err(DocumentStoreError::InvalidKey);
    }
    Ok(dir.join(format!("{key}.op")))
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The row for a document that is about to be written.
///
/// `owner` is the account the document belongs to — the caller's own id in an
/// online deployment, `None` for the local operator (see
/// [`DocumentEntry::owner_id`]).
fn entry_for(
    key: &str,
    name: &str,
    owner: Option<&str>,
    created_at: u64,
    updated_at: u64,
    size: u64,
) -> DocumentEntry {
    DocumentEntry {
        key: key.to_string(),
        name: name.to_string(),
        owner_id: owner.map(str::to_string),
        created_at,
        updated_at,
        size,
        has_thumbnail: false,
    }
}

/// Where a key's document lives on disk.
///
/// Still a function of the directory rather than of the store: the callers that
/// need a path (the document loader, the raster exporter) want the path, and
/// resolving it through the database would only add a query to say what the key
/// already says.
pub fn path_for(dir: &Path, key: &str) -> Result<PathBuf, DocumentStoreError> {
    document_path(dir, key)
}

/// The file list, most recently touched first — every row, whatever account it
/// belongs to.
///
/// This is the operator's view of their own directory, and it is what every
/// caller gets in a deployment that has no accounts. A deployment that HAS
/// accounts must ask [`list_owned_by`] instead: one directory holds every
/// account's documents, so "list everything" is exactly the statement that
/// would show one account another's file names.
pub fn list(db: &DocumentDb) -> Result<Vec<DocumentEntry>, DocumentStoreError> {
    document_db::list_entries(db)
}

/// The file list of one account, most recently touched first.
///
/// Rows with no owner are deliberately absent: nothing attributes them to
/// `owner`, and an unattributed row handed to whoever asks for a list is the
/// leak this function exists to close.
pub fn list_owned_by(
    db: &DocumentDb,
    owner: &str,
) -> Result<Vec<DocumentEntry>, DocumentStoreError> {
    document_db::list_entries_owned_by(db, owner)
}

/// One stored document, when the index has a row for it.
///
/// The row — not the file — is what answers "is this a document of mine": a
/// file that arrived in the directory outside the store has no owner either,
/// and a route that trusted the file would be trusting the filesystem for an
/// authorization decision.
pub fn find(db: &DocumentDb, key: &str) -> Result<Option<DocumentEntry>, DocumentStoreError> {
    document_db::find_entry(db, key)
}

/// Register a new document whose bytes someone else writes.
///
/// The daemon serializes a document straight to its path (the same writer the
/// desktop Save uses), so the store hands out the key and the path rather than
/// copying bytes through itself. The row follows the file: a create that fails
/// to write leaves nothing in the list.
///
/// `owner` is recorded on the row and is the only thing that later attributes
/// the document to anyone; see [`DocumentEntry::owner_id`].
pub fn create_with<F>(
    db: &DocumentDb,
    name: Option<&str>,
    owner: Option<&str>,
    write: F,
) -> Result<DocumentEntry, DocumentStoreError>
where
    F: FnOnce(&Path) -> Result<(), DocumentStoreError>,
{
    let dir = db.dir();
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let key = new_key();
    let path = document_path(dir, &key)?;
    write(&path)?;
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let now = now_secs();
    let entry = entry_for(&key, name.unwrap_or(DEFAULT_NAME), owner, now, now, size);
    document_db::insert_entry(db, &entry)?;
    Ok(entry)
}

/// Refresh an existing document's size and timestamp after a save.
///
/// The file decides whether the document exists — the row follows it, and a row
/// for a document whose file is gone is exactly what this repairs.
pub fn touch(db: &DocumentDb, key: &str) -> Result<DocumentEntry, DocumentStoreError> {
    let path = document_path(db.dir(), key)?;
    if !path.exists() {
        return Err(DocumentStoreError::NotFound);
    }
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    document_db::touch_entry(db, key, now_secs(), size, DEFAULT_NAME)
}

/// Rename a document — the key, and so the address, stays put.
pub fn rename(db: &DocumentDb, key: &str, name: &str) -> Result<DocumentEntry, DocumentStoreError> {
    let path = document_path(db.dir(), key)?;
    if !path.exists() {
        return Err(DocumentStoreError::NotFound);
    }
    document_db::rename_entry(db, key, name, now_secs())?.ok_or(DocumentStoreError::NotFound)
}

/// Remove a document, its row, and whatever pointed at it.
///
/// The last-opened pointer follows the row out through the schema's foreign key
/// (see `crate::document_db`), so a delete cannot leave the daemon coming back
/// to a document that is gone.
pub fn delete(db: &DocumentDb, key: &str) -> Result<(), DocumentStoreError> {
    let path = document_path(db.dir(), key)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DocumentStoreError::NotFound)
        }
        Err(error) => {
            return Err(DocumentStoreError::Io(format!(
                "remove {}: {error}",
                path.display()
            )))
        }
    }
    document_db::delete_entry(db, key)?;
    Ok(())
}

/// Remember `key` as the document to reopen next time.
///
/// The daemon holds one document at a time, so "last open" is a single key per
/// operator, not a session list. Written on every successful open and create.
pub fn remember_last(db: &DocumentDb, key: &str) -> Result<(), DocumentStoreError> {
    if !key_is_valid(key) {
        return Err(DocumentStoreError::InvalidKey);
    }
    // A key no document carries is not a thing to reopen: refusing here is what
    // keeps a start-up from pointing at nothing.
    if document_db::find_entry(db, key)?.is_none() {
        return Err(DocumentStoreError::NotFound);
    }
    document_db::remember_last_opened(db, document_db::LOCAL_OWNER, key)
}

/// The document to reopen on startup, when it still exists.
///
/// Returns `None` for a missing or unreadable record, and for a record whose
/// document has since been deleted — the caller falls back to a fresh
/// document rather than failing to start.
pub fn last_document(db: &DocumentDb) -> Option<DocumentEntry> {
    let entry = document_db::last_entry(db, document_db::LOCAL_OWNER).ok()??;
    // The record is only as good as the file it names.
    path_for(db.dir(), &entry.key)
        .ok()
        .filter(|path| path.exists())?;
    Some(entry)
}

/// Where the unsaved-work draft lives.
///
/// One draft PER OWNER, not one per process. The slot used to be a single file
/// for the whole daemon, which was correct while there was one operator and
/// became a leak the moment a deployment had accounts: the draft holds a whole
/// document's unsaved content, and `GET`/`restore` would have handed account A's
/// work to account B — the per-caller role gate cannot help, because it decides
/// what a caller may do, never whose file this is.
///
/// `owner` is the account whose workspace the draft belongs to; `None` is the
/// local operator, whose slot keeps the original name so an upgrade does not
/// orphan the draft already sitting there. Kept inside the documents directory
/// so a deployment that moves the store (`NORKA_DOCUMENTS_DIR`) moves the draft
/// with it — a recovery file on a different volume would be lost exactly when
/// it is needed.
pub fn recovery_path(dir: &Path, owner: Option<&str>) -> PathBuf {
    match owner {
        None => dir.join("recovery.op"),
        Some(account) => dir.join(owner_slot_file(account)),
    }
}

/// The file name of one account's draft slot.
///
/// A digest, not the account id itself. The id comes from the identity verifier
/// as an opaque string of unbounded length, so a name built from it directly
/// could exceed the filesystem's component limit; and two spellings of the same
/// slot — the digest and a sanitized id — would be two files one account could
/// read through either. 128 bits of SHA-256 is injective in practice, and hex is
/// path-safe by construction: no separator, no `.`-only component, no length
/// surprise.
fn owner_slot_file(account: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(account.as_bytes());
    let mut name = String::with_capacity(9 + 32 + 3);
    name.push_str("recovery.");
    for byte in &digest[..16] {
        name.push_str(&format!("{byte:02x}"));
    }
    name.push_str(".op");
    name
}

/// What is known about a stored draft, for the banner that offers it back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecoveryInfo {
    /// Unix seconds the draft was written.
    pub saved_at: u64,
    pub size: u64,
}

/// Describe `owner`'s draft, when one exists.
pub fn recovery_info(dir: &Path, owner: Option<&str>) -> Option<RecoveryInfo> {
    let path = recovery_path(dir, owner);
    let metadata = std::fs::metadata(&path).ok()?;
    let saved_at = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs())
        .unwrap_or_else(now_secs);
    Some(RecoveryInfo {
        saved_at,
        size: metadata.len(),
    })
}

/// Drop `owner`'s draft — after it has been restored, or refused.
pub fn clear_recovery(dir: &Path, owner: Option<&str>) -> Result<(), DocumentStoreError> {
    match std::fs::remove_file(recovery_path(dir, owner)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DocumentStoreError::Io(format!("remove recovery: {error}"))),
    }
}

/// Where a key's thumbnail lives, when one has been rendered.
pub fn thumb_path(dir: &Path, key: &str) -> Result<PathBuf, DocumentStoreError> {
    if !key_is_valid(key) {
        return Err(DocumentStoreError::InvalidKey);
    }
    Ok(dir.join(format!("{key}.thumb.png")))
}

/// Record that a thumbnail now exists for `key`.
pub fn note_thumbnail(db: &DocumentDb, key: &str, exists: bool) -> Result<(), DocumentStoreError> {
    if document_db::set_thumbnail(db, key, exists)? {
        Ok(())
    } else {
        Err(DocumentStoreError::NotFound)
    }
}

#[cfg(test)]
#[path = "document_store_tests.rs"]
mod tests;
