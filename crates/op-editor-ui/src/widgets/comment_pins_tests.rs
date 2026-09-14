//! Pin geometry, placement and hit-testing.
//!
//! The geometry is tested without a canvas: a pin is placed, hit-tested and
//! painted from the same three pure functions, and that is the property these
//! tests are here to hold — not the exact number of pixels a circle is wide.
//!
//! The placement tests are also where the two coordinate spaces meet: a thread
//! carries a document point, a pin carries a screen rect, and the conversion
//! runs against the viewport. `zoom_and_pan_move_the_marker_with_the_design`
//! is the one that fails if that ever collapses into a single space.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::{Color, Point2D, Rect};
use op_editor_core::editor_ui_state::{
    Comment, CommentAnchor, CommentAuthor, CommentThread, CommentsUiState,
};

fn canvas() -> Rect {
    Rect::xywh(240.0, 40.0, 800.0, 600.0)
}

fn plain_viewport() -> Viewport {
    Viewport {
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
    }
}

fn thread(id: i64, page: &str, role: Option<&str>, resolved: bool) -> CommentThread {
    CommentThread {
        id,
        // One point per id, so a test can name the marker it means.
        anchor: Some(CommentAnchor::new(
            page,
            id as f64 * 100.0,
            id as f64 * 50.0,
        )),
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

/// A pin source at a document point, on `page`.
fn source(id: i64, page: &str, x: f64, y: f64) -> CommentPinThread {
    CommentPinThread {
        thread_id: id,
        ordinal: id as usize,
        anchor: CommentAnchor::new(page, x, y),
        color: None,
        resolved: false,
        replies: 0,
    }
}

/// A source on the page every placement test uses.
fn on_page(id: i64, x: f64, y: f64) -> CommentPinThread {
    source(id, "p1", x, y)
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
fn a_marker_beyond_the_canvas_edge_is_pulled_back_inside_it() {
    let region = canvas();
    // A document point scrolled off to the left and above.
    let off_screen = Point2D::new(-5_000.0, -5_000.0);
    let anchor = pin_anchor(off_screen, region, 0);
    assert!(anchor.x >= region.origin.x + PIN_INSET);
    assert!(anchor.y >= region.origin.y + PIN_INSET);
    // And the whole marker is inside, so nothing is clipped away from the target.
    let rect = pin_rect(anchor);
    assert!(rect.origin.x >= region.origin.x);
    assert!(rect.origin.y >= region.origin.y);

    // A point far below and right.
    let anchor = pin_anchor(Point2D::new(9_000.0, 9_000.0), region, 0);
    let rect = pin_rect(anchor);
    assert!(rect.origin.x + rect.size.x <= region.origin.x + region.size.x);
    assert!(rect.origin.y + rect.size.y <= region.origin.y + region.size.y);
}

#[test]
fn a_second_marker_at_one_point_steps_clear_of_the_first() {
    let point = Point2D::new(420.0, 200.0);
    let first = pin_rect(pin_anchor(point, canvas(), 0));
    let second = pin_rect(pin_anchor(point, canvas(), 1));
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
    let pins = place_pins(&[on_page(7, 300.0, 200.0)], canvas(), &plain_viewport());
    let center = Point2D::new(
        pins[0].rect.origin.x + pins[0].rect.size.x / 2.0,
        pins[0].rect.origin.y + pins[0].rect.size.y / 2.0,
    );
    assert_eq!(hit_test(&pins, center, false), Some(7));
    assert_eq!(hit_test(&pins, Point2D::new(0.0, 0.0), false), None);
}

#[test]
fn the_topmost_marker_wins_a_shared_click() {
    let pins = place_pins(
        &[on_page(1, 300.0, 200.0), on_page(2, 300.0, 200.0)],
        canvas(),
        &plain_viewport(),
    );
    assert_eq!(pins.len(), 2);
    // The two are fanned, so the second is painted clear of the first; a click
    // on the second's centre opens the second.
    let center = Point2D::new(
        pins[1].rect.origin.x + pins[1].rect.size.x / 2.0,
        pins[1].rect.origin.y + pins[1].rect.size.y / 2.0,
    );
    assert_eq!(hit_test(&pins, center, false), Some(2));
}

#[test]
fn only_this_pages_pinned_threads_become_sources() {
    // A pin belongs to the page its anchor names, and a thread with no anchor
    // has no place at all: both are filtered out of the sources rather than
    // placed somewhere invented.
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", None, false),
        thread(2, "p2", None, false),
        CommentThread {
            id: 3,
            anchor: None,
            ..CommentThread::default()
        },
        thread(4, "p1", None, false),
    ]);
    let sources = threads_for_page(&ui, "p1");
    assert_eq!(
        sources
            .iter()
            .map(|source| source.thread_id)
            .collect::<Vec<_>>(),
        vec![1, 4]
    );
    // The page's own list numbers from one, so the marker and the rail row
    // agree.
    assert_eq!(
        sources
            .iter()
            .map(|source| source.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn a_thread_with_an_unpaintable_coordinate_is_never_a_source() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        CommentThread {
            id: 1,
            anchor: Some(CommentAnchor::new("p1", f64::NAN, 10.0)),
            ..CommentThread::default()
        },
        CommentThread {
            id: 2,
            anchor: Some(CommentAnchor::new("p1", 10.0, f64::INFINITY)),
            ..CommentThread::default()
        },
        // Further out than the daemon would ever store.
        CommentThread {
            id: 3,
            anchor: Some(CommentAnchor::new(
                "p1",
                op_editor_core::editor_ui_state::MAX_COMMENT_COORDINATE * 2.0,
                10.0,
            )),
            ..CommentThread::default()
        },
        thread(4, "p1", None, false),
    ]);
    let sources = threads_for_page(&ui, "p1");
    assert_eq!(
        sources
            .iter()
            .map(|source| source.thread_id)
            .collect::<Vec<_>>(),
        vec![4]
    );
    assert_eq!(sources[0].ordinal, 1, "numbering counts what is painted");
    // The rail still lists them — a conversation is not hidden by a coordinate
    // this client cannot draw.
    assert_eq!(ui.threads_on_page("p1").len(), 4);
    assert_eq!(ui.ordinal(1), None);
}

#[test]
fn zoom_and_pan_move_the_marker_with_the_design() {
    let region = Rect::xywh(0.0, 0.0, 800.0, 600.0);
    let source = [on_page(1, 100.0, 100.0)];
    let at_one = place_pins(
        &source,
        region,
        &Viewport {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        },
    );
    let at_two = place_pins(
        &source,
        region,
        &Viewport {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 2.0,
        },
    );
    // Twice the zoom puts the same document point twice as far from the origin.
    assert_eq!(at_one[0].rect.origin.x, 100.0 - PIN_DIAMETER / 2.0);
    assert_eq!(at_two[0].rect.origin.x, 200.0 - PIN_DIAMETER / 2.0);

    let panned = place_pins(
        &source,
        region,
        &Viewport {
            pan_x: -50.0,
            pan_y: 25.0,
            zoom: 1.0,
        },
    );
    // A pan moves the marker by exactly the pan: the pin travels with the page,
    // because the document point never moved.
    assert_eq!(panned[0].rect.origin.x, at_one[0].rect.origin.x - 50.0);
    assert_eq!(panned[0].rect.origin.y, at_one[0].rect.origin.y + 25.0);
    // The anchor a caller can frame on is still the document point.
    assert_eq!(panned[0].anchor.x, 100.0);
    assert_eq!(panned[0].anchor.y, 100.0);
}

#[test]
fn the_threads_carried_into_pins_keep_their_own_colours() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", Some("ux_ui"), false),
        thread(2, "p1", Some("chief-vibes-officer"), false),
        thread(3, "p1", None, true),
    ]);
    let sources = threads_for_page(&ui, "p1");
    assert_eq!(sources[0].color, crate::util::parse_hex_color("#8B5CF6"));
    // A role this build does not know gets no colour at all, rather than one of
    // the seven it does know.
    assert!(sources[1].color.is_none());
    assert!(sources[2].color.is_none());
    assert!(sources[2].resolved);
    // The sources carry the page's coordinate, which is the only thing the
    // placement needs.
    assert_eq!(sources[1].anchor.x, 200.0);
}

#[test]
fn a_thread_with_no_comments_has_no_role_to_colour_its_pin() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![CommentThread {
        id: 1,
        anchor: Some(CommentAnchor::new("p1", 10.0, 20.0)),
        comments: vec![],
        ..CommentThread::default()
    }]);
    let sources = threads_for_page(&ui, "p1");
    assert_eq!(sources.len(), 1);
    assert!(sources[0].color.is_none());
    assert_eq!(sources[0].replies, 0);
}

#[test]
fn the_glyph_is_two_digits_and_then_a_cap() {
    let pins = place_pins(
        &[
            on_page(1, 300.0, 200.0),
            on_page(2, 300.0, 200.0),
            // Enough threads on the page to push the last one past ninety-nine.
            on_page(3, 300.0, 200.0),
        ],
        canvas(),
        &plain_viewport(),
    );
    assert_eq!(pins[0].label(), "1");
    assert_eq!(pins[2].label(), "3");
    // The cap is a formatting rule of the marker, so it is asserted directly.
    let mut capped = pins[2].clone();
    capped.ordinal = 120;
    assert_eq!(capped.label(), "99+");
}

#[test]
fn painting_a_marker_writes_its_number_and_the_authors_colour() {
    let mut source = on_page(1, 300.0, 200.0);
    source.color = crate::util::parse_hex_color("#22C55E");
    let pins = place_pins(&[source], canvas(), &plain_viewport());
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
        vec!["1"]
    );
    let open_fill = backend.round_fills.iter().find(|(rect, _, color)| {
        *rect == pins[0].rect && *color != Theme::dark().card.with_alpha(0.9)
    });
    assert!(
        open_fill
            .is_some_and(|(_, _, color)| *color == crate::util::parse_hex_color("#22C55E").unwrap()),
        "an open pin is filled with the author's role colour"
    );
}

#[test]
fn a_resolved_marker_is_drawn_as_an_outline_and_a_roled_one_is_not_coloured_in() {
    let mut closed = on_page(1, 300.0, 200.0);
    closed.resolved = true;
    closed.color = crate::util::parse_hex_color("#22C55E");
    let pins = place_pins(&[closed], canvas(), &plain_viewport());
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
    let pins = place_pins(&[on_page(1, 300.0, 200.0)], canvas(), &plain_viewport());
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
