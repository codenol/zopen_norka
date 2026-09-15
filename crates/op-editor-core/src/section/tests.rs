//! Sections as nodes: the marker, the mockups, and the stored shape.

use super::*;
use crate::test_support;
use jian_ops_schema::node::PenNode;

/// A frame with the given role.
fn frame(id: &str, role: Option<&str>) -> PenNode {
    let mut node = test_support::frame(id, "Frame", 0.0, 0.0, 100.0, 100.0, Vec::new());
    if let (PenNode::Frame(frame), Some(role)) = (&mut node, role) {
        frame.base.role = Some(role.to_string());
    }
    node
}

/// A section frame with the given mockups as its children.
fn section(id: &str, children: Vec<PenNode>) -> PenNode {
    let mut node = test_support::frame(id, "Section", 0.0, 0.0, 800.0, 600.0, children);
    if let PenNode::Frame(frame) = &mut node {
        frame.base.role = Some(SECTION_ROLE.to_string());
    }
    node
}

#[test]
fn a_marked_frame_is_a_section() {
    let node = frame("n1", Some(SECTION_ROLE));
    assert!(is_section(&node));
    assert_eq!(
        section_id(&node).map(|id| id.as_str().to_string()),
        Some("n1".to_string())
    );
}

#[test]
fn an_unmarked_frame_is_not_a_section() {
    assert!(!is_section(&frame("n1", None)));
    assert!(!is_section(&frame("n1", Some("button"))));
    // The marker is exact: the schema's `role` is a free-form tag authored by
    // hand and by the HTML importer, so a near miss must not be a section.
    assert!(!is_section(&frame("n1", Some("Section"))));
    assert!(!is_section(&frame("n1", Some("norka:section"))));
}

#[test]
fn a_group_with_the_marker_is_not_a_section() {
    // A section has bounds, a colour and a child list; a group has no bounds of
    // its own to put a title above.
    let mut group = test_support::group("g1", "Group", Vec::new());
    if let PenNode::Group(group) = &mut group {
        group.base.role = Some(SECTION_ROLE.to_string());
    }
    assert!(!is_section(&group));
    assert!(section_frame(&group).is_none());
}

#[test]
fn the_mockups_of_a_section_are_its_direct_children() {
    let node = section("s1", vec![frame("n1", None), frame("n2", Some("screen"))]);
    let mockups = section_mockups(&node);
    assert_eq!(mockups.len(), 2);
    // A screen's own children belong to that screen, not to the section.
    assert_eq!(section_mockups(&mockups[0]).len(), 0);
    // Anything that is not a section has no mockups, so a caller can ask
    // without checking first.
    assert!(section_mockups(&frame("n3", None)).is_empty());
}

#[test]
fn an_unreadable_role_is_not_a_section_id() {
    assert!(section_id(&frame("n1", None)).is_none());
}

#[test]
fn properties_start_empty_and_say_so() {
    let properties = SectionProperties::empty();
    assert!(properties.is_empty());
    assert!(properties.summary.is_empty());
    assert!(properties.analytics.is_empty());
    assert!(properties.flows.is_empty());
}

#[test]
fn linking_the_same_document_twice_replaces_the_link() {
    let mut properties = SectionProperties::empty();
    let first = AnalyticsLink::new(
        "k1",
        "Checkout",
        SectionDigest::of_text("a"),
        SectionDigest::of_text("m"),
        10,
        None,
    );
    let second = AnalyticsLink::new(
        "k1",
        "Checkout v2",
        SectionDigest::of_text("b"),
        SectionDigest::of_text("m"),
        20,
        Some("userA"),
    );
    properties.link(first);
    properties.link(second);
    assert_eq!(properties.analytics.len(), 1);
    assert_eq!(
        properties.link_for("k1").map(|link| link.name.as_str()),
        Some("Checkout v2")
    );
    assert!(properties.unlink("k1").is_some());
    assert!(properties.unlink("k1").is_none());
    assert!(properties.is_empty());
}

#[test]
fn properties_round_trip_through_the_stored_shape() {
    let mut properties = SectionProperties::empty();
    properties.summary.what_it_is = "Оформление заказа".to_string();
    properties.summary.what_to_check = "Пустая корзина".to_string();
    properties.analytics.push(AnalyticsLink::new(
        "k1",
        "Checkout analytics",
        SectionDigest::of_text("analytics"),
        SectionDigest::of_text("mockups"),
        1_700_000_000,
        Some("userA"),
    ));
    properties.flows.push(UxFlow::new("f1", "Checkout"));

    let encoded = StoredSectionProperties::current(properties.clone()).encode();
    let decoded = StoredSectionProperties::decode(&encoded).expect("decode");
    assert_eq!(decoded.properties, properties);
    assert_eq!(decoded.format, SECTION_PROPERTIES_FORMAT);
}

#[test]
fn a_stored_payload_of_an_unknown_format_is_refused() {
    let text = format!(
        r#"{{"format":{},"properties":{{"analytics":[],"summary":{{}},"flows":[]}}}}"#,
        SECTION_PROPERTIES_FORMAT + 1
    );
    assert_eq!(
        StoredSectionProperties::decode(&text),
        Err(SectionFormatError::UnsupportedFormat {
            format: SECTION_PROPERTIES_FORMAT + 1
        })
    );
}

#[test]
fn a_stored_payload_that_is_not_this_shape_is_refused() {
    let error = StoredSectionProperties::decode("not json").expect_err("refused");
    assert!(matches!(error, SectionFormatError::Malformed { .. }));
    assert!(error.to_string().contains("not readable"));
}

#[test]
fn the_format_error_names_what_it_could_not_read() {
    let error = SectionFormatError::UnsupportedFormat { format: 7 };
    assert_eq!(
        error.to_string(),
        "section properties format 7 is not supported"
    );
}

#[test]
fn the_section_tool_draws_a_section_and_the_frame_tool_does_not() {
    use crate::id_allocator::SequentialIdAllocator;
    use crate::walkers::find_node;
    use crate::{EditorState, Tool};

    let mut state = EditorState::new();
    let mut allocator = SequentialIdAllocator::new(1);
    let frame = state
        .create_node_for_tool_with_allocator(Tool::Frame, &mut allocator, 0.0, 0.0, 100.0, 80.0)
        .expect("an id")
        .expect("a node");
    let section = state
        .create_node_for_tool_with_allocator(
            Tool::Section,
            &mut allocator,
            200.0,
            0.0,
            400.0,
            300.0,
        )
        .expect("an id")
        .expect("a node");

    let frame_node = find_node(state.active_children(), &frame).expect("the frame");
    let section_node = find_node(state.active_children(), &section).expect("the section");
    assert!(!is_section(frame_node), "a frame is an ordinary frame");
    assert!(
        is_section(section_node),
        "the section tool marks what it draws"
    );
    assert_eq!(
        section_id(section_node),
        Some(section.clone()),
        "the frame's own id is the section's identity — it is what the properties row is keyed by"
    );
}

#[test]
fn marking_is_refused_for_anything_that_is_not_a_frame() {
    // A section groups SCREENS, and the canvas only lets a screen be dropped
    // into a frame. Promoting a group would create a section nothing can be
    // put into.
    let mut group = test_support::group("g1", "Group", Vec::new());
    assert!(!mark_as_section(&mut group));
    assert!(!is_section(&group));

    let mut frame = frame("n1", None);
    assert!(mark_as_section(&mut frame));
    assert!(is_section(&frame));
    assert!(unmark_section(&mut frame));
    assert!(!is_section(&frame));
    // Unmarking what was never marked is not a change.
    assert!(!unmark_section(&mut frame));
}

#[test]
fn unmarking_keeps_the_screens_the_section_grouped() {
    let mut node = section("s1", vec![frame("m1", None), frame("m2", None)]);

    assert!(unmark_section(&mut node));

    assert_eq!(
        section_mockups(&node).len(),
        0,
        "it is not a section any more"
    );
    let PenNode::Frame(frame) = &node else {
        panic!("still a frame");
    };
    assert_eq!(
        frame.children.as_deref().map(<[PenNode]>::len),
        Some(2),
        "the screens stay: unmarking is not deleting"
    );
}
