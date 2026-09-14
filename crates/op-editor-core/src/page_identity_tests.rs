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
    let mut state =
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
