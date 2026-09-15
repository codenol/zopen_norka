//! Fingerprints: what they catch, and what they deliberately do not.

use super::*;
use crate::test_support;
use jian_ops_schema::node::PenNode;

fn section_with(children: Vec<PenNode>) -> PenNode {
    let mut node = test_support::frame("s1", "Section", 0.0, 0.0, 800.0, 600.0, children);
    if let PenNode::Frame(frame) = &mut node {
        frame.base.role = Some(crate::section::SECTION_ROLE.to_string());
    }
    node
}

#[test]
fn a_digest_is_sixty_four_lower_case_hex_characters() {
    let digest = SectionDigest::of_text("anything");
    assert_eq!(digest.as_str().len(), SectionDigest::HEX_LEN);
    assert!(digest
        .as_str()
        .chars()
        .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch)));
    assert_eq!(digest.to_string(), digest.as_str());
}

#[test]
fn the_same_bytes_always_give_the_same_digest() {
    assert_eq!(
        SectionDigest::of_text("cart"),
        SectionDigest::of_text("cart")
    );
    assert_ne!(
        SectionDigest::of_text("cart"),
        SectionDigest::of_text("Cart")
    );
    // One character is a change: the digest is not a summary of a document,
    // it is a check on one.
    assert_ne!(
        SectionDigest::of_text("cart"),
        SectionDigest::of_text("cart ")
    );
}

#[test]
fn two_fields_cannot_be_confused_for_one() {
    // Length prefixes, so "ab"+"c" and "a"+"bc" are not the same digest. Stated
    // as a test because the whole point of the fingerprint is that two different
    // sections never collide.
    let a = analytics_fingerprint("ab");
    let b = analytics_fingerprint("a");
    assert_ne!(a, b);
    assert_ne!(
        mockup_fingerprint_of(&[]),
        mockup_fingerprint_of(&[test_support::rect("n1", "R", 0.0, 0.0, 10.0, 10.0)])
    );
}

#[test]
fn the_analytics_fingerprint_ignores_line_endings_and_trailing_space() {
    let unix = "# Checkout\n\n- one\n- two\n";
    let windows = "# Checkout\r\n\r\n- one\r\n- two\r\n";
    assert_eq!(analytics_fingerprint(unix), analytics_fingerprint(windows));
    assert_eq!(
        analytics_fingerprint(unix),
        analytics_fingerprint("# Checkout\n\n- one\n- two")
    );
}

#[test]
fn the_analytics_fingerprint_catches_an_edit_anywhere_in_the_prose() {
    let before = "# Checkout\n\nWhat the feature is for.\n";
    let after = "# Checkout\n\nWhat the feature is FOR.\n";
    assert_ne!(analytics_fingerprint(before), analytics_fingerprint(after));
    // ...including one that only removes something.
    assert_ne!(
        analytics_fingerprint(before),
        analytics_fingerprint("# Checkout\n")
    );
}

#[test]
fn the_mockup_fingerprint_changes_when_a_screen_changes() {
    let before = section_with(vec![test_support::rect(
        "n1", "Screen", 0.0, 0.0, 375.0, 812.0,
    )]);
    let moved = section_with(vec![test_support::rect(
        "n1", "Screen", 0.0, 40.0, 375.0, 812.0,
    )]);
    let resized = section_with(vec![test_support::rect(
        "n1", "Screen", 0.0, 0.0, 375.0, 700.0,
    )]);
    let renamed = section_with(vec![test_support::rect(
        "n1", "Cart", 0.0, 0.0, 375.0, 812.0,
    )]);
    let base = mockup_fingerprint(&before);
    assert_ne!(base, mockup_fingerprint(&moved), "a screen moved");
    assert_ne!(base, mockup_fingerprint(&resized), "a screen resized");
    assert_ne!(base, mockup_fingerprint(&renamed), "a screen renamed");
}

#[test]
fn the_mockup_fingerprint_catches_an_edit_inside_a_screen() {
    // The whole subtree counts: a label inside a screen is part of what the
    // section shows, and an analytics document read against the old label is
    // exactly the thing the mark exists to say.
    let child = |content: &str| test_support::text("n2", "Title", 0.0, 0.0, 100.0, 20.0, content);
    let screen = |content: &str| {
        test_support::frame("n1", "Screen", 0.0, 0.0, 375.0, 812.0, vec![child(content)])
    };
    assert_ne!(
        mockup_fingerprint_of(&[screen("Корзина")]),
        mockup_fingerprint_of(&[screen("Корзина пуста")])
    );
}

#[test]
fn the_mockup_fingerprint_catches_adding_removing_and_reordering() {
    let a = || test_support::rect("n1", "A", 0.0, 0.0, 100.0, 100.0);
    let b = || test_support::rect("n2", "B", 0.0, 200.0, 100.0, 100.0);
    let one = mockup_fingerprint_of(&[a()]);
    let two = mockup_fingerprint_of(&[a(), b()]);
    let swapped = mockup_fingerprint_of(&[b(), a()]);
    assert_ne!(one, two, "a screen was added");
    assert_ne!(two, swapped, "the screens were reordered");
    assert_ne!(
        one,
        mockup_fingerprint_of(&[]),
        "the last screen was removed"
    );
}

#[test]
fn the_mockup_fingerprint_ignores_how_the_section_looks() {
    // Renaming a section, moving its own bounds or recolouring it is not a
    // change to its screens — and would be circular besides, since the
    // properties hang off this very node.
    let children = vec![test_support::rect("n1", "Screen", 0.0, 0.0, 375.0, 812.0)];
    let plain = section_with(children.clone());
    let mut renamed = section_with(children.clone());
    if let PenNode::Frame(frame) = &mut renamed {
        frame.base.name = Some("Checkout".to_string());
        frame.base.x = Some(1200.0);
    }
    assert_eq!(mockup_fingerprint(&plain), mockup_fingerprint(&renamed));
}

#[test]
fn a_frame_that_is_not_a_section_has_no_mockups_to_have_changed() {
    let plain = test_support::rect("n9", "Not a section", 0.0, 0.0, 10.0, 10.0);
    assert_eq!(
        mockup_fingerprint(&plain),
        mockup_fingerprint_of(&[]),
        "an empty section and a non-section answer the same, which is honest"
    );
}
