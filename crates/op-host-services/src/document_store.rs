//! Server-side documents: a directory, an index, and short keys.
//!
//! The browser has no filesystem, so "the file" has to live somewhere the
//! daemon can reach. This is that somewhere: one `.op` per document plus an
//! `index.json` that carries what a file list needs (name, timestamps, size)
//! without opening every document.
//!
//! Keys are short, opaque and validated before they touch a path. The address
//! bar shows them (`/f/<key>`), people paste them into chat, so they must be
//! safe to hand around — which is also why nothing here trusts a key that did
//! not come out of [`new_key`].

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Environment variable naming the documents directory, for tests and for a
/// deployment that wants documents on another volume.
pub const DOCUMENTS_DIR_ENV: &str = "NORKA_DOCUMENTS_DIR";

/// Longest key this store issues; also the acceptance bound for pasted ones.
const MAX_KEY_LEN: usize = 32;
/// Shortest key accepted — short enough for a URL, long enough not to collide.
const MIN_KEY_LEN: usize = 8;

/// One document as the file list sees it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocumentEntry {
    pub key: String,
    pub name: String,
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
    /// The index exists but cannot be read as one.
    CorruptIndex(String),
    Io(String),
}

impl std::fmt::Display for DocumentStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => write!(f, "invalid document key"),
            Self::NotFound => write!(f, "document not found"),
            Self::CorruptIndex(detail) => write!(f, "document index unreadable: {detail}"),
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
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
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

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read the index, treating a missing file as an empty store.
pub fn list(dir: &Path) -> Result<Vec<DocumentEntry>, DocumentStoreError> {
    let path = index_path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(DocumentStoreError::Io(format!("read index: {error}"))),
    };
    let mut entries: Vec<DocumentEntry> = serde_json::from_str(&raw)
        .map_err(|error| DocumentStoreError::CorruptIndex(error.to_string()))?;
    // Most recently touched first: the file list is a recency list.
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.key.cmp(&a.key)));
    Ok(entries)
}

fn write_index(dir: &Path, entries: &[DocumentEntry]) -> Result<(), DocumentStoreError> {
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let raw = serde_json::to_string_pretty(entries)
        .map_err(|error| DocumentStoreError::Io(format!("serialize index: {error}")))?;
    // Write-then-rename: a crash mid-write must not leave a half index, and
    // readers never observe a partial file because rename is atomic.
    let tmp = dir.join("index.json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|error| DocumentStoreError::Io(format!("create index: {error}")))?;
        file.write_all(raw.as_bytes())
            .map_err(|error| DocumentStoreError::Io(format!("write index: {error}")))?;
    }
    std::fs::rename(&tmp, index_path(dir))
        .map_err(|error| DocumentStoreError::Io(format!("replace index: {error}")))
}

fn entry_for(key: &str, name: &str, created_at: u64, updated_at: u64) -> DocumentEntry {
    DocumentEntry {
        key: key.to_string(),
        name: name.to_string(),
        created_at,
        updated_at,
        size: 0,
        has_thumbnail: false,
    }
}

/// Register a new document and write its first bytes.
pub fn create(
    dir: &Path,
    name: Option<&str>,
    bytes: &[u8],
) -> Result<DocumentEntry, DocumentStoreError> {
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let key = new_key();
    write_document(dir, &key, bytes)?;
    let now = now_secs();
    let mut entries = list(dir)?;
    let mut entry = entry_for(&key, name.unwrap_or("Untitled"), now, now);
    entry.size = bytes.len() as u64;
    entries.insert(0, entry.clone());
    write_index(dir, &entries)?;
    Ok(entry)
}

/// File recording which document was last open, so a restart returns to it.
const LAST_DOCUMENT_FILE: &str = "last.json";

/// Remember `key` as the document to reopen next time.
///
/// The daemon holds one document at a time, so "last open" is a single key,
/// not a session list. Written on every successful open and create.
pub fn remember_last(dir: &Path, key: &str) -> Result<(), DocumentStoreError> {
    if !key_is_valid(key) {
        return Err(DocumentStoreError::InvalidKey);
    }
    let Some(entry) = list(dir)?.into_iter().find(|entry| entry.key == key) else {
        return Err(DocumentStoreError::NotFound);
    };
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let raw = serde_json::to_string_pretty(&entry)
        .map_err(|error| DocumentStoreError::Io(format!("serialize last: {error}")))?;
    let tmp = dir.join("last.json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|error| DocumentStoreError::Io(format!("create last: {error}")))?;
        file.write_all(raw.as_bytes())
            .map_err(|error| DocumentStoreError::Io(format!("write last: {error}")))?;
    }
    std::fs::rename(&tmp, dir.join(LAST_DOCUMENT_FILE))
        .map_err(|error| DocumentStoreError::Io(format!("replace last: {error}")))
}

/// The document to reopen on startup, when it still exists.
///
/// Returns `None` for a missing or unreadable record, and for a record whose
/// document has since been deleted — the caller falls back to a fresh
/// document rather than failing to start.
pub fn last_document(dir: &Path) -> Option<DocumentEntry> {
    let raw = std::fs::read_to_string(dir.join(LAST_DOCUMENT_FILE)).ok()?;
    let entry: DocumentEntry = serde_json::from_str(&raw).ok()?;
    // The record is only as good as the file it names.
    document_path(dir, &entry.key).ok().filter(|path| path.exists())?;
    Some(entry)
}

/// Where a key's thumbnail lives, when one has been rendered.
pub fn thumb_path(dir: &Path, key: &str) -> Result<PathBuf, DocumentStoreError> {
    if !key_is_valid(key) {
        return Err(DocumentStoreError::InvalidKey);
    }
    Ok(dir.join(format!("{key}.thumb.png")))
}

/// Record that a thumbnail now exists for `key`.
pub fn note_thumbnail(dir: &Path, key: &str, exists: bool) -> Result<(), DocumentStoreError> {
    let mut entries = list(dir)?;
    let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) else {
        return Err(DocumentStoreError::NotFound);
    };
    if entry.has_thumbnail == exists {
        return Ok(());
    }
    entry.has_thumbnail = exists;
    write_index(dir, &entries)
}

/// Where a key's document lives on disk.
pub fn path_for(dir: &Path, key: &str) -> Result<PathBuf, DocumentStoreError> {
    document_path(dir, key)
}

/// Register a new document whose bytes someone else writes.
///
/// The daemon serializes a document straight to its path (the same writer the
/// desktop Save uses), so the store hands out the key and the path rather
/// than copying bytes through itself.
pub fn create_with<F>(
    dir: &Path,
    name: Option<&str>,
    write: F,
) -> Result<DocumentEntry, DocumentStoreError>
where
    F: FnOnce(&Path) -> Result<(), DocumentStoreError>,
{
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let key = new_key();
    let path = document_path(dir, &key)?;
    write(&path)?;
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let now = now_secs();
    let mut entries = list(dir)?;
    let mut entry = entry_for(&key, name.unwrap_or("Untitled"), now, now);
    entry.size = size;
    entries.insert(0, entry.clone());
    write_index(dir, &entries)?;
    Ok(entry)
}

/// Refresh an existing document's size and timestamp after a save.
pub fn touch(dir: &Path, key: &str) -> Result<DocumentEntry, DocumentStoreError> {
    if !document_path(dir, key)?.exists() {
        return Err(DocumentStoreError::NotFound);
    }
    let size = std::fs::metadata(document_path(dir, key)?)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let mut entries = list(dir)?;
    let now = now_secs();
    let position = entries.iter().position(|entry| entry.key == key);
    let mut entry = match position {
        Some(index) => entries.remove(index),
        None => entry_for(key, "Untitled", now, now),
    };
    entry.updated_at = now;
    entry.size = size;
    entries.insert(0, entry.clone());
    write_index(dir, &entries)?;
    Ok(entry)
}

/// Write a document's bytes, creating it when the file is missing.
pub fn write_document(dir: &Path, key: &str, bytes: &[u8]) -> Result<(), DocumentStoreError> {
    let path = document_path(dir, key)?;
    std::fs::create_dir_all(dir)
        .map_err(|error| DocumentStoreError::Io(format!("create {}: {error}", dir.display())))?;
    std::fs::write(&path, bytes)
        .map_err(|error| DocumentStoreError::Io(format!("write {}: {error}", path.display())))
}

/// Read a document's bytes.
pub fn read_document(dir: &Path, key: &str) -> Result<Vec<u8>, DocumentStoreError> {
    let path = document_path(dir, key)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(DocumentStoreError::NotFound)
        }
        Err(error) => Err(DocumentStoreError::Io(format!("read {}: {error}", path.display()))),
    }
}

/// Replace an existing document's bytes and refresh its index entry.
pub fn save(
    dir: &Path,
    key: &str,
    name: Option<&str>,
    bytes: &[u8],
) -> Result<DocumentEntry, DocumentStoreError> {
    if !document_path(dir, key)?.exists() {
        return Err(DocumentStoreError::NotFound);
    }
    write_document(dir, key, bytes)?;
    let mut entries = list(dir)?;
    let now = now_secs();
    let position = entries.iter().position(|entry| entry.key == key);
    let mut entry = match position {
        Some(index) => entries.remove(index),
        None => entry_for(key, name.unwrap_or("Untitled"), now, now),
    };
    entry.updated_at = now;
    entry.size = bytes.len() as u64;
    if let Some(name) = name {
        entry.name = name.to_string();
    }
    entries.insert(0, entry.clone());
    write_index(dir, &entries)?;
    Ok(entry)
}

/// Remove a document and its index entry.
pub fn delete(dir: &Path, key: &str) -> Result<(), DocumentStoreError> {
    let path = document_path(dir, key)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DocumentStoreError::NotFound)
        }
        Err(error) => return Err(DocumentStoreError::Io(format!("remove {}: {error}", path.display()))),
    }
    let entries: Vec<DocumentEntry> = list(dir)?
        .into_iter()
        .filter(|entry| entry.key != key)
        .collect();
    write_index(dir, &entries)
}

/// Rename a document — the key, and so the address, stays put.
pub fn rename(dir: &Path, key: &str, name: &str) -> Result<DocumentEntry, DocumentStoreError> {
    if !document_path(dir, key)?.exists() {
        return Err(DocumentStoreError::NotFound);
    }
    let mut entries = list(dir)?;
    let Some(position) = entries.iter().position(|entry| entry.key == key) else {
        return Err(DocumentStoreError::NotFound);
    };
    entries[position].name = name.to_string();
    entries[position].updated_at = now_secs();
    let entry = entries[position].clone();
    write_index(dir, &entries)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "norka-store-{tag}-{}-{}",
                std::process::id(),
                new_key()
            ));
            std::fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_new_key_is_usable_in_a_path() {
        let key = new_key();
        assert!(key_is_valid(&key), "{key}");
        assert!(!key.contains('/'));
        assert!(key.len() >= MIN_KEY_LEN && key.len() <= MAX_KEY_LEN);
    }

    #[test]
    fn suspicious_keys_are_refused_before_touching_a_path() {
        for key in ["", "..", "../etc/passwd", "abc/def", ".hidden..", "SHORT", "i-l-o"] {
            assert!(!key_is_valid(key), "{key} must not be a key");
            assert_eq!(
                read_document(Path::new("/tmp"), key),
                Err(DocumentStoreError::InvalidKey),
                "{key}"
            );
        }
    }

    #[test]
    fn create_list_read_and_delete_round_trip() {
        let dir = TempDir::new("round-trip");
        let entry = create(dir.path(), Some("Список токенов"), b"first").expect("create");
        assert_eq!(entry.name, "Список токенов");
        assert_eq!(entry.size, 5);

        let listed = list(dir.path()).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, entry.key);

        assert_eq!(read_document(dir.path(), &entry.key).expect("read"), b"first");

        delete(dir.path(), &entry.key).expect("delete");
        assert!(list(dir.path()).expect("list").is_empty());
        assert_eq!(
            read_document(dir.path(), &entry.key),
            Err(DocumentStoreError::NotFound)
        );
    }

    #[test]
    fn saving_refreshes_the_entry_and_keeps_one_row() {
        let dir = TempDir::new("save");
        let entry = create(dir.path(), Some("Draft"), b"one").expect("create");
        let saved = save(dir.path(), &entry.key, None, b"two-longer").expect("save");
        assert_eq!(saved.key, entry.key);
        assert_eq!(saved.size, 10);
        assert_eq!(saved.name, "Draft");
        let listed = list(dir.path()).expect("list");
        assert_eq!(listed.len(), 1, "a save must not add a second row");
        assert_eq!(read_document(dir.path(), &entry.key).expect("read"), b"two-longer");
    }

    #[test]
    fn saving_an_unknown_key_is_not_found() {
        let dir = TempDir::new("save-missing");
        let key = new_key();
        assert_eq!(
            save(dir.path(), &key, None, b"x"),
            Err(DocumentStoreError::NotFound)
        );
    }

    #[test]
    fn renaming_keeps_the_key_so_links_survive() {
        let dir = TempDir::new("rename");
        let entry = create(dir.path(), Some("Before"), b"body").expect("create");
        let renamed = rename(dir.path(), &entry.key, "After").expect("rename");
        assert_eq!(renamed.key, entry.key);
        assert_eq!(renamed.name, "After");
        assert_eq!(read_document(dir.path(), &entry.key).expect("read"), b"body");
    }

    #[test]
    fn the_list_is_most_recent_first() {
        let dir = TempDir::new("order");
        let first = create(dir.path(), Some("First"), b"1").expect("create");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = create(dir.path(), Some("Second"), b"2").expect("create");
        let listed = list(dir.path()).expect("list");
        assert_eq!(listed[0].key, second.key, "the newer document leads");
        assert_eq!(listed[1].key, first.key);
    }

    #[test]
    fn a_missing_index_reads_as_empty_and_a_broken_one_is_reported() {
        let dir = TempDir::new("index");
        assert!(list(dir.path()).expect("missing index").is_empty());
        std::fs::write(index_path(dir.path()), "{ not json").expect("write");
        assert!(matches!(
            list(dir.path()),
            Err(DocumentStoreError::CorruptIndex(_))
        ));
    }

    #[test]
    fn the_directory_can_be_pointed_somewhere_else() {
        let previous = std::env::var_os(DOCUMENTS_DIR_ENV);
        // SAFETY: this test owns the variable for its duration; the store is
        // not read concurrently here.
        unsafe { std::env::set_var(DOCUMENTS_DIR_ENV, "/tmp/norka-docs-test") };
        assert_eq!(documents_dir(), PathBuf::from("/tmp/norka-docs-test"));
        match previous {
            Some(value) => unsafe { std::env::set_var(DOCUMENTS_DIR_ENV, value) },
            None => unsafe { std::env::remove_var(DOCUMENTS_DIR_ENV) },
        }
    }
}
