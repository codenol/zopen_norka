//! The analytics asset store: where a document lives, how it is addressed, and
//! what it refuses.

use super::*;
use crate::document_store;
use crate::document_test_dir::TempDir;

/// A store in a fresh directory.
fn store(tag: &str) -> (TempDir, DocumentDb) {
    let dir = TempDir::new(tag);
    let db = dir.open();
    (dir, db)
}

#[test]
fn a_loaded_document_lands_as_a_file_and_a_row() {
    let (dir, db) = store("analytics-create");
    let asset = create(
        &db,
        "Checkout analytics",
        Some("userA"),
        "# Checkout\n\nWhy.\n",
    )
    .expect("create");

    assert!(
        key_is_valid(&asset.key),
        "the address is addressable: {}",
        asset.key
    );
    assert_eq!(asset.owner_id.as_deref(), Some("userA"));
    assert_eq!(asset.name, "Checkout analytics");
    assert_eq!(asset.size, "# Checkout\n\nWhy.\n".len() as u64);

    // The markdown is a real file, in its own directory, readable by anything
    // that reads files — which is the point of it being markdown.
    let path = dir.join(&format!("analytics/{}.md", asset.key));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the asset"),
        "# Checkout\n\nWhy.\n"
    );

    let loaded = read(&db, &asset.key).expect("read");
    assert_eq!(loaded.markdown, "# Checkout\n\nWhy.\n");
    assert_eq!(loaded.digest, analytics_fingerprint("# Checkout\n\nWhy.\n"));
    assert_eq!(loaded.asset, asset);
}

#[test]
fn a_reloaded_document_keeps_its_address_and_changes_its_digest() {
    let (_dir, db) = store("analytics-write");
    let asset = create(&db, "Analytics", None, "first\n").expect("create");
    let before = read(&db, &asset.key).expect("read").digest;

    let after = write(&db, &asset.key, "second\n").expect("write");
    assert_eq!(after.asset.key, asset.key, "the address did not move");
    assert_eq!(after.asset.created_at, asset.created_at);
    assert_ne!(after.digest, before, "the digest followed the bytes");
    assert_eq!(after.asset.size, "second\n".len() as u64);
    assert_eq!(
        std::fs::read_to_string(assets_dir(_dir.path()).join(format!("{}.md", asset.key)))
            .expect("read"),
        "second\n"
    );
}

#[test]
fn the_digest_is_the_one_the_store_can_resolve_now() {
    let (_dir, db) = store("analytics-digest");
    let asset = create(&db, "Analytics", None, "as loaded\n").expect("create");
    assert_eq!(
        digest(&db, &asset.key).expect("digest"),
        Some(analytics_fingerprint("as loaded\n"))
    );

    // A hand edit outside the app — the case the file format exists for — is
    // seen, because the digest is computed from the bytes and never stored.
    let path = assets_dir(_dir.path()).join(format!("{}.md", asset.key));
    std::fs::write(&path, "edited by hand\n").expect("hand edit");
    assert_eq!(
        digest(&db, &asset.key).expect("digest"),
        Some(analytics_fingerprint("edited by hand\n"))
    );
    // ...and a CRLF save is not an edit.
    std::fs::write(&path, "edited by hand\r\n").expect("crlf save");
    assert_eq!(
        digest(&db, &asset.key).expect("digest"),
        Some(analytics_fingerprint("edited by hand\n"))
    );
}

#[test]
fn an_asset_that_is_not_there_has_no_digest() {
    let (_dir, db) = store("analytics-missing-digest");
    assert_eq!(
        digest(&db, &document_store::new_key()).expect("digest"),
        None
    );
}

#[test]
fn a_row_without_its_file_is_a_broken_store_and_not_an_absent_asset() {
    // Reported as absent, this would make a section announce that its analytics
    // had been deleted when nobody deleted it.
    let (dir, db) = store("analytics-broken");
    let asset = create(&db, "Analytics", None, "text\n").expect("create");
    std::fs::remove_file(assets_dir(dir.path()).join(format!("{}.md", asset.key)))
        .expect("remove the file");

    let error = read(&db, &asset.key).expect_err("broken");
    assert!(
        matches!(error, SectionStoreError::MissingFile { .. }),
        "{error:?}"
    );
    assert!(error
        .to_string()
        .contains("this record accounts for is gone"));
    assert_eq!(
        digest(&db, &asset.key).expect_err("broken"),
        error,
        "the digest path agrees: the asset is not absent, it is damaged"
    );
}

#[test]
fn a_key_that_is_not_a_key_never_reaches_the_filesystem() {
    let (_dir, db) = store("analytics-key");
    for key in ["../../etc/passwd", "short", "has/slash", ""] {
        assert_eq!(
            read(&db, key).expect_err("refused"),
            SectionStoreError::InvalidKey,
            "{key}"
        );
        assert_eq!(
            find(&db, key).expect_err("refused"),
            SectionStoreError::InvalidKey,
            "{key}"
        );
        assert_eq!(
            asset_path(db.dir(), key).expect_err("refused"),
            SectionStoreError::InvalidKey,
            "{key}"
        );
    }
}

#[test]
fn writing_to_an_asset_nobody_loaded_is_not_found() {
    let (_dir, db) = store("analytics-write-missing");
    assert_eq!(
        write(&db, &document_store::new_key(), "text\n").expect_err("refused"),
        SectionStoreError::NotFound
    );
}

#[test]
fn an_asset_is_listed_for_its_owner_and_for_nobody_else() {
    let (_dir, db) = store("analytics-list");
    let mine = create(&db, "Mine", Some("userA"), "a\n").expect("create");
    let theirs = create(&db, "Theirs", Some("userB"), "b\n").expect("create");
    let local = create(&db, "Local", None, "c\n").expect("create");

    let listed = list(&db, Some("userA")).expect("list");
    assert_eq!(
        listed
            .iter()
            .map(|asset| asset.key.as_str())
            .collect::<Vec<_>>(),
        vec![mine.key.as_str()]
    );
    assert_eq!(list(&db, Some("userB")).expect("list")[0].key, theirs.key);
    // The unattributed ones belong to the operator of this machine, and to no
    // account: the fail-closed direction, and the same rule the file list
    // follows.
    assert_eq!(list(&db, None).expect("list")[0].key, local.key);
    assert!(list(&db, Some("userC")).expect("list").is_empty());
}

#[test]
fn renaming_an_asset_leaves_its_address_alone() {
    let (_dir, db) = store("analytics-rename");
    let asset = create(&db, "Old name", Some("userA"), "text\n").expect("create");
    let renamed = rename(&db, &asset.key, "New name").expect("rename");
    assert_eq!(renamed.key, asset.key);
    assert_eq!(renamed.name, "New name");
    assert_eq!(read(&db, &asset.key).expect("read").asset.name, "New name");
    assert_eq!(
        rename(&db, &document_store::new_key(), "x").expect_err("refused"),
        SectionStoreError::NotFound
    );
}

#[test]
fn an_asset_can_be_deleted_and_then_it_is_gone() {
    let (dir, db) = store("analytics-delete");
    let asset = create(&db, "Analytics", None, "text\n").expect("create");
    let path = assets_dir(dir.path()).join(format!("{}.md", asset.key));

    delete(&db, &asset.key).expect("delete");
    assert!(!path.exists(), "the file went with the row");
    assert!(find(&db, &asset.key).expect("find").is_none());
    assert_eq!(
        digest(&db, &asset.key).expect("digest"),
        None,
        "and a section that referenced it now resolves nothing"
    );
    assert_eq!(
        delete(&db, &asset.key).expect_err("refused"),
        SectionStoreError::NotFound
    );
}

#[test]
fn a_name_this_store_will_not_keep_is_refused_by_its_limit() {
    let (_dir, db) = store("analytics-name");
    let long = "я".repeat(MAX_ANALYTICS_NAME_CHARS + 1);
    let error = create(&db, &long, None, "text\n").expect_err("refused");
    assert_eq!(
        error,
        SectionStoreError::NameTooLong {
            chars: MAX_ANALYTICS_NAME_CHARS + 1,
            max: MAX_ANALYTICS_NAME_CHARS,
        }
    );
    assert!(error.to_string().contains("over the"));
    // Characters, not bytes: a name of the limit's worth of Cyrillic fits.
    let exact = "я".repeat(MAX_ANALYTICS_NAME_CHARS);
    assert!(create(&db, &exact, None, "text\n").is_ok());
}

#[test]
fn a_document_over_the_limit_is_refused_before_it_is_written() {
    let (dir, db) = store("analytics-size");
    let huge = "x".repeat(MAX_ANALYTICS_BYTES + 1);
    let error = create(&db, "Big", None, &huge).expect_err("refused");
    assert_eq!(
        error,
        SectionStoreError::TooLarge {
            bytes: MAX_ANALYTICS_BYTES + 1,
            max: MAX_ANALYTICS_BYTES,
        }
    );
    // Nothing was left behind by the refusal.
    assert!(
        !assets_dir(dir.path()).exists()
            || std::fs::read_dir(assets_dir(dir.path()))
                .expect("read dir")
                .next()
                .is_none()
    );

    let asset = create(&db, "Small", None, "ok\n").expect("create");
    assert_eq!(
        write(&db, &asset.key, &huge).expect_err("refused"),
        error,
        "and replacing one is refused the same way"
    );
    assert_eq!(read(&db, &asset.key).expect("read").markdown, "ok\n");
}

#[test]
fn two_assets_never_share_an_address() {
    let (_dir, db) = store("analytics-unique");
    let first = create(&db, "One", None, "a\n").expect("create");
    let second = create(&db, "Two", None, "b\n").expect("create");
    assert_ne!(first.key, second.key);
    assert_eq!(list(&db, None).expect("list").len(), 2);
}

#[test]
fn every_error_says_what_went_wrong() {
    let cases = vec![
        (SectionStoreError::InvalidKey, "invalid key"),
        (SectionStoreError::NotFound, "not found"),
        (SectionStoreError::EmptyNodeId, "empty node id"),
        (
            SectionStoreError::NameTooLong { chars: 5, max: 3 },
            "5 characters",
        ),
        (SectionStoreError::TooLarge { bytes: 9, max: 4 }, "9 bytes"),
        (
            SectionStoreError::MissingFile {
                path: "/tmp/x.md".to_string(),
            },
            "/tmp/x.md",
        ),
        (SectionStoreError::Database("disk".to_string()), "disk"),
        (SectionStoreError::Io("nope".to_string()), "nope"),
    ];
    for (error, expected) in cases {
        assert!(error.to_string().contains(expected), "{error}");
    }
    assert!(
        SectionStoreError::from(document_store::DocumentStoreError::NotFound)
            .to_string()
            .contains("not found")
    );
}
