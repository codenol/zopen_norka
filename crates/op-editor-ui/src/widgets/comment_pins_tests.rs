//! Pin geometry, placement and hit-testing.
//!
//! The geometry is tested without a canvas: a pin is placed, hit-tested and
//! painted from the same three pure functions, and that is the property these
//! tests are here to hold — not the exact number of pixels a circle is wide.

use super::*;
use crate::layout_scene::{NodeKind, SceneNode};
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::{Color, Point2D, Rect};
use op_editor_core::editor_ui_state::{Comment, CommentAuthor, CommentThread, CommentsUiState};

fn canvas() -> Rect {
    Rect::xywh(240.0, 40.0, 800.0, 600.0)
}

fn thread(id: i64, node: &str, role: Option<&str>, resolved: bool) -> CommentThread {
    CommentThread {
        id,
        node_id: node.to_string(),
        created_at: 1_700_000_000,
        resolved,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![Comment {
            id: id * 10,
            author: CommentAuthor {
                id: Some("u1".to_string()),
                name: "Kay".to_string(),
                role: role.map(str::to_string),
            },
            body: "first".to_string(),
            created_at: 1_700_000_000,
        }],
    }
}

fn source(id: i64, ordinal: usize, node: &str) -> CommentPinThread {
    CommentPinThread {
        thread_id: id,
        ordinal,
        node_id: node.to_string(),
        color: None,
        resolved: false,
        replies: 0,
    }
}

#[test]
fn a_marker_is_centred_on_its_anchor() {
    let anchor = Point2D::new(100.0, 200.0);
    let rect = pin_rect(anchor);
    assert_eq!(
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0
        ),
        anchor
    );
    assert_eq!(rect.size, Point2D::new(PIN_DIAMETER, PIN_DIAMETER));
}

#[test]
fn the_anchor_sits_on_the_elements_top_right_corner() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let anchor = pin_anchor(bounds, canvas(), 0);
    assert_eq!(anchor, Point2D::new(420.0, 200.0));
}

#[test]
fn a_marker_beyond_the_canvas_edge_is_pulled_back_inside_it() {
    let region = canvas();
    // An element scrolled off to the left and above.
    let off_screen = Rect::xywh(-5_000.0, -5_000.0, 100.0, 100.0);
    let anchor = pin_anchor(off_screen, region, 0);
    assert!(anchor.x >= region.origin.x + PIN_INSET);
    assert!(anchor.y >= region.origin.y + PIN_INSET);
    // And the whole marker is inside, so nothing is clipped away from the target.
    let rect = pin_rect(anchor);
    assert!(rect.origin.x >= region.origin.x);
    assert!(rect.origin.y >= region.origin.y);

    // An element far below and right.
    let far = Rect::xywh(9_000.0, 9_000.0, 100.0, 100.0);
    let anchor = pin_anchor(far, region, 0);
    let rect = pin_rect(anchor);
    assert!(rect.origin.x + rect.size.x <= region.origin.x + region.size.x);
    assert!(rect.origin.y + rect.size.y <= region.origin.y + region.size.y);
}

#[test]
fn a_second_marker_on_one_element_steps_clear_of_the_first() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let first = pin_rect(pin_anchor(bounds, canvas(), 0));
    let second = pin_rect(pin_anchor(bounds, canvas(), 1));
    // They must not be drawn on top of each other: a fully overlapping pin is
    // a pin nobody can click.
    assert!(second.origin.y > first.origin.y);
    assert!(second.origin.y - first.origin.y >= PIN_DIAMETER * 0.5);
}

#[test]
fn the_hit_rect_follows_the_painted_rect() {
    let rect = pin_rect(Point2D::new(500.0, 300.0));
    let hit = pin_hit_rect(rect, false);
    assert!(hit.origin.x < rect.origin.x && hit.origin.y < rect.origin.y);
    assert!(hit.size.x >= rect.size.x && hit.size.y >= rect.size.y);
    // Touch is more forgiving than a cursor.
    let touch = pin_hit_rect(rect, true);
    assert!(touch.size.x > hit.size.x);
}

#[test]
fn a_click_inside_a_marker_finds_it_and_a_click_outside_does_not() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let pins = place_pins(&[source(7, 1, "n1")], |_| Some(bounds), canvas());
    let center = Point2D::new(
        pins[0].rect.origin.x + pins[0].rect.size.x / 2.0,
        pins[0].rect.origin.y + pins[0].rect.size.y / 2.0,
    );
    assert_eq!(hit_test(&pins, center, false), Some(7));
    assert_eq!(hit_test(&pins, Point2D::new(0.0, 0.0), false), None);
}

#[test]
fn the_topmost_marker_wins_a_shared_click() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let pins = place_pins(
        &[source(1, 1, "n1"), source(2, 2, "n1")],
        |_| Some(bounds),
        canvas(),
    );
    assert_eq!(pins.len(), 2);
    // A point in the second marker's pad that is also inside the first's pad:
    // the one painted last is the one the user sees on top.
    let overlapping = Point2D::new(
        pins[1].rect.origin.x + pins[1].rect.size.x / 2.0,
        pins[1].rect.origin.y - 1.0,
    );
    assert_eq!(hit_test(&pins, overlapping, false), Some(2));
}

#[test]
fn a_thread_whose_element_is_gone_gets_no_marker() {
    let present = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let pins = place_pins(
        &[source(1, 1, "n1"), source(2, 2, "gone"), source(3, 3, "n1")],
        |id| (id == "n1").then_some(present),
        canvas(),
    );
    // The ordinals are the list positions, so a gap in the numbers is the
    // honest picture: thread 2 exists and has nowhere to be drawn.
    assert_eq!(
        pins.iter().map(|pin| pin.thread_id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(
        pins.iter().map(|pin| pin.ordinal).collect::<Vec<_>>(),
        vec![1, 3]
    );
}

#[test]
fn markers_against_the_tree_land_on_the_elements_visible_bounds() {
    let mut child = SceneNode::leaf("n1", NodeKind::Rect);
    child.bounds = Rect::xywh(10.0, 30.0, 60.0, 60.0);
    let mut frame = SceneNode::leaf("f1", NodeKind::Frame);
    frame.bounds = Rect::xywh(0.0, 20.0, 200.0, 300.0);
    frame.children = vec![child];

    let viewport = Viewport {
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
    };
    let region = Rect::xywh(0.0, 0.0, 800.0, 600.0);
    let pins = scene_pins(&[source(1, 1, "n1")], &[frame], region, &viewport);
    assert_eq!(pins.len(), 1);
    // The child's own bounds, not the frame's: the pin is about the element the
    // comment names.
    assert_eq!(
        pins[0].rect.origin,
        Point2D::new(70.0 - PIN_DIAMETER / 2.0, 30.0 - PIN_DIAMETER / 2.0)
    );
}

#[test]
fn zoom_moves_the_marker_with_the_element_it_pins() {
    let mut node = SceneNode::leaf("n1", NodeKind::Rect);
    node.bounds = Rect::xywh(10.0, 30.0, 60.0, 60.0);
    let region = Rect::xywh(0.0, 0.0, 800.0, 600.0);
    let at_one = scene_pins(
        &[source(1, 1, "n1")],
        &[node.clone()],
        region,
        &Viewport {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        },
    );
    let at_two = scene_pins(
        &[source(1, 1, "n1")],
        &[node],
        region,
        &Viewport {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 2.0,
        },
    );
    assert!(at_two[0].rect.origin.x > at_one[0].rect.origin.x);
    assert!(at_two[0].rect.origin.y > at_one[0].rect.origin.y);
}

#[test]
fn a_hidden_or_empty_element_has_no_marker_to_aim_at() {
    let mut hidden = SceneNode::leaf("hidden", NodeKind::Rect);
    hidden.bounds = Rect::xywh(10.0, 30.0, 60.0, 60.0);
    hidden.hidden = true;
    let mut empty = SceneNode::leaf("empty", NodeKind::Rect);
    empty.bounds = Rect::xywh(10.0, 30.0, 0.0, 0.0);
    let region = Rect::xywh(0.0, 0.0, 800.0, 600.0);
    let viewport = Viewport {
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
    };
    assert!(scene_pins(&[source(1, 1, "hidden")], &[hidden], region, &viewport).is_empty());
    assert!(scene_pins(&[source(1, 1, "empty")], &[empty], region, &viewport).is_empty());
    assert!(scene_pins(&[source(1, 1, "absent")], &[], region, &viewport).is_empty());
}

#[test]
fn the_threads_carried_into_pins_keep_the_lists_own_numbers_and_colours() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "n1", Some("ux_ui"), false),
        thread(2, "n2", Some("chief-vibes-officer"), false),
        thread(3, "n3", None, true),
    ]);
    let sources = threads_for(&ui);
    assert_eq!(
        sources.iter().map(|s| s.ordinal).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(sources[0].color, crate::util::parse_hex_color("#8B5CF6"));
    // A role this build does not know gets no colour at all, rather than one of
    // the seven it does know.
    assert!(sources[1].color.is_none());
    assert!(sources[2].color.is_none());
    assert!(sources[2].resolved);
}

#[test]
fn a_thread_with_no_comments_has_no_role_to_colour_its_pin() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![CommentThread {
        id: 1,
        node_id: "n1".to_string(),
        comments: vec![],
        ..CommentThread::default()
    }]);
    let sources = threads_for(&ui);
    assert_eq!(sources.len(), 1);
    assert!(sources[0].color.is_none());
    assert_eq!(sources[0].replies, 0);
}

#[test]
fn the_glyph_is_two_digits_and_then_a_cap() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let pins = place_pins(
        &[source(1, 7, "n1"), source(2, 120, "n1")],
        |_| Some(bounds),
        canvas(),
    );
    assert_eq!(pins[0].label(), "7");
    assert_eq!(pins[1].label(), "99+");
}

#[test]
fn painting_a_marker_writes_its_number_and_the_authors_colour() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let mut thread_source = source(1, 4, "n1");
    thread_source.color = crate::util::parse_hex_color("#22C55E");
    let pins = place_pins(&[thread_source], |_| Some(bounds), canvas());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint(&mut cx, &Theme::dark(), &pins, None);

    assert_eq!(
        backend
            .texts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["4"]
    );
    let open_fill = backend.round_fills.iter().find(|(rect, _, color)| {
        *rect == pins[0].rect && *color != Theme::dark().card.with_alpha(0.9)
    });
    assert!(
        open_fill.is_some_and(|(_, _, color)| *color == crate::util::parse_hex_color("#22C55E").unwrap()),
        "an open pin is filled with the author's role colour"
    );
}

#[test]
fn a_resolved_marker_is_drawn_as_an_outline_and_a_roled_one_is_not_coloured_in() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let mut closed = source(1, 1, "n1");
    closed.resolved = true;
    closed.color = crate::util::parse_hex_color("#22C55E");
    let pins = place_pins(&[closed], |_| Some(bounds), canvas());
    let theme = Theme::dark();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint(&mut cx, &theme, &pins, None);

    let green = crate::util::parse_hex_color("#22C55E").unwrap();
    assert!(
        backend
            .round_fills
            .iter()
            .all(|(_, _, color)| *color != green),
        "a closed thread paints its colour as an outline, never as a solid claim of an open one"
    );
    assert_eq!(
        backend.texts.len(),
        1,
        "the number is still there to find it by"
    );
    let _ = Color::rgb_u8(0, 0, 0);
}

#[test]
fn hover_does_not_move_the_marker_out_from_under_the_click() {
    let bounds = Rect::xywh(300.0, 200.0, 120.0, 60.0);
    let pins = place_pins(&[source(1, 1, "n1")], |_| Some(bounds), canvas());
    let center = Point2D::new(
        pins[0].rect.origin.x + pins[0].rect.size.x / 2.0,
        pins[0].rect.origin.y + pins[0].rect.size.y / 2.0,
    );
    assert_eq!(hit_test(&pins, center, false), Some(1));
    // The hover ring is painted 1 px wider than the rest state; the hit rect is
    // derived from the rest rect, so hovering cannot take the pin away from the
    // click that is already on its way.
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint(&mut cx, &Theme::dark(), &pins, Some(1));
    assert_eq!(hit_test(&pins, center, false), Some(1));
}
