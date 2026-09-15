//! A section's properties: one row per section, and what a row refuses.

use super::*;
use crate::document_store::{self, DocumentEntry, DocumentStoreError};
use crate::document_test_dir::TempDir;
use op_editor_core::section::{AnalyticsLink, SectionDigest, SectionSummary, UxFlow};

/// A store with one stored document, and that document's entry.
fn store(tag: &str) -> (TempDir, DocumentDb, DocumentEntry) {
    let dir = TempDir::new(tag);
    let db = dir.open();
    let entry = document_store::create_with(&db, Some("Doc"), Some("userA"), |path| {
        std::fs::write(path, "{}").map_err(|error| DocumentStoreError::Io(error.to_string()))
    })
    .expect("seed a document");
    (dir, db, entry)
}

/// Properties with something in every part of them.
fn properties() -> SectionProperties {
    let mut properties = SectionProperties::empty();
    properties.analytics.push(AnalyticsLink::new(
        "k0123456789",
        "Checkout analytics",
        SectionDigest::of_text("analytics"),
        SectionDigest::of_text("mockups"),
        1_700_000_000,
        Some("userA"),
    ));
    properties.summary = SectionSummary {
        what_it_is: "Оформление заказа".to_string(),
        where_to_look: "Начинать с корзины".to_string(),
        use_cases: "Покупка одной позиции".to_string(),
        what_to_check: "Пустая корзина".to_string(),
    };
    properties
        .flows
        .push(
            UxFlow::new("f1", "Checkout").with_step(op_editor_core::section::FlowStep::new(
                op_editor_core::section::FlowStepId::new("A").expect("id"),
                "Start",
                op_editor_core::section::FlowStepKind::Start,
            )),
        );
    properties
}

fn node(id: &str) -> NodeId {
    NodeId::new(id)
}

#[test]
fn a_section_nobody_wrote_about_has_no_properties_and_that_is_not_an_error() {
    let (_dir, db, entry) = store("section-empty");
    assert_eq!(load(&db, &entry.key, &node("n1")).expect("load"), None);
}

#[test]
fn properties_survive_a_round_trip_through_the_row() {
    let (_dir, db, entry) = store("section-round-trip");
    let written = properties();
    save(&db, &entry.key, &node("n1"), &written).expect("save");
    assert_eq!(
        load(&db, &entry.key, &node("n1")).expect("load"),
        Some(written)
    );
}

#[test]
fn saving_again_replaces_what_was_there() {
    let (_dir, db, entry) = store("section-replace");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    let mut edited = properties();
    edited.summary.what_to_check = "Другое".to_string();
    save(&db, &entry.key, &node("n1"), &edited).expect("save again");
    assert_eq!(
        load(&db, &entry.key, &node("n1"))
            .expect("load")
            .expect("properties")
            .summary
            .what_to_check,
        "Другое"
    );
    assert_eq!(list(&db, &entry.key).expect("list").len(), 1);
}

#[test]
fn a_sections_properties_are_found_by_its_own_address() {
    let (_dir, db, entry) = store("section-address");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    save(&db, &entry.key, &node("n2"), &SectionProperties::empty()).expect("save");
    // Another document's node id is another section.
    let other = document_store::create_with(&db, Some("Other"), Some("userA"), |path| {
        std::fs::write(path, "{}").map_err(|error| DocumentStoreError::Io(error.to_string()))
    })
    .expect("seed");

    assert!(load(&db, &entry.key, &node("n1")).expect("load").is_some());
    assert_eq!(load(&db, &other.key, &node("n1")).expect("load"), None);
    assert_eq!(list(&db, &other.key).expect("list"), Vec::new());
    assert_eq!(list(&db, &entry.key).expect("list").len(), 2);
}

#[test]
fn properties_are_written_against_a_stored_document_only() {
    // A section's properties hang off a document this daemon has a row for —
    // the same rule comments follow. The check is inside the statement, so an
    // empty key cannot slip in between a check and a write.
    let (_dir, db, _entry) = store("section-unstored");
    let error = save(&db, "0000000000", &node("n1"), &properties()).expect_err("refused");
    assert_eq!(error, SectionStoreError::NotFound);
}

#[test]
fn an_empty_node_id_names_nothing_and_is_refused() {
    let (_dir, db, entry) = store("section-empty-id");
    assert_eq!(
        save(&db, &entry.key, &NodeId::NONE, &properties()).expect_err("refused"),
        SectionStoreError::EmptyNodeId
    );
}

#[test]
fn deleting_a_sections_properties_is_reported_honestly() {
    let (_dir, db, entry) = store("section-delete");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    assert!(delete(&db, &entry.key, &node("n1")).expect("delete"));
    assert!(!delete(&db, &entry.key, &node("n1")).expect("delete again"));
    assert_eq!(load(&db, &entry.key, &node("n1")).expect("load"), None);
}

#[test]
fn deleting_the_document_takes_its_sections_properties_with_it() {
    // The row is not a record of a design that outlives the file: whoever takes
    // the key next must not inherit somebody else's analytics.
    let (_dir, db, entry) = store("section-cascade");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    document_store::delete(&db, &entry.key).expect("delete the document");
    assert_eq!(list(&db, &entry.key).expect("list"), Vec::new());
    assert_eq!(load(&db, &entry.key, &node("n1")).expect("load"), None);
}

#[test]
fn a_payload_this_build_cannot_read_is_refused_and_not_returned_empty() {
    // The one failure this store must not have: authored work disappearing
    // without anybody deciding to delete it.
    let (_dir, db, entry) = store("section-unreadable");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    db.conn()
        .execute(
            "UPDATE section_properties SET properties = ?3 WHERE document_key = ?1 AND node_id = ?2",
            params![entry.key, "n1", "{\"format\":99,\"properties\":{}}"],
        )
        .expect("hand write a newer payload");

    let error = load(&db, &entry.key, &node("n1")).expect_err("refused");
    assert!(
        matches!(
            error,
            SectionStoreError::Properties(
                op_editor_core::section::SectionFormatError::UnsupportedFormat { format: 99 }
            )
        ),
        "{error:?}"
    );
    assert!(list(&db, &entry.key).is_err(), "and the list refuses too");

    db.conn()
        .execute(
            "UPDATE section_properties SET properties = ?3 WHERE document_key = ?1 AND node_id = ?2",
            params![entry.key, "n1", "not json at all"],
        )
        .expect("hand write rubbish");
    assert!(matches!(
        load(&db, &entry.key, &node("n1")).expect_err("refused"),
        SectionStoreError::Properties(
            op_editor_core::section::SectionFormatError::Malformed { .. }
        )
    ));
}

#[test]
fn a_row_whose_node_id_is_empty_is_skipped_rather_than_fatal() {
    // Only something other than this module could have written it, and one bad
    // row must not take a document's whole list down.
    let (_dir, db, entry) = store("section-empty-row");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");
    db.conn()
        .execute(
            "INSERT INTO section_properties (document_key, node_id, properties, updated_at)
             VALUES (?1, '', '{\"format\":1,\"properties\":{}}', 0)",
            params![entry.key],
        )
        .expect("hand write a row with no id");

    let listed = list(&db, &entry.key).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].node_id, node("n1"));
}

#[test]
fn the_stored_payload_carries_the_sections_own_analytics_links() {
    // What a row is FOR: a reader of the section gets from it to the analytics
    // it was built from, with the fingerprints that say whether it still
    // matches.
    let (_dir, db, entry) = store("section-links");
    save(&db, &entry.key, &node("n7"), &properties()).expect("save");
    let stored = load(&db, &entry.key, &node("n7"))
        .expect("load")
        .expect("properties");
    let link = stored.link_for("k0123456789").expect("the link");
    assert_eq!(link.name, "Checkout analytics");
    assert_eq!(link.digest, SectionDigest::of_text("analytics"));
    assert_eq!(link.mockups, SectionDigest::of_text("mockups"));
    assert_eq!(link.linked_by.as_deref(), Some("userA"));
}

#[test]
fn the_reverse_lookup_finds_the_document_that_links_an_asset() {
    // The question issue #110 turns on: an asset is reached by its own key, so
    // "which document names this asset" is the only thing that can vouch for a
    // reader who does not own it. Answered per document — the one the request
    // named — so reading an asset costs that document's rows and not the whole
    // store's.
    let (_dir, db, entry) = store("section-reverse-lookup");
    save(&db, &entry.key, &node("n1"), &properties()).expect("save");

    assert!(references_asset(&db, &entry.key, "k0123456789").expect("lookup"));
    assert!(
        !references_asset(&db, &entry.key, "k9999999999").expect("lookup"),
        "a key nobody linked is not vouched for by a document that links another"
    );
    assert!(
        !references_asset(&db, "a-document-that-does-not-exist", "k0123456789").expect("lookup"),
        "the lookup is scoped to the document it is asked about"
    );
}

#[test]
fn a_row_this_build_cannot_read_vouches_for_nothing() {
    // Whether an unreadable row holds the link is exactly what cannot be
    // established, and a vouch nobody can verify is not one — so this is an
    // error the caller answers "no" to, never a silent yes.
    let (_dir, db, entry) = store("section-reverse-lookup-bad-row");
    db.conn()
        .execute(
            "INSERT INTO section_properties (document_key, node_id, properties, updated_at)
             VALUES (?1, 'n1', '{\"format\":999,\"properties\":{}}', 0)",
            params![entry.key],
        )
        .expect("hand write a row from a newer build");

    assert!(references_asset(&db, &entry.key, "k0123456789").is_err());
}
