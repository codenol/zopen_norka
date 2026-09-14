//! The page identity rule, and the fallback that makes it total.

use super::*;
fn doc(json: &str) -> jian_ops_schema::PenDocument {
    jian_ops_schema::load_str(json)
        .expect("fixture parses")
        .value
}

#[test]
fn a_document_with_pages_names_the_one_it_is_showing() {
    let mut state = EditorState::from_document(doc(r#"{"version":"1.0.0","pages":[
            {"id":"p1","name":"Page 1","children":[]},
            {"id":"p2","name":"Cover","children":[]}
        ]}"#));
    assert_eq!(state.active_page_id(), Some("p1"));
    assert_eq!(
        state.active_page_identity(),
        ("p1".to_string(), "Page 1".to_string())
    );
    assert!(state.set_active_page(1));
    assert_eq!(state.active_page_id(), Some("p2"));
    assert_eq!(
        state.active_page_identity(),
        ("p2".to_string(), "Cover".to_string())
    );
}

#[test]
fn an_out_of_range_index_names_the_page_whose_children_are_shown() {
    // The same fallback `active_children` uses, so the identity can never name
    // a page the canvas is not painting.
    let mut state = EditorState::from_document(doc(
        r#"{"version":"1.0.0","pages":[{"id":"p1","name":"Page 1","children":[]}]}"#,
    ));
    state.ui.active_page_index = 9;
    assert_eq!(state.active_page_id(), Some("p1"));
    assert_eq!(state.active_page_identity().0, "p1");
}

#[test]
fn a_single_page_document_gets_the_id_the_scene_has_always_used() {
    // No `pages` array: the id is synthesized, and it has to be the same string
    // the render scene and the MCP page tools use for that shape.
    let state =
        EditorState::from_document(doc(r#"{"version":"1.0.0","name":"Untitled","children":[
            {"type":"rectangle","id":"n1","name":"Card","x":0,"y":0,"width":10,"height":10}
        ]}"#));
    assert_eq!(state.active_page_id(), None);
    assert_eq!(
        state.active_page_identity(),
        ("page-1".to_string(), "Untitled".to_string())
    );
}

#[test]
fn an_empty_document_names_its_page_after_the_starter_node() {
    let state = EditorState::from_document(doc(r#"{"version":"1.0.0","children":[]}"#));
    assert_eq!(
        state.active_page_identity(),
        ("n1".to_string(), "Page 1".to_string())
    );
}

#[test]
fn every_page_can_be_named_by_index_not_only_the_visible_one() {
    // A surface that lists pages has to name each row's page, and it must get
    // the same string the toolbar badge and the comment rail use for that page —
    // a row counting threads under one name while the rail files them under
    // another is the failure this rule exists to prevent.
    let mut state = EditorState::from_document(doc(r#"{"version":"1.0.0","pages":[
            {"id":"p1","name":"Page 1","children":[]},
            {"id":"p2","name":"Cover","children":[]}
        ]}"#));
    assert_eq!(
        state.page_identity_at(0),
        ("p1".to_string(), "Page 1".to_string())
    );
    assert_eq!(
        state.page_identity_at(1),
        ("p2".to_string(), "Cover".to_string())
    );
    // The active page is this rule at the active index, so the two cannot drift.
    state.set_active_page(1);
    assert_eq!(state.page_identity_at(1), state.active_page_identity());
}

#[test]
fn an_out_of_range_row_index_names_the_page_whose_children_are_shown() {
    // The same clamping `active_page_identity` does: an index past the end reads
    // the last page rather than panicking on a panel repaint.
    let state = EditorState::from_document(doc(
        r#"{"version":"1.0.0","pages":[{"id":"p1","name":"Page 1","children":[]}]}"#,
    ));
    assert_eq!(
        state.page_identity_at(9),
        ("p1".to_string(), "Page 1".to_string())
    );
}

#[test]
fn a_single_page_document_names_its_only_row_by_the_synthesized_id() {
    let state =
        EditorState::from_document(doc(r#"{"version":"1.0.0","name":"Untitled","children":[
        {"type":"rectangle","id":"n1","name":"Card","x":0,"y":0,"width":10,"height":10}
    ]}"#));
    assert_eq!(
        state.page_identity_at(0),
        state.active_page_identity(),
        "the page list's only row must carry the identity the badge counts under"
    );
    assert_eq!(state.page_identity_at(0).0, "page-1");
}
