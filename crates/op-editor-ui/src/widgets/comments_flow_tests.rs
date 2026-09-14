//! The comment flow's placement, press routing and state effects.
//!
//! The placement tests are the ones issue #53 is about: a composer for a comment
//! that has not been written yet must open at the point it will be pinned to,
//! not in a corner of the canvas.

use super::*;
use crate::widgets::comment_pins::CommentPinThread;
use crate::widgets::comment_thread_popover::CommentThreadPopover;
use op_editor_core::editor_ui_state::{Comment, CommentAnchor, CommentAuthor, CommentThread};

/// A rail: the right-hand slot the inspector would occupy.
fn rail() -> Rect {
    Rect::xywh(1040.0, 40.0, 240.0, 600.0)
}

/// The canvas the flow is placed against, on the page `editor()` writes to.
fn canvas() -> CommentCanvas<'static> {
    CommentCanvas::new(Rect::xywh(240.0, 40.0, 800.0, 600.0), "p1")
}

/// An editor state with one thread at a known point and one on another page.
/// The page id a state gives its own page — read, never spelled: the canvas and
/// the rail both key on this string, and a test that hard-coded its own would
/// pass while the flow looked for a page nobody had named.
fn page_of(state: &EditorState) -> String {
    state.active_page_identity().0
}

/// Two pages, so "this page" and "another page" are both real.
const TWO_PAGES: &str = r#"{"version":"1.0.0","pages":[
  {"id":"p1","name":"Page 1","children":[
    {"type":"rectangle","id":"n1","name":"Card","x":0,"y":0,"width":10,"height":10}
  ]},
  {"id":"p2","name":"Page 2","children":[]}
]}"#;

fn editor() -> EditorState {
    let doc = jian_ops_schema::load_str(TWO_PAGES)
        .expect("the fixture parses")
        .value;
    let mut state = EditorState::from_document(doc);
    state.editor_ui.locale = op_i18n::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_000_000.0;
    state.editor_ui.comments.transport = true;
    debug_assert_eq!(page_of(&state), "p1", "the fixture names its page p1");
    state.editor_ui.comments.install_threads(vec![
        CommentThread {
            id: 1,
            anchor: Some(CommentAnchor::new("p1", 360.0, 260.0)),
            created_at: 1_699_999_700,
            resolved: false,
            resolved_at: None,
            resolved_by: None,
            resolved_by_name: None,
            comments: vec![Comment {
                id: 10,
                author: CommentAuthor {
                    id: Some("u1".to_string()),
                    name: "Kay".to_string(),
                    role: Some("ux_ui".to_string()),
                },
                body: "this spacing looks off".to_string(),
                created_at: 1_699_999_700,
            }],
        },
        CommentThread {
            id: 2,
            anchor: Some(CommentAnchor::new("p2", 100.0, 100.0)),
            created_at: 1_699_999_800,
            resolved: false,
            resolved_at: None,
            resolved_by: None,
            resolved_by_name: None,
            comments: vec![Comment {
                id: 20,
                author: CommentAuthor {
                    id: None,
                    name: String::new(),
                    role: None,
                },
                body: "about another page".to_string(),
                created_at: 1_699_999_800,
            }],
        },
    ]);
    state
}

/// A pin as the canvas would place it for the same state.
fn pin(thread_id: i64, rect: Rect) -> CommentPin {
    CommentPin {
        thread_id,
        ordinal: thread_id as usize,
        anchor: CommentAnchor::new("p1", 360.0, 260.0),
        rect,
        color: None,
        resolved: false,
        replies: 0,
    }
}

fn painted_pin(thread_id: i64, x: f32, y: f32) -> CommentPin {
    pin(thread_id, comment_pins::pin_rect(Point2D::new(x, y)))
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn nothing_paints_without_an_open_composer() {
    let state = editor();
    assert!(popover_for(&state).is_none());
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    assert!(paint_popover(&mut cx, &state, canvas(), &[]).is_none());
}

#[test]
fn the_popover_hangs_under_the_pin_of_its_own_thread() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    assert_eq!(
        popover_anchor(&state, canvas(), &pins),
        comment_pins::pin_rect(Point2D::new(600.0, 300.0))
    );
}

#[test]
fn a_composer_for_an_unwritten_comment_opens_at_the_point_it_will_pin() {
    // The bug this test exists for: a click in the middle of the page opened the
    // field in the canvas' top-left corner, and the pin then landed somewhere
    // the reviewer never clicked.
    let mut state = editor();
    state
        .editor_ui
        .comments
        .begin_thread_at(CommentAnchor::new("p1", 360.0, 260.0));
    let anchor = popover_anchor(&state, canvas(), &[]);
    // The same rect the pin will occupy once the thread exists: the document
    // point through the viewport (identity pan/zoom) plus the canvas origin.
    assert_eq!(
        anchor,
        comment_pins::pin_rect(Point2D::new(240.0 + 360.0, 40.0 + 260.0))
    );
    assert_ne!(
        anchor.origin,
        Point2D::new(canvas().rect.origin.x + 8.0, canvas().rect.origin.y + 8.0),
        "not the canvas corner"
    );

    // And once the thread comes back from the server, the pin is placed at the
    // same point — the marker lands under the field that wrote it.
    let placed = comment_pins::place_pins(
        &[CommentPinThread {
            thread_id: 9,
            ordinal: 1,
            anchor: CommentAnchor::new("p1", 360.0, 260.0),
            color: None,
            resolved: false,
            replies: 0,
        }],
        canvas().rect,
        &state.viewport,
    );
    assert_eq!(placed[0].rect, anchor);
}

#[test]
fn a_composer_on_another_page_has_nowhere_on_this_canvas_to_hang() {
    let mut state = editor();
    state
        .editor_ui
        .comments
        .begin_thread_at(CommentAnchor::new("p2", 10.0, 10.0));
    // The point exists, but not on the page being shown, so the box falls back
    // to the canvas corner rather than being placed from the wrong page's
    // coordinates.
    assert_eq!(
        popover_anchor(&state, canvas(), &[]).origin,
        Point2D::new(248.0, 48.0)
    );
}

#[test]
fn a_press_outside_the_popover_closes_it_and_does_not_reach_the_canvas() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins).unwrap();

    let consumed = press_popover(
        &mut state,
        Some(rect),
        Point2D::new(rect.origin.x - 50.0, rect.origin.y - 50.0),
    );
    assert!(
        consumed,
        "the click that dismisses the panel is the panel's"
    );
    assert!(state.editor_ui.comments.composer().is_none());
}

#[test]
fn a_press_on_send_queues_the_reply_and_a_press_on_close_does_not() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    state.editor_ui.comments.reply_draft = "it does".to_string();
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins).unwrap();

    assert!(press_popover(
        &mut state,
        Some(rect),
        center(CommentThreadPopover::send_rect(rect))
    ));
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Reply {
            thread_id: 1,
            text: "it does".to_string(),
        }]
    );
}

#[test]
fn the_resolution_button_asks_for_the_opposite_of_the_threads_state() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins).unwrap();
    let resolution = center(CommentThreadPopover::resolution_rect(rect));

    assert!(press_popover(&mut state, Some(rect), resolution));
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Resolve { thread_id: 1 }]
    );

    // Once the server says it is closed, the same button reopens it.
    state.editor_ui.comments.threads[0].resolved = true;
    assert!(press_popover(&mut state, Some(rect), resolution));
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Reopen { thread_id: 1 }]
    );
}

#[test]
fn pressing_the_field_gives_it_the_keyboard() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    state.editor_ui.comments.blur_composer();
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins).unwrap();
    assert!(press_popover(
        &mut state,
        Some(rect),
        center(CommentThreadPopover::input_rect(rect))
    ));
    assert!(state.editor_ui.comments.composer_focused);
    // The host's "a text input owns the keyboard" rule reads this, and the
    // comment field must answer yes from the click that opened it (issue #49).
    assert!(state.editor_ui.comments.takes_keyboard());
}

#[test]
fn a_composer_clicked_at_the_canvas_edge_still_lands_inside_the_window() {
    // The reviewer's own case: a click near the right edge of the page. The box
    // has to be readable without leaving the canvas, whatever the point.
    let region = canvas();
    for (x, y) in [
        (region.rect.origin.x + region.rect.size.x - 1.0, 300.0),
        (region.rect.origin.x + 1.0, 60.0),
        (500.0, region.rect.origin.y + region.rect.size.y - 1.0),
        (-400.0, -300.0),
    ] {
        let mut state = editor();
        state
            .editor_ui
            .comments
            .begin_thread_at(CommentAnchor::new("p1", x as f64, y as f64));
        let anchor = popover_anchor(&state, canvas(), &[]);
        let popover = popover_for(&state).expect("a composer is open");
        let rect = popover.rect_at(anchor, region.rect);
        assert!(
            rect.origin.x >= region.rect.origin.x
                && rect.origin.y >= region.rect.origin.y
                && rect.origin.x + rect.size.x <= region.rect.origin.x + region.rect.size.x
                && rect.origin.y + rect.size.y <= region.rect.origin.y + region.rect.size.y,
            "a click at ({x}, {y}) put the field outside the canvas: {rect:?}"
        );
        // The field a reviewer has to type into is the one that owns the
        // keyboard, so it cannot be a box they cannot see.
        assert!(state.editor_ui.comments.takes_keyboard());
    }
}

#[test]
fn a_press_with_no_painted_popover_is_not_consumed() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    assert!(!press_popover(&mut state, None, Point2D::new(100.0, 100.0)));
    assert!(state.editor_ui.comments.composer().is_some());
}

#[test]
fn a_composer_closed_between_paint_and_press_does_not_eat_the_click() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    let pins = vec![painted_pin(1, 600.0, 300.0)];
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins).unwrap();
    state.editor_ui.comments.close();
    assert!(!press_popover(
        &mut state,
        Some(rect),
        center(CommentThreadPopover::send_rect(rect))
    ));
}

#[test]
fn the_rail_paints_only_while_the_comment_tool_is_active() {
    let mut state = editor();
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    assert!(paint_panel(&mut cx, &state, rail(), "p1").is_none());
    state.editor_ui.comments.begin_mode();
    assert_eq!(paint_panel(&mut cx, &state, rail(), "p1"), Some(rail()));
}

#[test]
fn clicking_a_row_opens_the_thread_and_asks_for_the_pin_to_be_framed() {
    let mut state = editor();
    state.editor_ui.comments.begin_mode();
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint_panel(&mut cx, &state, rail(), "p1").unwrap();
    let panel = panel_for(&state, "p1").unwrap();
    let row = panel.row_rects(rail())[0];

    let action = press_panel(&mut state, Some(rail()), center(row), "p1");
    assert_eq!(
        action,
        Some(CommentsPanelAction::RevealAnchor(CommentAnchor::new(
            "p1", 360.0, 260.0
        )))
    );
    assert!(state.editor_ui.comments.is_open(1));
    // Nothing is selected: a comment is about a point, so there is no element to
    // select, and picking whatever happens to be under the point now would be a
    // different gesture.
    assert!(state.selection.is_empty());
}

#[test]
fn a_row_for_another_page_is_not_in_this_pages_list() {
    let mut state = editor();
    state.editor_ui.comments.begin_mode();
    let panel = panel_for(&state, "p1").unwrap();
    let rows = panel.row_rects(rail());
    assert_eq!(rows.len(), 1, "only p1's thread is listed");
    assert_eq!(panel.elsewhere(), 1, "p2's thread is counted instead");
}

#[test]
fn a_row_with_no_pin_opens_without_asking_for_a_camera_move() {
    // The rail lists a migrated thread; opening it is the whole answer, because
    // there is no place on the canvas it points at.
    let mut state = editor();
    let pinned = state.editor_ui.comments.threads[0].clone();
    state.editor_ui.comments.install_threads(vec![
        pinned,
        CommentThread {
            id: 77,
            anchor: None,
            comments: vec![Comment::default()],
            ..CommentThread::default()
        },
    ]);
    state.editor_ui.comments.begin_mode();
    let panel = panel_for(&state, "p1").unwrap();
    let rows = panel.row_rects(rail());
    assert_eq!(rows.len(), 2);
    let action = press_panel(&mut state, Some(rail()), center(rows[1]), "p1");
    assert_eq!(action, Some(CommentsPanelAction::Handled));
    assert!(state.editor_ui.comments.is_open(77));
}

#[test]
fn a_press_beside_the_rail_belongs_to_the_canvas() {
    let mut state = editor();
    state.editor_ui.comments.begin_mode();
    assert!(press_panel(
        &mut state,
        Some(rail()),
        Point2D::new(rail().origin.x - 60.0, center(rail()).y),
        "p1",
    )
    .is_none());
}

#[test]
fn the_rails_close_button_leaves_the_comment_tool() {
    let mut state = editor();
    state.editor_ui.comments.begin_mode();
    let close = center(crate::widgets::comments_panel::CommentsPanel::close_rect(
        rail(),
    ));
    assert_eq!(
        press_panel(&mut state, Some(rail()), close, "p1"),
        Some(CommentsPanelAction::Handled)
    );
    assert!(!state.editor_ui.comments.pin_mode);
    // And with the tool off there is no rail to press: the inspector is back.
    assert!(panel_for(&state, "p1").is_none());
}

#[test]
fn a_canvas_click_opens_a_composer_for_that_point() {
    let mut state = editor();
    state.editor_ui.comments.begin_mode();
    // What the canvas does with the click: the point comes from the shared
    // screen→document mapping, and the flow's job is only to make sure the
    // composer lands on it.
    state
        .editor_ui
        .comments
        .begin_thread_at(CommentAnchor::new("p1", 210.5, 96.0));
    assert_eq!(
        state.editor_ui.comments.composer(),
        Some(op_editor_core::editor_ui_state::CommentComposer::NewThread(
            CommentAnchor::new("p1", 210.5, 96.0)
        ))
    );
    state.editor_ui.comments.new_draft = "too tight".to_string();
    assert!(state.editor_ui.comments.send());
    assert_eq!(
        state.editor_ui.comments.take_requests().last(),
        Some(&op_editor_core::editor_ui_state::CommentRequest::Create {
            anchor: CommentAnchor::new("p1", 210.5, 96.0),
            text: "too tight".to_string(),
        })
    );
}

#[test]
fn the_pins_carried_into_the_flow_are_the_pages_own() {
    let state = editor();
    let ui = state.editor_ui.comments.clone();
    // Both threads are in the state; only the one on this page can be drawn
    // here, and only it may be numbered.
    let sources = crate::widgets::comment_pins::threads_for_page(&ui, &page_of(&state));
    assert_eq!(
        sources
            .iter()
            .map(|source: &CommentPinThread| source.thread_id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(sources[0].ordinal, 1);
}

#[test]
fn the_tool_and_the_rail_are_one_bit() {
    // Selecting the tool shows the rail and asks the daemon for the list; the
    // list is per page, and the pins on the canvas are the same page's.
    let mut state = editor();
    state.editor_ui.comments.take_requests();
    state.editor_ui.comments.toggle_pin_mode();
    assert!(state.editor_ui.comments.pin_mode);
    assert!(state.editor_ui.comments.rail_visible());
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Reload]
    );
    state.editor_ui.comments.toggle_pin_mode();
    assert!(!state.editor_ui.comments.rail_visible());
}
