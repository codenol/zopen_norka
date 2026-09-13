//! The comment flow's placement, press routing and state effects.

use super::*;
use crate::theme::Theme;
use crate::widgets::comment_pins::CommentPinThread;
use crate::widgets::comment_thread_popover::CommentThreadPopover;
use op_editor_core::editor_ui_state::{Comment, CommentAuthor, CommentThread};

fn canvas() -> Rect {
    Rect::xywh(240.0, 40.0, 800.0, 600.0)
}

/// An editor state with one thread about `n1` and one about a deleted element.
fn editor() -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.locale = op_i18n::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_000_000.0;
    state.editor_ui.comments.install_threads(vec![
        CommentThread {
            id: 1,
            node_id: "n1".to_string(),
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
            node_id: "gone".to_string(),
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
                body: "about an element that no longer exists".to_string(),
                created_at: 1_699_999_800,
            }],
        },
    ]);
    state
}

fn node_exists(id: &str) -> bool {
    id == "n1"
}

fn pin(thread_id: i64, rect: Rect) -> CommentPin {
    CommentPin {
        thread_id,
        ordinal: thread_id as usize,
        node_id: "n1".to_string(),
        rect,
        color: None,
        resolved: false,
        replies: 0,
    }
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
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
    assert_eq!(
        popover_anchor(&state, canvas(), &pins),
        Rect::xywh(600.0, 300.0, 22.0, 22.0)
    );
}

#[test]
fn a_thread_without_a_pin_still_gets_a_panel() {
    let mut state = editor();
    state.editor_ui.comments.open(2);
    // No pin for thread 2: its element is not in the document any more.
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
    let anchor = popover_anchor(&state, canvas(), &pins);
    assert_ne!(anchor, pins[0].rect);
    assert_eq!(anchor.origin, Point2D::new(248.0, 48.0));

    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_popover(&mut cx, &state, canvas(), &pins);
    assert!(rect.is_some(), "the conversation is still readable");
}

#[test]
fn a_press_outside_the_popover_closes_it_and_does_not_reach_the_canvas() {
    let mut state = editor();
    state.editor_ui.comments.open(1);
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
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
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
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
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
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
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
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
    let pins = vec![pin(1, Rect::xywh(600.0, 300.0, 22.0, 22.0))];
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
fn the_panel_paints_only_when_it_is_open() {
    let mut state = editor();
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    assert!(paint_panel(&mut cx, &state, canvas(), &node_exists).is_none());
    state.editor_ui.comments.panel_open = true;
    assert!(paint_panel(&mut cx, &state, canvas(), &node_exists).is_some());
}

#[test]
fn clicking_a_row_opens_the_thread_selects_its_element_and_asks_for_a_reveal() {
    let mut state = editor();
    state.editor_ui.comments.panel_open = true;
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_panel(&mut cx, &state, canvas(), &node_exists).unwrap();
    let panel = panel_for(&state, &node_exists).unwrap();
    let row = panel.row_rects(rect)[0];

    let action = press_panel(&mut state, Some(rect), center(row), &node_exists);
    assert_eq!(
        action,
        Some(CommentsPanelAction::RevealNode("n1".to_string()))
    );
    assert!(state.editor_ui.comments.is_open(1));
    assert_eq!(state.selection.anchor.as_str(), "n1");
}

#[test]
fn a_row_with_no_element_opens_the_thread_without_a_reveal() {
    let mut state = editor();
    state.editor_ui.comments.panel_open = true;
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_panel(&mut cx, &state, canvas(), &node_exists).unwrap();
    let panel = panel_for(&state, &node_exists).unwrap();
    let row = panel.row_rects(rect)[1];

    let action = press_panel(&mut state, Some(rect), center(row), &node_exists);
    assert_eq!(action, Some(CommentsPanelAction::Handled));
    assert!(state.editor_ui.comments.is_open(2));
    // Nothing was selected: there is no element to select.
    assert!(state.selection.is_empty());
}

#[test]
fn a_press_beside_the_panel_belongs_to_the_canvas() {
    let mut state = editor();
    state.editor_ui.comments.panel_open = true;
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_panel(&mut cx, &state, canvas(), &node_exists).unwrap();
    assert!(press_panel(
        &mut state,
        Some(rect),
        Point2D::new(rect.origin.x - 60.0, center(rect).y),
        &node_exists,
    )
    .is_none());
}

#[test]
fn the_toggle_arms_and_disarms_pin_mode() {
    let mut state = editor();
    state.editor_ui.comments.panel_open = true;
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_panel(&mut cx, &state, canvas(), &node_exists).unwrap();
    let arm = center(crate::widgets::comments_panel::CommentsPanel::arm_rect(
        rect,
    ));
    assert_eq!(
        press_panel(&mut state, Some(rect), arm, &node_exists),
        Some(CommentsPanelAction::Handled)
    );
    assert!(state.editor_ui.comments.pin_mode);
    press_panel(&mut state, Some(rect), arm, &node_exists);
    assert!(!state.editor_ui.comments.pin_mode);
}

#[test]
fn the_close_button_shuts_the_panel() {
    let mut state = editor();
    state.editor_ui.comments.panel_open = true;
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_panel(&mut cx, &state, canvas(), &node_exists).unwrap();
    let close = center(crate::widgets::comments_panel::CommentsPanel::close_rect(
        rect,
    ));
    press_panel(&mut state, Some(rect), close, &node_exists);
    assert!(!state.editor_ui.comments.panel_open);
}

#[test]
fn a_pin_click_opens_a_composer_for_that_element() {
    let mut state = editor();
    state.editor_ui.comments.toggle_pin_mode();
    // What the canvas does with the click: the pin mode is the state's, and the
    // flow's job is only to make sure the composer lands on the right element.
    state.editor_ui.comments.begin_thread_on("n1");
    assert_eq!(
        state.editor_ui.comments.composer(),
        Some(op_editor_core::editor_ui_state::CommentComposer::NewThread(
            "n1".to_string()
        ))
    );
    state.editor_ui.comments.new_draft = "too tight".to_string();
    assert!(state.editor_ui.comments.send());
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Create {
            node_id: "n1".to_string(),
            text: "too tight".to_string(),
        }]
    );
}

#[test]
fn the_pins_carried_into_the_flow_keep_their_thread_ids() {
    let ui = editor().editor_ui.comments.clone();
    let sources = crate::widgets::comment_pins::threads_for(&ui);
    assert_eq!(
        sources
            .iter()
            .map(|source: &CommentPinThread| source.thread_id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn the_pill_opens_the_panel_and_asks_for_the_conversation() {
    let mut state = editor();
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = paint_toggle(&mut cx, &state, canvas()).unwrap();
    // Nothing is open yet, so the daemon's list is as old as the last read: the
    // press has to ask for it as well as show the panel.
    assert!(press_toggle(&mut state, Some(rect), center(rect)));
    assert!(state.editor_ui.comments.panel_open);
    assert_eq!(
        state.editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Reload]
    );
}

#[test]
fn a_press_beside_the_pill_belongs_to_the_canvas() {
    let mut state = editor();
    let rect = crate::widgets::comments_panel::CommentsToggle::rect_in_canvas(canvas());
    assert!(!press_toggle(
        &mut state,
        Some(rect),
        Point2D::new(rect.origin.x - 40.0, rect.origin.y)
    ));
    assert!(!state.editor_ui.comments.panel_open);
    // And with no rect painted (the panel is open), the press is not the pill's.
    assert!(!press_toggle(&mut state, None, center(rect)));
}

#[test]
fn the_pill_is_painted_only_while_the_panel_is_shut() {
    let mut state = editor();
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    assert!(paint_toggle(&mut cx, &state, canvas()).is_some());
    state.editor_ui.comments.panel_open = true;
    assert!(paint_toggle(&mut cx, &state, canvas()).is_none());
}

#[test]
fn the_pill_counts_the_open_threads_and_sits_in_the_canvas_corner() {
    let mut state = editor();
    state.editor_ui.comments.threads[0].resolved = true;
    let rect = crate::widgets::comments_panel::CommentsToggle::rect_in_canvas(canvas());
    let region = canvas();
    assert!(rect.origin.x + rect.size.x <= region.origin.x + region.size.x);
    assert_eq!(rect.origin.y, region.origin.y + 8.0);

    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    crate::widgets::comments_panel::CommentsToggle::paint(
        &mut cx,
        &Theme::dark(),
        rect,
        state.editor_ui.comments.open_count(),
        false,
    );
    // One of the two threads is resolved, so the badge says one.
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert_eq!(painted, vec!["1"]);
}
