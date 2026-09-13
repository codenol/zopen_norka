//! List panel geometry, rows and paint.
//!
//! What a reviewer can be wrong about here: clicking the row they aimed at,
//! losing a thread because its element is gone, and the panel calling an
//! element pinned when there is nothing on the canvas to jump to.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::editor_ui_state::{Comment, CommentAuthor, CommentThread, CommentsUiState};

fn canvas() -> Rect {
    Rect::xywh(240.0, 40.0, 800.0, 600.0)
}

fn thread(
    id: i64,
    node: &str,
    name: &str,
    role: Option<&str>,
    body: &str,
    resolved: bool,
) -> CommentThread {
    CommentThread {
        id,
        node_id: node.to_string(),
        created_at: 1_699_999_700,
        resolved,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![Comment {
            id: id * 10,
            author: CommentAuthor {
                id: Some(format!("u{id}")),
                name: name.to_string(),
                role: role.map(str::to_string),
            },
            body: body.to_string(),
            created_at: 1_699_999_700,
        }],
    }
}

fn panel(ui: &CommentsUiState, now: f64, node_exists: impl Fn(&str) -> bool) -> CommentsPanel {
    CommentsPanel::new(
        Theme::dark(),
        Locale::EnUs,
        rows(ui, Locale::EnUs, None, node_exists),
        ui.pin_mode,
        ui.loading,
        ui.error.clone(),
        now,
    )
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn a_thread_without_its_element_is_a_row_and_not_a_pin() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(
            1,
            "n1",
            "Kay",
            Some("ux_ui"),
            "too close to the edge",
            false,
        ),
        thread(2, "gone", "Ada", None, "this one lost its element", false),
    ]);
    let listed = rows(&ui, Locale::EnUs, None, |id| id == "n1");
    assert_eq!(listed.len(), 2);
    assert!(listed[0].pinned);
    // The thread is still listed — that is the whole point — and says so.
    assert!(!listed[1].pinned);
    assert_eq!(listed[1].author, "Ada");
}

#[test]
fn a_row_carries_the_author_the_first_line_the_count_and_the_state() {
    let mut ui = CommentsUiState::default();
    let mut with_replies = thread(
        1,
        "n1",
        "Kay",
        Some("admin"),
        "first line\nsecond line",
        true,
    );
    with_replies.comments.push(Comment {
        id: 99,
        author: CommentAuthor {
            id: Some("u2".to_string()),
            name: "Ada".to_string(),
            role: None,
        },
        body: "an answer".to_string(),
        created_at: 1_699_999_800,
    });
    ui.install_threads(vec![with_replies]);
    let listed = rows(&ui, Locale::EnUs, None, |_| true);
    assert_eq!(listed[0].ordinal, 1);
    assert_eq!(listed[0].author, "Kay");
    // One line, because a row is one line tall.
    assert_eq!(listed[0].excerpt, "first line second line");
    assert_eq!(listed[0].reply_count, 1);
    assert!(listed[0].resolved);
    assert_eq!(listed[0].color, crate::util::parse_hex_color("#E0A800"));
}

#[test]
fn the_viewers_own_comment_is_named_as_you() {
    let mut ui = CommentsUiState::default();
    ui.set_viewer_id(Some("u1".to_string()));
    ui.install_threads(vec![thread(1, "n1", "Kay", None, "mine", false)]);
    let listed = rows(&ui, Locale::EnUs, ui.viewer_id.as_deref(), |_| true);
    assert_eq!(listed[0].author, "You");
    // And with no id of our own it is the name the server recorded.
    let anonymous = rows(&ui, Locale::EnUs, None, |_| true);
    assert_eq!(anonymous[0].author, "Kay");
}

#[test]
fn a_local_operator_comment_is_named_as_one() {
    let mut ui = CommentsUiState::default();
    let mut local = thread(1, "n1", "", None, "no account behind this", false);
    local.comments[0].author.id = None;
    ui.install_threads(vec![local]);
    let listed = rows(&ui, Locale::EnUs, None, |_| true);
    assert_eq!(listed[0].author, "Local operator");
}

#[test]
fn a_thread_with_no_comments_still_has_a_row() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![CommentThread {
        id: 3,
        node_id: "n3".to_string(),
        comments: vec![],
        ..CommentThread::default()
    }]);
    let listed = rows(&ui, Locale::EnUs, None, |_| true);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].excerpt, "");
    assert_eq!(listed[0].author, "Unknown");
    assert_eq!(listed[0].reply_count, 0);
}

#[test]
fn a_press_lands_on_the_row_it_aimed_at() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "n1", "Kay", None, "one", false),
        thread(2, "n2", "Ada", None, "two", false),
        thread(3, "n3", "Cy", None, "three", false),
    ]);
    let list = panel(&ui, 0.0, |_| true);
    let rect = list.rect_in_canvas(canvas());
    let row_rects = list.row_rects(rect);
    assert_eq!(row_rects.len(), 3);
    for (index, row) in row_rects.iter().enumerate() {
        let id = index as i64 + 1;
        assert_eq!(list.hit_test(rect, center(*row)), CommentsPanelHit::Row(id));
    }
}

#[test]
fn a_press_on_the_panel_edges_does_nothing_to_the_rows() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![thread(1, "n1", "Kay", None, "one", false)]);
    let list = panel(&ui, 0.0, |_| true);
    let rect = list.rect_in_canvas(canvas());
    assert_eq!(
        list.hit_test(rect, center(CommentsPanel::close_rect(rect))),
        CommentsPanelHit::Close
    );
    assert_eq!(
        list.hit_test(rect, center(CommentsPanel::arm_rect(rect))),
        CommentsPanelHit::ArmPin
    );
    assert_eq!(
        list.hit_test(
            rect,
            Point2D::new(rect.origin.x - 40.0, rect.origin.y + 200.0)
        ),
        CommentsPanelHit::Outside
    );
}

#[test]
fn rows_never_overlap_the_toggle_above_them() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![thread(1, "n1", "Kay", None, "one", false)]);
    let list = panel(&ui, 0.0, |_| true);
    let rect = list.rect_in_canvas(canvas());
    let arm = CommentsPanel::arm_rect(rect);
    let first = list.row_rects(rect)[0];
    assert!(first.origin.y >= arm.origin.y + arm.size.y);
}

#[test]
fn the_panel_sits_against_the_canvas_right_edge() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![thread(1, "n1", "Kay", None, "one", false)]);
    let list = panel(&ui, 0.0, |_| true);
    let region = canvas();
    let rect = list.rect_in_canvas(region);
    assert!(rect.origin.x + rect.size.x <= region.origin.x + region.size.x);
    assert!(rect.origin.x > region.origin.x + region.size.x / 2.0);
    assert!(rect.origin.y >= region.origin.y);
}

#[test]
fn a_long_list_is_capped_and_counted() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(
        (1..=20)
            .map(|id| thread(id, "n1", "Kay", None, "body", false))
            .collect(),
    );
    let list = panel(&ui, 0.0, |_| true);
    assert_eq!(list.visible_rows().len(), MAX_ROWS);
    assert_eq!(list.hidden_rows(), 20 - MAX_ROWS);
    // The height is what the rows cost, not what the list holds.
    assert_eq!(
        list.height(),
        HEADER_H + ARM_H + MAX_ROWS as f32 * ROW_H + PAD
    );
}

#[test]
fn an_empty_list_paints_its_own_reason() {
    let ui = CommentsUiState::default();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let list = panel(&ui, 0.0, |_| true);
    list.paint(&mut cx, list.rect_in_canvas(canvas()));
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Comments"), "{painted:?}");
    assert!(
        painted.iter().any(|text| text.contains("No comments")),
        "{painted:?}"
    );
    assert!(painted.contains(&"Comment on an element"), "{painted:?}");
}

#[test]
fn a_loading_and_a_failed_list_say_which_they_are() {
    let mut loading = CommentsUiState::default();
    loading.set_loading();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let list = panel(&loading, 0.0, |_| true);
    list.paint(&mut cx, list.rect_in_canvas(canvas()));
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, _)| text.contains("Loading comments")),
        "{:?}",
        backend.texts
    );

    let mut failed = CommentsUiState::default();
    failed.set_error("comments.error.transport");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let list = panel(&failed, 0.0, |_| true);
    list.paint(&mut cx, list.rect_in_canvas(canvas()));
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, _)| text.contains("could not be loaded")),
        "{:?}",
        backend.texts
    );
}

#[test]
fn a_row_paints_its_marks_and_the_missing_pin() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "n1", "Kay", None, "one", false),
        thread(2, "gone", "Ada", None, "two", true),
    ]);
    let list = panel(&ui, 1_700_000_000_000.0, |id| id == "n1");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let rect = list.rect_in_canvas(canvas());
    list.paint(&mut cx, rect);
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Kay"), "{painted:?}");
    assert!(painted.contains(&"Ada"), "{painted:?}");
    assert!(painted.contains(&"Open"), "{painted:?}");
    assert!(painted.contains(&"Resolved"), "{painted:?}");
    assert!(painted.contains(&"No pin"), "{painted:?}");
    assert!(painted.contains(&"5m ago"), "{painted:?}");
}

#[test]
fn the_armed_toggle_says_what_it_will_do() {
    let mut ui = CommentsUiState::default();
    ui.toggle_pin_mode();
    let list = panel(&ui, 0.0, |_| true);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    list.paint(&mut cx, list.rect_in_canvas(canvas()));
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, _)| text.contains("Click an element")),
        "{:?}",
        backend.texts
    );
}

#[test]
fn the_list_is_localized() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![thread(1, "n1", "Кай", None, "текст", false)]);
    let panel = CommentsPanel::new(
        Theme::dark(),
        Locale::Ru,
        rows(&ui, Locale::Ru, None, |_| true),
        false,
        false,
        None,
        0.0,
    );
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    panel.paint(&mut cx, panel.rect_in_canvas(canvas()));
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Комментарии"), "{painted:?}");
    assert!(painted.contains(&"Открыт"), "{painted:?}");
    assert!(painted.contains(&"Комментарий к элементу"), "{painted:?}");
}
