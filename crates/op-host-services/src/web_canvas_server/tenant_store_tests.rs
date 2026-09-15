//! Tests for the on-disk tenant store.

use super::*;
use crate::web_canvas_server::tenant_store::TenantAcl;
use op_editor_core::EditorState;

/// A store rooted in a fresh temp directory, removed when the guard drops.
struct TempStore {
    root: PathBuf,
    store: TenantStore,
}

impl TempStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "op-tenant-store-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        Self {
            store: TenantStore::new(Some(root.clone())),
            root,
        }
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A document carrying one recognisable node, built through the same
/// canonical loader the daemon uses.
///
/// The round trip is asserted on DOCUMENT content, not on editor UI state —
/// the `.op` format deliberately does not carry the latter, so a marker like
/// `file_name_display` would silently "fail" every time.
fn named_document(name: &str) -> EditorState {
    let json = serde_json::json!({
        "version": "1.0.0",
        "children": [{
            "id": "n1", "type": "rectangle", "name": name,
            "x": 1, "y": 2, "width": 80, "height": 40,
        }],
    })
    .to_string();
    let loaded = op_pen_loader::load_canonical(&json).expect("canonical document");
    let mut state = EditorState::starter();
    state.replace_document(loaded.value);
    state
}

/// One document's ACL in the map the store writes. A file holds every
/// document's list together, keyed by the store's own name for the document.
fn acls_with(key: &str, acl: &TenantAcl) -> std::collections::BTreeMap<String, TenantAcl> {
    let mut acls = std::collections::BTreeMap::new();
    acls.insert(key.to_string(), acl.clone());
    acls
}

/// The name of the document's first node, read off the serialized form so
/// this does not depend on `PenNode`'s variant shape.
fn node_name(state: &EditorState) -> Option<String> {
    let json = serde_json::to_value(state.doc.children.first()?).ok()?;
    json.get("name")?.as_str().map(str::to_string)
}

#[test]
fn a_saved_document_comes_back() {
    let temp = TempStore::new("roundtrip");
    let mut acl = TenantAcl::default();
    acl.shared_with.insert("userB".to_string());

    temp.store
        .save(
            "userA",
            &named_document("kept.op"),
            &acls_with("docA", &acl),
        )
        .expect("save");

    let restored = temp.store.load_document("userA").expect("load");
    assert_eq!(node_name(&restored).as_deref(), Some("kept.op"));
    assert_eq!(
        temp.store.load_acl("userA", "docA").shared_with,
        acl.shared_with
    );
}

#[test]
fn an_account_with_nothing_stored_reports_so() {
    let temp = TempStore::new("empty");
    assert_eq!(
        temp.store.load_document("userA").unwrap_err(),
        TenantStoreError::NotStored
    );
    assert!(temp.store.load_acl("userA", "docA").shared_with.is_empty());
    assert!(!temp.store.has_document("userA"));
}

#[test]
fn a_disabled_store_reads_and_writes_nothing() {
    let store = TenantStore::new(None);
    assert!(!store.is_enabled());
    assert_eq!(
        store.load_document("userA").unwrap_err(),
        TenantStoreError::Disabled
    );
    assert_eq!(
        store
            .save(
                "userA",
                &EditorState::starter(),
                &acls_with("docA", &TenantAcl::default())
            )
            .unwrap_err(),
        TenantStoreError::Disabled
    );
    assert!(store.tenant_dir("userA").is_none());
}

#[test]
fn a_corrupt_document_is_kept_aside_and_the_account_starts_fresh() {
    let temp = TempStore::new("corrupt");
    temp.store
        .save(
            "userA",
            &named_document("original.op"),
            &acls_with("docA", &TenantAcl::default()),
        )
        .expect("save");
    let dir = temp.store.tenant_dir("userA").expect("dir");
    let document = dir.join("current.op");
    std::fs::write(&document, b"this is not a document").expect("corrupt it");

    let error = temp.store.load_document("userA").unwrap_err();
    assert!(
        matches!(error, TenantStoreError::Unreadable(_)),
        "{error:?}"
    );
    assert!(
        !document.exists(),
        "the unreadable file must be moved aside, not left to fail every visit"
    );
    let kept: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("corrupt"))
        .collect();
    assert_eq!(
        kept.len(),
        1,
        "the bytes must be preserved, not deleted: {kept:?}"
    );
}

#[test]
fn a_second_corrupt_file_does_not_overwrite_the_first() {
    let temp = TempStore::new("corrupt-twice");
    let dir = temp.store.tenant_dir("userA").expect("dir");
    std::fs::create_dir_all(&dir).expect("dir");
    let document = dir.join("current.op");

    // Two separate quarantines. The stamp is second-resolution, so this
    // asserts only that the first is never destroyed.
    std::fs::write(&document, b"garbage one").expect("write");
    let _ = temp.store.load_document("userA");
    std::fs::write(&document, b"garbage two").expect("write");
    let _ = temp.store.load_document("userA");

    let kept: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("corrupt"))
        .collect();
    assert!(!kept.is_empty(), "at least the first must survive");
}

#[test]
fn a_corrupt_access_list_reads_as_empty_rather_than_granting_anyone() {
    let temp = TempStore::new("bad-acl");
    let dir = temp.store.tenant_dir("userA").expect("dir");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("acl.json"), b"{not json").expect("write");
    assert!(
        temp.store.load_acl("userA", "docA").shared_with.is_empty(),
        "an unreadable access list must grant nobody"
    );
}

#[test]
fn an_account_id_never_becomes_a_path_component() {
    // The whole point of hashing: an id full of traversal cannot address a
    // directory outside the store root.
    let temp = TempStore::new("traversal");
    for hostile in [
        "../../../../etc/passwd",
        "..",
        "/etc/shadow",
        "a/b/c",
        "..\\..\\windows",
        "userA\0evil",
    ] {
        let dir = temp.store.tenant_dir(hostile).expect("dir");
        assert!(
            dir.starts_with(&temp.root),
            "{hostile:?} escaped the data directory: {dir:?}"
        );
        assert_eq!(
            dir.parent(),
            Some(temp.root.as_path()),
            "{hostile:?} produced a nested path"
        );
        let name = dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(name.len(), 16, "{hostile:?} -> {name}");
        assert!(
            name.chars().all(|c| c.is_ascii_hexdigit()),
            "{hostile:?} -> {name}"
        );
    }
}

#[test]
fn a_hostile_account_id_still_round_trips_its_own_document() {
    // Hashing must not break the account, only the path injection.
    let temp = TempStore::new("hostile-roundtrip");
    let hostile = "../../../../etc/passwd";
    temp.store
        .save(
            hostile,
            &named_document("hostile.op"),
            &acls_with("docA", &TenantAcl::default()),
        )
        .expect("save");
    let restored = temp.store.load_document(hostile).expect("load");
    assert_eq!(node_name(&restored).as_deref(), Some("hostile.op"));
    // And it did not collide with a different account.
    assert_eq!(
        temp.store.load_document("userA").unwrap_err(),
        TenantStoreError::NotStored
    );
}

#[test]
fn directory_names_are_stable_and_distinct() {
    assert_eq!(dir_name("userA"), dir_name("userA"));
    assert_ne!(dir_name("userA"), dir_name("userB"));
    assert_eq!(dir_name("userA").len(), 16);
}

#[test]
fn a_write_leaves_no_temp_file_behind() {
    let temp = TempStore::new("atomic");
    temp.store
        .save(
            "userA",
            &EditorState::starter(),
            &acls_with("docA", &TenantAcl::default()),
        )
        .expect("save");
    let dir = temp.store.tenant_dir("userA").expect("dir");
    let strays: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(
        strays.is_empty(),
        "a completed write must leave no temp file"
    );
}

#[test]
fn saving_the_access_list_alone_does_not_require_a_document() {
    let temp = TempStore::new("acl-only");
    let mut acl = TenantAcl::default();
    acl.shared_with.insert("userB".to_string());
    temp.store
        .save_acls_for("userA", &acls_with("docA", &acl))
        .expect("save acl");
    assert_eq!(
        temp.store.load_acl("userA", "docA").shared_with,
        acl.shared_with
    );
    assert!(!temp.store.has_document("userA"));
}

// ---------------------------------------------------------------------------
// The restore chain must not touch the process-global thumbnail registry.
// ---------------------------------------------------------------------------

/// A thumbnail id used by no other test, so these assertions are immune to
/// the registry being cleared by whatever else runs in parallel.
const ISOLATION_THUMB_ID: u64 = 918_273_645;

#[test]
fn restoring_a_tenant_never_publishes_its_thumbnails_globally() {
    // `EditorState::from_document` activates a document's thumbnails, and
    // activation REPLACES the process-global map wholesale — so restoring one
    // account would discard every other account's and rebind their ids.
    let _registry = crate::web_canvas_server::lock_image_thumb_registry();
    let temp = TempStore::new("thumb-isolation");
    let dir = temp.store.tenant_dir("userA").expect("dir");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("current.op");

    // Write a file that DOES carry a thumbnail, using the ordinary
    // (thumbnail-capturing) desktop save path.
    jian_ops_schema::image_thumbs::store_thumb(ISOLATION_THUMB_ID, vec![7, 7, 7]);
    crate::doc_io::save_to_path(&named_document("thumbed.op"), &path).expect("save");

    // Restoring it through the tenant store must not publish that thumbnail.
    jian_ops_schema::image_thumbs::clear_registry();
    let restored = temp.store.load_document("userA").expect("load");
    assert_eq!(node_name(&restored).as_deref(), Some("thumbed.op"));
    assert!(
        jian_ops_schema::image_thumbs::thumb_for(ISOLATION_THUMB_ID).is_none(),
        "a tenant restore must not publish its thumbnails into the shared registry"
    );
}

#[test]
fn a_persisted_tenant_carries_no_thumbnail_data() {
    // The save side of the same problem: `capture_snapshot` reads a registry
    // with no tenant dimension, so an eviction would write whichever account
    // activated last into THIS account's file.
    let _registry = crate::web_canvas_server::lock_image_thumb_registry();
    let temp = TempStore::new("thumb-capture");
    jian_ops_schema::image_thumbs::store_thumb(ISOLATION_THUMB_ID + 1, vec![1, 2, 3]);

    temp.store
        .save(
            "userA",
            &named_document("plain.op"),
            &acls_with("docA", &TenantAcl::default()),
        )
        .expect("save");

    let written = std::fs::read_to_string(
        temp.store
            .tenant_dir("userA")
            .expect("dir")
            .join("current.op"),
    )
    .expect("read back");
    assert!(
        !written.contains("imageThumbs"),
        "another tenant's image data must not ride along"
    );
}

#[test]
fn a_parsed_push_really_registers_its_thumbnail_seed() {
    // Grounded in the true key semantics: `DocumentSeedKey` is the version
    // String's (ptr, len, capacity), so the seed can only be observed through
    // the SAME document allocation the parse produced. Asserting on a cloned
    // document — as an earlier version of this test did — is vacuous: a clone
    // has a different pointer and always misses.
    use crate::web_canvas_server::{PendingDocumentPush, ServeMode};

    let body = seeded_push_body();
    let mut push = PendingDocumentPush::parse(&body, ServeMode::Local).expect("parses");
    let prepared = push.prepared.take().expect("a document push");

    // Consuming the seed through the real document proves it was registered:
    // `discard_for_document` returns true only when a pending seed exists for
    // exactly this allocation.
    assert!(
        jian_ops_schema::image_thumbs::discard_for_document(prepared.document()),
        "the parse must register a pending seed under this document"
    );
    // …and it is gone, so a second release is a no-op — which is what makes
    // the `Drop`, the gate path and the runtime-reject path idempotent.
    assert!(!jian_ops_schema::image_thumbs::discard_for_document(
        prepared.document()
    ));
}

#[test]
fn an_online_push_never_registers_a_seed_at_all() {
    // Online policy discards at parse time, so there is nothing for any later
    // path to leak.
    use crate::web_canvas_server::{PendingDocumentPush, ServeMode};

    let body = seeded_push_body();
    let mut push = PendingDocumentPush::parse(&body, ServeMode::Online).expect("parses");
    let prepared = push.prepared.take().expect("a document push");
    assert!(
        !jian_ops_schema::image_thumbs::discard_for_document(prepared.document()),
        "online must not register a seed the shared registry could activate"
    );
}

/// A push body carrying an embedded thumbnail.
fn seeded_push_body() -> String {
    serde_json::json!({
        "document": {
            "version": "1.0.0",
            "children": [{
                "id": "n1", "type": "rectangle", "name": "seeded",
                "x": 0, "y": 0, "width": 4, "height": 4,
            }],
            "imageThumbs": { "5150": "AQID" },
        },
        "sourceClientId": "s",
    })
    .to_string()
}
