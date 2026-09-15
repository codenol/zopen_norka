//! Where an evicted tenant's document goes, and how it comes back.
//!
//! M1 kept tenants purely in memory, so eviction lost the document and a
//! returning account got a starter. This is the other half: eviction writes,
//! and a cache miss reads.
//!
//! ## Directory naming
//!
//! The directory is `SHA-256(user_id)`, first 16 hex characters — never the
//! account id itself. An account id is a string the daemon did not choose,
//! and joining an unvetted string onto a path is how `../` gets to walk out
//! of the data directory. Hashing removes the question entirely: the output
//! alphabet is `[0-9a-f]`, so there is no traversal to defend against, no
//! case-collision on a case-insensitive filesystem, and no length limit to
//! worry about.
//!
//! ## Writing
//!
//! Temp file in the same directory, then `rename`. A crash therefore leaves
//! either the previous document or the new one, never a half-written file
//! that would fail to load on the account's next visit.
//!
//! ## Reading a file that will not load
//!
//! Renamed to `.corrupt` and the account gets a starter document. Deleting it
//! would destroy the only copy of whatever the user had; silently overwriting
//! it does the same thing one save later. Keeping it costs a few bytes and is
//! the difference between "we can look at it" and "it is gone".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use op_editor_core::EditorState;
use sha2::{Digest, Sha256};

/// Root of the on-disk tenant store. Unset disables persistence entirely,
/// which is what a demo or a test wants.
pub const DATA_DIR_ENV: &str = "OPENPENCIL_ONLINE_DATA_DIR";

/// The document file inside a tenant directory.
const DOCUMENT_FILE: &str = "current.op";
/// The access list inside a tenant directory.
const ACL_FILE: &str = "acl.json";
/// Suffix a file that would not load is moved aside under.
const CORRUPT_SUFFIX: &str = "corrupt";

/// Longest access list that is written or read back.
///
/// A share list is a handful of accounts; a larger one is a bug or an attempt
/// to make the daemon allocate on every eviction.
pub(super) const MAX_SHARED_ACCOUNTS: usize = 256;

/// Why a tenant could not be persisted or restored.
///
/// Every variant is non-fatal at the call site: a failed save leaves the
/// previous file, and a failed load yields a starter document. Persistence
/// must never be able to take the service down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantStoreError {
    /// No data directory is configured, so there is nothing to read or write.
    Disabled,
    /// The tenant has nothing stored yet.
    NotStored,
    /// The stored file could not be read or written.
    Io(String),
    /// The stored document exists but could not be loaded. It has been moved
    /// aside; the caller should start fresh.
    Unreadable(String),
    /// The access list is already at its ceiling. A client fault, not an IO
    /// one — reported so the grant is refused rather than accepted and then
    /// silently dropped by the bounded write.
    ShareLimitReached(usize),
}

impl std::fmt::Display for TenantStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("tenant persistence is not configured"),
            Self::NotStored => f.write_str("no stored document for this account"),
            Self::Io(detail) => write!(f, "tenant store IO failed: {detail}"),
            Self::Unreadable(detail) => {
                write!(f, "stored document could not be loaded: {detail}")
            }
            Self::ShareLimitReached(limit) => {
                write!(f, "this document is already shared with {limit} accounts")
            }
        }
    }
}

impl std::error::Error for TenantStoreError {}

/// Every access list one account's documents have, keyed by document key.
///
/// The file is one per ACCOUNT rather than one per document: a deployment's
/// account directory already exists, the lists are small and read together on
/// restore, and a directory of lists would mean walking it on every grant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredAcls {
    /// Document key → its access list. A document with no entry has no list,
    /// which is not the same as a list with nobody on it: the first is a
    /// document nobody has shared, the second one shared and then revoked.
    pub documents: std::collections::BTreeMap<String, TenantAcl>,
}

/// The format version written today.
const ACL_FORMAT_VERSION: u64 = 2;

/// One document's access list, exactly as the tenant holds it.
///
/// A re-export rather than a second struct: the in-memory ACL and the stored
/// one are the same three fields, and two definitions is how they drift — a
/// field added to one would be silently dropped by every save.
pub use super::tenant::DocumentAcl as TenantAcl;

/// The on-disk tenant store.
#[derive(Debug, Clone)]
pub struct TenantStore {
    root: Option<PathBuf>,
}

impl TenantStore {
    /// Build from the environment. No data directory means no persistence.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var(DATA_DIR_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        )
    }

    pub const fn new(root: Option<PathBuf>) -> Self {
        Self { root }
    }

    pub const fn is_enabled(&self) -> bool {
        self.root.is_some()
    }

    /// Prove at start-up that this process can actually write the data
    /// directory.
    ///
    /// A read-only mount, or a volume owned by root while the container runs
    /// as UID 10001, fails every eviction — silently, half an hour after
    /// start, one account at a time. Exercising the full create → write →
    /// rename → delete cycle here turns that into a start-up failure that
    /// names the likely cause.
    pub fn check_writable(&self) -> Result<(), TenantStoreError> {
        let Some(root) = self.root.as_ref() else {
            return Ok(());
        };
        let probe_dir = root.join(".probe");
        let describe = |error: std::io::Error| {
            TenantStoreError::Io(format!(
                "{error} (is {} writable by the user this process runs as? \
                 the container image runs as UID 10001)",
                root.display()
            ))
        };
        std::fs::create_dir_all(&probe_dir).map_err(describe)?;
        let staged = probe_dir.join("write.tmp");
        let landed = probe_dir.join("write.ok");
        std::fs::write(&staged, b"probe").map_err(describe)?;
        // Rename specifically: the atomic publish every save depends on can
        // fail where a plain write succeeds (some network filesystems).
        std::fs::rename(&staged, &landed).map_err(describe)?;
        std::fs::remove_file(&landed).map_err(describe)?;
        let _ = std::fs::remove_dir(&probe_dir);
        Ok(())
    }

    /// The directory holding `user_id`'s tenant, if persistence is on.
    ///
    /// See the module docs for why this is a hash and not the id.
    pub fn tenant_dir(&self, user_id: &str) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join(dir_name(user_id)))
    }

    /// Write a tenant's document and access list.
    ///
    /// Called during eviction, while the registry lock is held and the tenant
    /// has no leases — so nothing can be writing the document underneath it.
    pub fn save(
        &self,
        user_id: &str,
        state: &EditorState,
        acls: &std::collections::BTreeMap<String, TenantAcl>,
    ) -> Result<(), TenantStoreError> {
        let Some(dir) = self.tenant_dir(user_id) else {
            return Err(TenantStoreError::Disabled);
        };
        std::fs::create_dir_all(&dir).map_err(|error| TenantStoreError::Io(error.to_string()))?;
        // The same streaming, atomic writer desktop Save uses — but WITHOUT
        // capturing the process-global thumbnail registry, which has no tenant
        // dimension and would otherwise persist whichever account activated
        // last into this account's file.
        crate::doc_io::save_to_path_without_thumbnails(state, &dir.join(DOCUMENT_FILE))
            .map_err(|error| TenantStoreError::Io(error.to_string()))?;
        self.save_acls(&dir, acls)
    }

    /// Write just the access list. Used when a grant or revoke should survive
    /// a restart even though the document has not changed.
    pub fn save_acls_for(
        &self,
        user_id: &str,
        acls: &std::collections::BTreeMap<String, TenantAcl>,
    ) -> Result<(), TenantStoreError> {
        let Some(dir) = self.tenant_dir(user_id) else {
            return Err(TenantStoreError::Disabled);
        };
        std::fs::create_dir_all(&dir).map_err(|error| TenantStoreError::Io(error.to_string()))?;
        self.save_acls(&dir, acls)
    }

    fn save_acls(
        &self,
        dir: &Path,
        acls: &std::collections::BTreeMap<String, TenantAcl>,
    ) -> Result<(), TenantStoreError> {
        let documents: serde_json::Map<String, serde_json::Value> = acls
            .iter()
            .filter(|(key, _)| !key.trim().is_empty())
            .map(|(key, acl)| (key.clone(), acl_json(acl)))
            .collect();
        let body = serde_json::json!({
            "version": ACL_FORMAT_VERSION,
            "documents": documents,
        });
        atomic_write(&dir.join(ACL_FILE), body.to_string().as_bytes())
    }

    /// Restore a tenant's document, if one was stored.
    ///
    /// A file that will not load is moved aside (see the module docs) and
    /// reported as [`TenantStoreError::Unreadable`], so the caller starts the
    /// account fresh rather than failing its request.
    pub fn load_document(&self, user_id: &str) -> Result<EditorState, TenantStoreError> {
        let Some(dir) = self.tenant_dir(user_id) else {
            return Err(TenantStoreError::Disabled);
        };
        let path = dir.join(DOCUMENT_FILE);
        if !path.is_file() {
            return Err(TenantStoreError::NotStored);
        }
        // Restored without publishing thumbnails: activation replaces the
        // process-global registry wholesale, so one account's restore would
        // discard every other account's.
        match crate::doc_io::load_editor_state_without_thumbnails(
            &path,
            op_editor_core::Locale::EnUs,
        ) {
            Ok(state) => Ok(state),
            Err(error) => {
                let detail = error.to_string();
                quarantine(&path);
                Err(TenantStoreError::Unreadable(detail))
            }
        }
    }

    /// Restore one document's access list. A missing or unreadable list is an
    /// empty one — failing closed, since a list only ever grants access.
    pub fn load_acl(&self, user_id: &str, key: &str) -> TenantAcl {
        self.load_acl_file(user_id)
            .documents
            .remove(key)
            .unwrap_or_default()
    }

    /// Restore the whole ACL: membership, the metadata beside it, and whether
    /// the document is open to anybody with the link.
    ///
    /// A file that will not parse is quarantined and read as empty, because
    /// every field here only ever GRANTS access — the fail-closed reading of a
    /// corrupt access list is an empty one. The metadata is filtered against
    /// the membership for the same reason: a level for an account the list no
    /// longer names is a share that does not exist.
    pub fn load_acl_file(&self, user_id: &str) -> StoredAcls {
        let Some(dir) = self.tenant_dir(user_id) else {
            return StoredAcls::default();
        };
        let path = dir.join(ACL_FILE);
        let Ok(body) = std::fs::read_to_string(&path) else {
            return StoredAcls::default();
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
            quarantine(&path);
            return StoredAcls::default();
        };
        // The v1 file held ONE list for the whole account. It cannot be
        // migrated: it never recorded which document it was about, and
        // guessing — applying it to whichever document the account opens next
        // — would hand out access the operator never granted for that
        // document. So it is read as empty and reported, and every share has
        // to be issued again. Fail closed, loudly.
        let Some(documents) = parsed.get("documents").and_then(|value| value.as_object()) else {
            if parsed.get("sharedWith").is_some() {
                eprintln!(
                    "openpencil: {ACL_FORMAT_VERSION}-format access list expected, found the \
                     single-list format for one account; its shares are NOT applied, because \
                     the file does not say which document they were for. Re-issue them."
                );
                let _ = std::fs::rename(&path, path.with_extension("v1"));
            }
            return StoredAcls::default();
        };
        let documents = documents
            .iter()
            .filter(|(key, _)| !key.trim().is_empty())
            .map(|(key, value)| (key.clone(), parse_one_acl(value)))
            .collect();
        StoredAcls { documents }
    }

    /// Whether anything is stored for `user_id`.
    pub fn has_document(&self, user_id: &str) -> bool {
        self.tenant_dir(user_id)
            .is_some_and(|dir| dir.join(DOCUMENT_FILE).is_file())
    }
}

/// Read one document's ACL out of its stored JSON.
///
/// Every field only ever GRANTS access, so the fail-closed reading of anything
/// unreadable is empty — and the metadata is filtered against the membership,
/// because a level for an account the list no longer names is a share that does
/// not exist.
fn parse_one_acl(parsed: &serde_json::Value) -> TenantAcl {
    let shared_with: BTreeSet<String> = parsed
        .get("sharedWith")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str())
                .filter(|entry| !entry.trim().is_empty())
                .take(MAX_SHARED_ACCOUNTS)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let grants = parsed
        .get("grants")
        .and_then(|value| value.as_object())
        .map(|entries| {
            entries
                .iter()
                .filter(|(account, _)| shared_with.contains(account.as_str()))
                .map(|(account, value)| {
                    (
                        account.clone(),
                        super::tenant::TenantGrant::from_json(value),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let link_access = parsed
        .get("linkAccess")
        .and_then(|value| value.get("level"))
        .and_then(|level| level.as_str())
        .and_then(|level| op_editor_core::ShareLevel::from_wire(level).ok());
    TenantAcl {
        shared_with,
        grants,
        link_access,
    }
}

/// One document's ACL as it goes on disk.
fn acl_json(acl: &TenantAcl) -> serde_json::Value {
    let bounded: Vec<&String> = acl.shared_with.iter().take(MAX_SHARED_ACCOUNTS).collect();
    // The metadata is written only for accounts that are ON the bounded list: a
    // level for somebody the list had to drop would be metadata about a share
    // that does not exist, and it would outlive the revoke that removed them.
    let grants: std::collections::BTreeMap<&String, serde_json::Value> = acl
        .grants
        .iter()
        .filter(|(account, _)| acl.shared_with.contains(*account))
        .map(|(account, grant)| (account, grant.to_json()))
        .collect();
    let mut body = serde_json::json!({
        "sharedWith": bounded,
        "grants": grants,
    });
    if let Some(level) = acl.link_access {
        body["linkAccess"] = serde_json::json!({ "level": level.wire() });
    }
    body
}

/// The directory name for `user_id`: the first 16 hex characters of its
/// SHA-256. See the module docs for why this is not the id itself.
pub fn dir_name(user_id: &str) -> String {
    let digest = Sha256::digest(user_id.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Write `bytes` to `path` via a same-directory temp file plus rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), TenantStoreError> {
    // Unique per writer: a fixed `.tmp` is shared by every concurrent write to
    // the same tenant, so two of them interleave into one file and the rename
    // publishes a mixture of both.
    let temp = path.with_extension(format!("tmp.{}.{}", std::process::id(), next_temp_id()));
    std::fs::write(&temp, bytes).map_err(|error| TenantStoreError::Io(error.to_string()))?;
    std::fs::rename(&temp, path).map_err(|error| {
        // Leaving the temp behind would accumulate one file per failed write.
        let _ = std::fs::remove_file(&temp);
        TenantStoreError::Io(error.to_string())
    })
}

/// A per-process counter making temp file names unique.
fn next_temp_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Move a file that would not parse out of the way, preserving it.
///
/// Best effort: if the rename fails there is nothing useful left to do, and
/// the caller is already returning a starter document. A pre-existing
/// `.corrupt` from an earlier failure is kept — the timestamp suffix means a
/// second bad file does not overwrite the first.
fn quarantine(path: &Path) {
    let stamp = crate::web_canvas_server::tenant::now_unix();
    let target = path.with_extension(format!("{CORRUPT_SUFFIX}.{stamp}"));
    if std::fs::rename(path, &target).is_err() {
        return;
    }
    eprintln!(
        "openpencil --serve-web --online: kept an unreadable tenant file at {}",
        target.display()
    );
}

#[cfg(test)]
#[path = "tenant_store_tests.rs"]
mod tests;
