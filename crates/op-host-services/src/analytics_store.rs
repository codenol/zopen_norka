//! Analytics assets: the markdown documents a section is built from.
//!
//! Analytics is a document in its own right — what a feature is for, its use
//! cases, what the data says — and an ASSET: loaded once and referenced, the way
//! a component or a recipe is. A section references it; several sections may
//! reference the same one, and editing it edits something shared, with the
//! consequences that has.
//!
//! ## Where the markdown lives
//!
//! One file per asset, `<documents dir>/analytics/<key>.md`, beside the `.op`
//! files and accounted for by a row in the same database
//! (`analytics_assets`, migration 4). A file rather than a column because the
//! operator's reason for markdown is that a person who is not in this app can
//! read it: a real file with a real format, which git can version and a text
//! editor can open. The row is the accounting — who it belongs to, what it is
//! called, when it changed — exactly the split [`crate::document_store`] makes
//! for the `.op` files themselves.
//!
//! ## How it is addressed
//!
//! By the same short key the document store issues ([`new_key`]): time-ordered
//! base32, validated by [`key_is_valid`] before it is ever joined to a path. So
//! an asset can be named in a URL (`/a/<key>`), pasted into a chat, and read
//! back by a route that decides on it — and a key that did not come out of this
//! code can never reach the filesystem.
//!
//! ## Why the digest is computed, never stored
//!
//! A link remembers the analytics digest it was made at
//! ([`op_editor_core::section::AnalyticsLink`]), and the CURRENT digest is
//! recomputed from the bytes every time it is asked for. A stored copy would be
//! a second source of truth, and the moment it disagreed with the file — which
//! is exactly what happens when somebody edits the markdown outside the app,
//! the case the file format exists for — the section would report a change that
//! had already been made, or miss one. Reading a few kilobytes is cheap; being
//! subtly wrong about whether the design still matches its reasoning is not.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row};

use op_editor_core::section::{analytics_fingerprint, SectionDigest};

use crate::document_db::{db_error, DocumentDb};
use crate::document_store::{key_is_valid, new_key, now_secs};
use crate::section_store_error::SectionStoreError;

/// Longest markdown accepted, in bytes.
///
/// A ceiling against abuse rather than an editorial judgement: an asset is read
/// by everyone who can open a section that references it, and a public
/// deployment must not become a place to park unbounded data. One mebibyte is
/// several hundred pages of prose — far past any analytics brief — and it is
/// checked before the write so the refusal names the limit rather than the
/// disk filling up.
pub(crate) const MAX_ANALYTICS_BYTES: usize = 1_048_576;

/// Longest asset name accepted, in characters.
///
/// Characters, not bytes: the name is prose in whatever script the workspace
/// writes in, and a byte limit would give a Russian name half the room of a
/// Latin one. The bound exists because the name is shown in a list.
pub(crate) const MAX_ANALYTICS_NAME_CHARS: usize = 200;

/// Directory, inside the documents directory, where the assets live.
///
/// Its own directory rather than loose among the `.op` files: the two are
/// different things with different lifetimes — a document is edited and saved,
/// an asset is loaded and referenced — and one listing that mixed them would
/// make the file screen show assets it cannot open.
const ANALYTICS_DIR: &str = "analytics";

/// The columns an [`AnalyticsAsset`] is built from, in one place so every
/// reader agrees about the order.
const ASSET_COLUMNS: &str = "key, name, owner_id, created_at, updated_at, size";

/// One analytics document, as the store accounts for it.
///
/// The bytes are not here: this is the record, and [`read`] hands back the
/// document with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalyticsAsset {
    /// Short, opaque, URL-safe address.
    pub key: String,
    /// What a person reads in a list.
    pub name: String,
    /// The account this asset belongs to. `None` is the local operator, who has
    /// no account — the same meaning `documents.owner_id` carries.
    pub owner_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub size: u64,
}

/// An asset with its markdown, and the digest of that markdown as it is now.
///
/// The three arrive together because a caller that has the text almost always
/// needs the digest too — a link records it — and computing it at the moment of
/// reading is the only way it cannot go stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalyticsDocument {
    pub asset: AnalyticsAsset,
    pub markdown: String,
    pub digest: SectionDigest,
}

/// Where the assets of one documents directory live.
pub(crate) fn assets_dir(dir: &Path) -> PathBuf {
    dir.join(ANALYTICS_DIR)
}

/// The path of one asset, refusing a key that is not a key.
///
/// The validation is the whole guard: every path this module builds goes
/// through here, so a key with `..`, a slash or a drive letter in it never
/// reaches the filesystem, whoever pasted it.
pub(crate) fn asset_path(dir: &Path, key: &str) -> Result<PathBuf, SectionStoreError> {
    if !key_is_valid(key) {
        return Err(SectionStoreError::InvalidKey);
    }
    Ok(assets_dir(dir).join(format!("{key}.md")))
}

/// Load a new analytics document into the store.
///
/// The file is written first and the row second: a row always accounts for a
/// file that exists, where the other order would leave an asset in the list
/// whose markdown never arrived.
pub(crate) fn create(
    db: &DocumentDb,
    name: &str,
    owner: Option<&str>,
    markdown: &str,
) -> Result<AnalyticsAsset, SectionStoreError> {
    check_name(name)?;
    check_size(markdown)?;
    let dir = assets_dir(db.dir());
    std::fs::create_dir_all(&dir)
        .map_err(|error| SectionStoreError::Io(format!("create {}: {error}", dir.display())))?;
    let key = new_key();
    let path = asset_path(db.dir(), &key)?;
    std::fs::write(&path, markdown)
        .map_err(|error| SectionStoreError::Io(format!("write {}: {error}", path.display())))?;
    let now = now_secs();
    let asset = AnalyticsAsset {
        key,
        name: name.to_string(),
        owner_id: owner.map(str::to_string),
        created_at: now,
        updated_at: now,
        size: markdown.len() as u64,
    };
    insert(&db.conn(), &asset)?;
    Ok(asset)
}

/// Replace an asset's markdown.
pub(crate) fn write(
    db: &DocumentDb,
    key: &str,
    markdown: &str,
) -> Result<AnalyticsDocument, SectionStoreError> {
    check_size(markdown)?;
    let path = asset_path(db.dir(), key)?;
    // The row decides whether the asset exists — see [`find`] — and it is
    // checked before the file is touched, so a write to a key nobody issued
    // cannot leave a file behind that no row accounts for.
    if find(db, key)?.is_none() {
        return Err(SectionStoreError::NotFound);
    }
    std::fs::write(&path, markdown)
        .map_err(|error| SectionStoreError::Io(format!("write {}: {error}", path.display())))?;
    let now = now_secs();
    db.conn()
        .execute(
            "UPDATE analytics_assets SET updated_at = ?2, size = ?3 WHERE key = ?1",
            params![key, now, markdown.len() as u64],
        )
        .map_err(db_error)?;
    read(db, key)
}

/// One asset with its markdown.
pub(crate) fn read(db: &DocumentDb, key: &str) -> Result<AnalyticsDocument, SectionStoreError> {
    let asset = find(db, key)?.ok_or(SectionStoreError::NotFound)?;
    let path = asset_path(db.dir(), key)?;
    let markdown = std::fs::read_to_string(&path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => SectionStoreError::MissingFile {
            path: path.display().to_string(),
        },
        _ => SectionStoreError::Io(format!("read {}: {error}", path.display())),
    })?;
    // A file that is not UTF-8 is `InvalidData` and lands in `Io` above: the
    // asset is markdown, and text this build cannot decode is text no reader
    // can read either.
    let digest = analytics_fingerprint(&markdown);
    Ok(AnalyticsDocument {
        asset,
        markdown,
        digest,
    })
}

/// The digest of an asset's markdown as it is now.
///
/// `Ok(None)` means there is no such asset. An asset whose file is gone is an
/// error rather than `None` — see [`SectionStoreError::MissingFile`] — because a
/// section that references it must not be told the analytics was deleted when
/// nobody deleted it.
pub(crate) fn digest(
    db: &DocumentDb,
    key: &str,
) -> Result<Option<SectionDigest>, SectionStoreError> {
    match read(db, key) {
        Ok(document) => Ok(Some(document.digest)),
        Err(SectionStoreError::NotFound) => Ok(None),
        Err(other) => Err(other),
    }
}

/// One asset's record, when it is there.
pub(crate) fn find(
    db: &DocumentDb,
    key: &str,
) -> Result<Option<AnalyticsAsset>, SectionStoreError> {
    if !key_is_valid(key) {
        return Err(SectionStoreError::InvalidKey);
    }
    find_by_conn(&db.conn(), key)
}

/// The assets of one account, most recently touched first.
///
/// `None` asks for the unattributed ones, which is what a deployment with no
/// accounts has. There is deliberately no "list everything": one directory
/// holds every account's assets, so that statement would show one account
/// another's work — the same rule the file list follows.
pub(crate) fn list(
    db: &DocumentDb,
    owner: Option<&str>,
) -> Result<Vec<AnalyticsAsset>, SectionStoreError> {
    let conn = db.conn();
    // Two SQL shapes rather than `(?1 IS NULL OR owner_id = ?1)`: the second
    // hands the planner a term it cannot use as an index probe, and NULL is not
    // "no filter" in SQL anyway.
    let sql = match owner {
        Some(_) => format!(
            "SELECT {ASSET_COLUMNS} FROM analytics_assets
             WHERE owner_id = ?1 ORDER BY updated_at DESC, key DESC"
        ),
        None => format!(
            "SELECT {ASSET_COLUMNS} FROM analytics_assets
             WHERE owner_id IS NULL ORDER BY updated_at DESC, key DESC"
        ),
    };
    let mut statement = conn.prepare(&sql).map_err(db_error)?;
    let rows = match owner {
        Some(owner) => statement.query_map(params![owner], asset_from_row),
        None => statement.query_map([], asset_from_row),
    }
    .map_err(db_error)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?)
}

/// Set an asset's name. The key — and so the address — stays put.
pub(crate) fn rename(
    db: &DocumentDb,
    key: &str,
    name: &str,
) -> Result<AnalyticsAsset, SectionStoreError> {
    check_name(name)?;
    if !key_is_valid(key) {
        return Err(SectionStoreError::InvalidKey);
    }
    let conn = db.conn();
    let changed = conn
        .execute(
            "UPDATE analytics_assets SET name = ?2, updated_at = ?3 WHERE key = ?1",
            params![key, name, now_secs()],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Err(SectionStoreError::NotFound);
    }
    find_by_conn(&conn, key)?.ok_or(SectionStoreError::NotFound)
}

/// Remove an asset: its file and its row.
///
/// Both, and in this order. A row left behind after the file went would make
/// every section that references the asset report a broken store instead of the
/// truth, and a file left behind after the row went would be a document nothing
/// points at and nothing lists.
///
/// What this does NOT do is touch the sections that reference it. Their links
/// stay, and they will report [`op_editor_core::section::LinkState::AssetMissing`]
/// — which is the honest answer: the sections did not change, something they
/// were built from went away.
pub(crate) fn delete(db: &DocumentDb, key: &str) -> Result<(), SectionStoreError> {
    let path = asset_path(db.dir(), key)?;
    let conn = db.conn();
    let removed = conn
        .execute("DELETE FROM analytics_assets WHERE key = ?1", params![key])
        .map_err(db_error)?;
    if removed == 0 {
        return Err(SectionStoreError::NotFound);
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        // The row is gone, which is what "this asset is deleted" means; a file
        // that was already missing is the state we were trying to reach.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SectionStoreError::Io(format!(
            "remove {}: {error}",
            path.display()
        ))),
    }
}

/// Add an asset's row.
fn insert(conn: &Connection, asset: &AnalyticsAsset) -> Result<(), SectionStoreError> {
    conn.execute(
        "INSERT INTO analytics_assets (key, name, owner_id, created_at, updated_at, size)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            asset.key,
            asset.name,
            asset.owner_id,
            asset.created_at,
            asset.updated_at,
            asset.size,
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// [`find`] for a caller that already holds the connection — the mutex is not
/// reentrant, so a function holding the guard must never call back into the
/// database handle.
fn find_by_conn(conn: &Connection, key: &str) -> Result<Option<AnalyticsAsset>, SectionStoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {ASSET_COLUMNS} FROM analytics_assets WHERE key = ?1"),
            params![key],
            asset_from_row,
        )
        .optional()
        .map_err(db_error)?)
}

/// Read an [`AnalyticsAsset`] out of the columns [`ASSET_COLUMNS`] names.
fn asset_from_row(row: &Row<'_>) -> rusqlite::Result<AnalyticsAsset> {
    Ok(AnalyticsAsset {
        key: row.get(0)?,
        name: row.get(1)?,
        owner_id: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        size: row.get(5)?,
    })
}

/// Refuse a name this store will not keep.
fn check_name(name: &str) -> Result<(), SectionStoreError> {
    let chars = name.chars().count();
    if chars > MAX_ANALYTICS_NAME_CHARS {
        return Err(SectionStoreError::NameTooLong {
            chars,
            max: MAX_ANALYTICS_NAME_CHARS,
        });
    }
    Ok(())
}

/// Refuse a document this store will not keep.
fn check_size(markdown: &str) -> Result<(), SectionStoreError> {
    if markdown.len() > MAX_ANALYTICS_BYTES {
        return Err(SectionStoreError::TooLarge {
            bytes: markdown.len(),
            max: MAX_ANALYTICS_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "analytics_store_tests.rs"]
mod tests;
