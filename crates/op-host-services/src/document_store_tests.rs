//! The store's rules: keys, file names, and what each operation means.
//!
//! The database's own behaviour (schema, migrations, the import) is pinned in
//! `document_db_tests`; these drive the `document_store` API the routes call.

use super::*;
use crate::document_test_dir::TempDir;

/// A key this store could have issued, so the store answers about the document
/// rather than about the key.
const KEY: &str = "aaaaaaaa00000001";

fn create(db: &DocumentDb, tag: &str, name: &str, body: &[u8]) -> DocumentEntry {
    let body = body.to_vec();
    create_with(db, Some(name), move |path| {
        std::fs::write(path, &body)
            .map_err(|error| DocumentStoreError::Io(format!("write {}: {error}", path.display())))
    })
    .unwrap_or_else(|error| panic!("create {tag}: {error}"))
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
    for key in [
        "",
        "..",
        "../etc/passwd",
        "abc/def",
        ".hidden..",
        "SHORT",
        "i-l-o",
    ] {
        assert!(!key_is_valid(key), "{key} must not be a key");
        assert_eq!(
            path_for(Path::new("/tmp"), key),
            Err(DocumentStoreError::InvalidKey),
            "{key}"
        );
        assert_eq!(
            thumb_path(Path::new("/tmp"), key),
            Err(DocumentStoreError::InvalidKey),
            "{key}"
        );
    }
}

#[test]
fn create_list_and_delete_round_trip() {
    let dir = TempDir::new("round-trip");
    let db = dir.open();
    let entry = create(&db, "first", "Список токенов", b"first");
    assert_eq!(entry.name, "Список токенов");
    assert_eq!(entry.size, 5);
    assert!(!entry.has_thumbnail);

    let listed = list(&db).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].key, entry.key);

    let path = path_for(db.dir(), &entry.key).expect("path");
    assert_eq!(std::fs::read(&path).expect("read"), b"first");

    delete(&db, &entry.key).expect("delete");
    assert!(list(&db).expect("list").is_empty());
    assert!(!path.exists(), "the file goes with the row");
    assert_eq!(
        delete(&db, &entry.key),
        Err(DocumentStoreError::NotFound),
        "a second delete has nothing to remove"
    );
}

#[test]
fn a_save_refreshes_the_entry_and_keeps_one_row() {
    let dir = TempDir::new("save");
    let db = dir.open();
    let entry = create(&db, "draft", "Draft", b"one");

    // What a save does: the document writer replaces the file, then the store's
    // row is refreshed to describe it.
    let path = path_for(db.dir(), &entry.key).expect("path");
    std::fs::write(&path, b"two-longer").expect("save");
    let saved = touch(&db, &entry.key).expect("touch");

    assert_eq!(saved.key, entry.key);
    assert_eq!(saved.size, 10);
    assert_eq!(saved.name, "Draft", "the name is not what a save changes");
    let listed = list(&db).expect("list");
    assert_eq!(listed.len(), 1, "a save must not add a second row");
    assert_eq!(std::fs::read(&path).expect("read"), b"two-longer");
}

#[test]
fn touching_a_document_without_a_file_is_not_found() {
    let dir = TempDir::new("touch-missing");
    let db = dir.open();
    assert_eq!(touch(&db, KEY), Err(DocumentStoreError::NotFound));
}

#[test]
fn a_document_that_arrived_without_a_row_is_adopted_by_a_save() {
    // A file restored from a backup, or copied in by hand, is a document: the
    // first save gives it the row the index never had.
    let dir = TempDir::new("adopt");
    let db = dir.open();
    let path = path_for(dir.path(), KEY).expect("path");
    std::fs::write(&path, b"restored").expect("write");
    let entry = touch(&db, KEY).expect("touch");
    assert_eq!(entry.key, KEY);
    assert_eq!(entry.name, DEFAULT_NAME);
    assert_eq!(entry.size, 8);
    assert_eq!(list(&db).expect("list").len(), 1);
}

#[test]
fn renaming_keeps_the_key_so_links_survive() {
    let dir = TempDir::new("rename");
    let db = dir.open();
    let entry = create(&db, "before", "Before", b"body");
    let renamed = rename(&db, &entry.key, "After").expect("rename");
    assert_eq!(renamed.key, entry.key);
    assert_eq!(renamed.name, "After");
    let path = path_for(db.dir(), &entry.key).expect("path");
    assert_eq!(std::fs::read(&path).expect("read"), b"body");
    assert_eq!(
        rename(&db, KEY, "Nothing"),
        Err(DocumentStoreError::NotFound),
        "renaming a document that is not stored is not a silent create"
    );
}

#[test]
fn the_list_is_most_recent_first() {
    let dir = TempDir::new("order");
    let db = dir.open();
    let first = create(&db, "first", "First", b"1");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = create(&db, "second", "Second", b"2");
    let listed = list(&db).expect("list");
    assert_eq!(listed[0].key, second.key, "the newer document leads");
    assert_eq!(listed[1].key, first.key);
}

#[test]
fn the_last_document_comes_back_while_its_file_is_there() {
    let dir = TempDir::new("last");
    let db = dir.open();
    let entry = create(&db, "doc", "Work", b"body");
    assert_eq!(last_document(&db), None, "nothing is remembered yet");

    remember_last(&db, &entry.key).expect("remember");
    let restored = last_document(&db).expect("restored");
    assert_eq!(restored.key, entry.key);
    assert_eq!(restored.name, "Work", "the list's own row comes back");

    assert_eq!(
        remember_last(&db, KEY),
        Err(DocumentStoreError::NotFound),
        "a key no document carries is not a thing to reopen"
    );
    assert_eq!(
        remember_last(&db, "not-a-valid-key"),
        Err(DocumentStoreError::InvalidKey)
    );

    // The record is only as good as the file it names.
    std::fs::remove_file(path_for(db.dir(), &entry.key).expect("path")).expect("remove");
    assert_eq!(last_document(&db), None);
}

#[test]
fn deleting_the_open_document_leaves_nothing_to_reopen() {
    let dir = TempDir::new("last-deleted");
    let db = dir.open();
    let entry = create(&db, "doc", "Work", b"body");
    remember_last(&db, &entry.key).expect("remember");
    delete(&db, &entry.key).expect("delete");
    assert_eq!(last_document(&db), None);
}

#[test]
fn the_thumbnail_flag_follows_the_document() {
    let dir = TempDir::new("thumb");
    let db = dir.open();
    let entry = create(&db, "doc", "Work", b"body");

    note_thumbnail(&db, &entry.key, true).expect("note");
    assert!(list(&db).expect("list")[0].has_thumbnail);
    // Writing the same value again is not an error and not a second write.
    note_thumbnail(&db, &entry.key, true).expect("note again");
    assert!(list(&db).expect("list")[0].has_thumbnail);

    note_thumbnail(&db, &entry.key, false).expect("clear");
    assert!(!list(&db).expect("list")[0].has_thumbnail);
    assert_eq!(
        note_thumbnail(&db, KEY, true),
        Err(DocumentStoreError::NotFound)
    );
}

#[test]
fn the_recovery_slot_is_still_a_file_beside_the_documents() {
    // The draft deliberately did not move into the database: it is one blob of
    // unsaved work, not a record anyone lists or queries.
    let dir = TempDir::new("recovery");
    let db = dir.open();
    assert_eq!(recovery_info(dir.path()), None);
    std::fs::write(recovery_path(dir.path()), b"draft").expect("write draft");
    let info = recovery_info(dir.path()).expect("info");
    assert_eq!(info.size, 5);
    assert!(info.saved_at > 0);
    clear_recovery(dir.path()).expect("clear");
    assert_eq!(recovery_info(dir.path()), None);
    clear_recovery(dir.path()).expect("clearing twice is not an error");
    // And the store itself is unaffected by any of it.
    assert!(list(&db).expect("list").is_empty());
}

#[test]
fn the_directory_can_be_pointed_somewhere_else() {
    // The store keeps no state derived from the environment — the directory is
    // an argument to `DocumentDb::open`, and every test above passes its own —
    // so this only exercises the resolver.
    let previous = std::env::var_os(DOCUMENTS_DIR_ENV);
    // SAFETY: this test owns the variable for its duration; no store is opened
    // from it in this process.
    unsafe { std::env::set_var(DOCUMENTS_DIR_ENV, "/tmp/norka-docs-test") };
    assert_eq!(documents_dir(), PathBuf::from("/tmp/norka-docs-test"));
    match previous {
        Some(value) => unsafe { std::env::set_var(DOCUMENTS_DIR_ENV, value) },
        None => unsafe { std::env::remove_var(DOCUMENTS_DIR_ENV) },
    }
}
