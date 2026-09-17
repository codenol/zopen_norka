//! Reading a document back: page-0 inventory, the DSL statement scanner and the
//! name-based attribution. All pure — the payloads here are trimmed copies of
//! the shapes the two hand passes recorded.

use super::document::{attribution, page0, root_statement_count, scan_dsl_statements};
use serde_json::json;

/// A read-back payload with one screen and a nested child.
fn payload() -> serde_json::Value {
    json!({
        "version": 57,
        "document": {
            // Arbitrary: this layer never reads the document version, and a
            // fixture must not carry the current product version (the
            // version-sync gate scans for exactly that).
            "version": "1.0.0",
            "pages": [
                { "id": "page-0", "name": "Page 1", "children": [
                    { "id": "n20", "type": "frame", "name": "Sign-in Card", "children": [
                        { "id": "n21", "type": "text", "name": "Title", "content": "Sign in" },
                        { "id": "n22", "type": "frame", "name": "Email Field", "children": [
                            { "id": "n23", "type": "text", "name": "Label", "content": "Email" }
                        ] }
                    ] }
                ] },
                { "id": "components", "name": "Components/Buttons", "children": [
                    { "id": "c1", "type": "frame", "name": "atom-button", "children": [] }
                ] }
            ]
        }
    })
}

#[test]
fn page0_counts_only_the_active_page() {
    let inventory = page0(&payload(), 0);
    // 4 nodes on page 0, and none of the kit page's nodes.
    assert_eq!(inventory.nodes, 4);
    assert_eq!(inventory.pages, 2);
    assert_eq!(inventory.frames.len(), 1);
    assert_eq!(inventory.frames[0].name, "Sign-in Card");
    assert_eq!(inventory.frames[0].kind, "frame");
    assert_eq!(inventory.frames[0].nodes, 4);
    assert_eq!(inventory.frame_names(), vec!["Sign-in Card".to_string()]);
}

#[test]
fn a_bare_document_is_read_the_same_as_a_wrapped_payload() {
    let wrapped = payload();
    let bare = wrapped.get("document").cloned().expect("document");
    assert_eq!(page0(&wrapped, 0).nodes, page0(&bare, 0).nodes);
}

#[test]
fn an_empty_page_reports_no_screen() {
    let empty = json!({ "document": { "pages": [ { "children": [] } ] } });
    let inventory = page0(&empty, 0);
    assert_eq!(inventory.nodes, 0);
    assert!(inventory.frames.is_empty());
}

#[test]
fn the_scanner_finds_root_and_nested_statements() {
    let reply = "\
<step>Composing the card.</step>\n\
I(null, {id:\"n20\", type:\"frame\", name:\"Sign-in Card\", children:[\n\
I(n20, {id:\"n21\", type:\"text\", name:\"Title\", content:\"Sign in\"});\n\
] });\n\
<!-- APPLIED -->\n";
    let statements = scan_dsl_statements(reply);
    assert_eq!(statements.len(), 2, "{statements:?}");
    assert_eq!(statements[0].parent, "null");
    assert_eq!(statements[0].name, "Sign-in Card");
    assert_eq!(statements[0].kind, "frame");
    assert_eq!(statements[1].parent, "n20");
    assert_eq!(statements[1].name, "Title");
    assert_eq!(root_statement_count(&statements), 1);
}

#[test]
fn a_statement_whose_keys_are_ordered_differently_is_still_found() {
    // The hand prototype's regex demanded `id, type, name` adjacency; a reply
    // that orders them otherwise used to count as "nothing landed".
    let reply = "I(null, {type:\"frame\", name:\"Pricing\", id:\"n30\"});\n";
    let statements = scan_dsl_statements(reply);
    assert_eq!(statements.len(), 1);
    assert_eq!(statements[0].name, "Pricing");
    assert_eq!(statements[0].id, "n30");
}

#[test]
fn a_statement_without_a_name_is_skipped_rather_than_invented() {
    let reply = "I(null, {id:\"n30\", type:\"frame\"});\n";
    assert!(scan_dsl_statements(reply).is_empty());
    // A nested statement must not donate its keys to the statement above it.
    let nested = "I(null, {id:\"n30\", type:\"frame\", children:[\nI(n30, {id:\"n31\", type:\"text\", name:\"Title\"});\n]});\n";
    let statements = scan_dsl_statements(nested);
    assert_eq!(statements.len(), 1);
    assert_eq!(statements[0].name, "Title");
    assert_eq!(root_statement_count(&statements), 0);
}

#[test]
fn attribution_matches_emitted_names_against_the_page() {
    let inventory = page0(&payload(), 0);
    let emit = |name: &str| format!("I(null, {{id:\"n20\", type:\"frame\", name:\"{name}\"}});\n");
    // The emitted names are the credential: a frame that is on the page AND was
    // named by this turn's own reply is this turn's node, and a name the reply
    // never mentioned is not.
    let statements = scan_dsl_statements(&format!(
        "{}{}",
        emit("Sign-in Card"),
        emit("Somebody Elses Frame")
    ));
    let (landed, nodes) = attribution(&statements, &inventory);
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].name, "Sign-in Card");
    assert_eq!(nodes, 4);
    // A chat-route reply carries no DSL at all, so nothing is attributed even
    // though the page is not empty.
    let (none, zero) = attribution(&scan_dsl_statements("Done — 1 subtask(s)."), &inventory);
    assert!(none.is_empty());
    assert_eq!(zero, 0);
}
