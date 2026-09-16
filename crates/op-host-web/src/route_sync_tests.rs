//! The two rules the address bar is checked against: which part of an address is
//! the document, and where a slug comes from.

use super::parse::{route_file_of, slug_for};

#[test]
fn a_document_part_is_extracted_for_history_comparison() {
    assert_eq!(route_file_of("/"), Some("/".to_string()));
    assert_eq!(route_file_of("/f/abc/slug"), Some("abc".to_string()));
    assert_eq!(route_file_of("/files"), None);
}

#[test]
fn slugs_come_from_the_shared_rule() {
    assert_eq!(slug_for("Список токенов"), "spisok-tokenov");
}
