//! The rail's thread list: rows, page scoping, geometry and paint.
//!
//! What a reviewer can be wrong about here: clicking the row they aimed at,
//! reading a page's list as the whole document's, and the rail's occupant being
//! ambiguous between the inspector and this.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::editor_ui_state::{
    Comment, CommentAnchor, CommentAuthor, CommentThread, CommentsUiState,
};

/// The rail's own rect — the same slot the property panel occupies.
fn rail() -> Rect {
    Rect::xywh(1040.0, 40.0, 240.0, 600.0)
}

fn thread(
    id: i64,
    page: &str,
    name: &str,
    role: Option<&str>,
    body: &str,
    resolved: bool,
) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, id as f64 * 30.0, id as f64 * 40.0)),
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

fn panel(ui: &CommentsUiState, now: f64, page: &str) -> CommentsPanel {
    CommentsPanel::new(
        Theme::dark(),
        Locale::EnUs,
        rows(ui, Locale::EnUs, None, page),
        ui.loading,
        ui.error.clone(),
        now,
        ui.open_count_elsewhere(page),
    )
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn the_list_holds_one_pages_threads_and_numbers_them_from_one() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "one", false),
        thread(2, "p2", "Ada", None, "other page", false),
        thread(3, "p1", "Cy", None, "three", false),
    ]);
    let listed = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed.iter().map(|row| row.thread_id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    // The ordinals are this page's, matching the pins drawn on it.
    assert_eq!(listed[0].ordinal, Some(1));
    assert_eq!(listed[1].ordinal, Some(2));
}

#[test]
fn the_rest_of_the_review_is_counted_rather_than_listed() {
    let mut ui = CommentsUiState::default();
    let mut closed = thread(4, "p2", "Ada", None, "done", true);
    closed.resolved = true;
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "here", false),
        thread(2, "p2", "Ada", None, "elsewhere", false),
        closed,
    ]);
    let list = panel(&ui, 0.0, "p1");
    // Only p2's OPEN thread counts: a resolved one is not outstanding work.
    assert_eq!(list.elsewhere(), 1);
}

#[test]
fn a_row_carries_the_author_the_first_line_the_count_and_the_state() {
    let mut ui = CommentsUiState::default();
    let mut with_replies = thread(
        1,
        "p1",
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
    let listed = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(listed[0].ordinal, Some(1));
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
    ui.install_threads(vec![thread(1, "p1", "Kay", None, "mine", false)]);
    let listed = rows(&ui, Locale::EnUs, ui.viewer_id.as_deref(), "p1");
    assert_eq!(listed[0].author, "You");
    // And with no id of our own it is the name the server recorded.
    let anonymous = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(anonymous[0].author, "Kay");
}

#[test]
fn a_local_operator_comment_is_named_as_one() {
    let mut ui = CommentsUiState::default();
    let mut local = thread(1, "p1", "", None, "no account behind this", false);
    local.comments[0].author.id = None;
    ui.install_threads(vec![local]);
    let listed = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(listed[0].author, "Local operator");
}

#[test]
fn a_thread_with_no_comments_still_has_a_row() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![CommentThread {
        id: 3,
        anchor: Some(CommentAnchor::new("p1", 10.0, 20.0)),
        comments: vec![],
        ..CommentThread::default()
    }]);
    let listed = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].excerpt, "");
    assert_eq!(listed[0].author, "Unknown");
    assert_eq!(listed[0].reply_count, 0);
}

#[test]
fn a_thread_with_no_pin_is_listed_marked_and_unumbered() {
    // A thread the daemon migrated from the element-keyed format has no page and
    // no coordinates. It is not hidden — a conversation nobody can find is worse
    // than one without a marker — and it is not given a number, because there is
    // no marker carrying that number.
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "on this page", false),
        CommentThread {
            id: 2,
            anchor: None,
            comments: vec![Comment {
                id: 20,
                author: CommentAuthor {
                    id: Some("u2".to_string()),
                    name: "Ada".to_string(),
                    role: None,
                },
                body: "migrated from the old format".to_string(),
                created_at: 1_699_999_800,
            }],
            ..CommentThread::default()
        },
    ]);
    let listed = rows(&ui, Locale::EnUs, None, "p1");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].ordinal, Some(1));
    assert_eq!(listed[1].ordinal, None, "no pin, no number");
    assert_eq!(listed[1].author, "Ada");

    // And the row says so, rather than reading as a pin somebody cannot see.
    let list = panel(&ui, 0.0, "p1");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    list.paint(&mut cx, rail());
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Pin 1"), "{painted:?}");
    assert!(painted.contains(&"No pin"), "{painted:?}");
    assert!(!painted.contains(&"Pin 2"), "{painted:?}");
}

#[test]
fn a_pin_less_thread_belongs_to_the_page_being_shown() {
    // It is in the list of whatever page the reviewer is on, and in the count
    // that list opens: otherwise a migrated conversation would be invisible
    // everywhere, and only a reload would ever change the badge.
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "here", false),
        CommentThread {
            id: 2,
            anchor: None,
            ..CommentThread::default()
        },
        thread(3, "p2", "Ada", None, "elsewhere", false),
    ]);
    assert_eq!(ui.threads_on_page("p1").len(), 2);
    assert_eq!(ui.threads_on_page("p2").len(), 2);
    assert_eq!(ui.open_count_on_page("p1"), 2);
    // "Elsewhere" means another page's pin, not a thread with no page at all.
    assert_eq!(ui.open_count_elsewhere("p1"), 1);
    assert_eq!(ui.open_count_elsewhere("p2"), 1);
}

#[test]
fn a_press_lands_on_the_row_it_aimed_at() {
    let ui = three_threads();
    let list = panel(&ui, 0.0, "p1");
    let rect = rail();
    let row_rects = list.row_rects(rect);
    assert_eq!(row_rects.len(), 3);
    for (index, row) in row_rects.iter().enumerate() {
        let id = index as i64 + 1;
        assert_eq!(list.hit_test(rect, center(*row)), CommentsPanelHit::Row(id));
    }
}

fn three_threads() -> CommentsUiState {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "one", false),
        thread(2, "p1", "Ada", None, "two", false),
        thread(3, "p1", "Cy", None, "three", false),
    ]);
    ui
}

#[test]
fn a_press_on_the_panel_edges_does_nothing_to_the_rows() {
    let ui = three_threads();
    let list = panel(&ui, 0.0, "p1");
    let rect = rail();
    assert_eq!(
        list.hit_test(rect, center(CommentsPanel::close_rect(rect))),
        CommentsPanelHit::Close
    );
    // The hint line is not a control: it says what the tool does, and a click on
    // it is a click on the panel.
    assert_eq!(
        list.hit_test(rect, center(CommentsPanel::hint_rect(rect))),
        CommentsPanelHit::Inside
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
fn rows_never_overlap_the_hint_above_them() {
    let ui = three_threads();
    let list = panel(&ui, 0.0, "p1");
    let rect = rail();
    let hint = CommentsPanel::hint_rect(rect);
    let first = list.row_rects(rect)[0];
    assert!(first.origin.y >= hint.origin.y + hint.size.y);
}

#[test]
fn the_list_is_painted_inside_the_rect_the_rail_gave_it() {
    // The rail's rect is the inspector's slot: the list paints inside it and
    // never outside, because the canvas is right next to it.
    let ui = three_threads();
    let list = panel(&ui, 0.0, "p1");
    let rect = rail();
    for row in list.row_rects(rect) {
        assert!(row.origin.x >= rect.origin.x);
        assert!(row.origin.x + row.size.x <= rect.origin.x + rect.size.x);
        assert!(row.origin.y + row.size.y <= rect.origin.y + rect.size.y);
    }
}

#[test]
fn a_long_list_is_capped_and_counted() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(
        (1..=20)
            .map(|id| thread(id, "p1", "Kay", None, "body", false))
            .collect(),
    );
    let list = panel(&ui, 0.0, "p1");
    // A rail tall enough for the cap: past a screenful the answer is a filter
    // rather than a scrollbar.
    let tall = Rect::xywh(
        1040.0,
        40.0,
        240.0,
        40.0 + 26.0 + 12.0 + MAX_ROWS as f32 * ROW_H,
    );
    assert_eq!(list.row_rects(tall).len(), MAX_ROWS);
    assert_eq!(list.hidden_rows(tall), 20 - MAX_ROWS);
    // A shorter rail clips instead: the rows it has no room for are counted,
    // not painted over the panel's own edge.
    let room = ((rail().size.y - HEADER_H - HINT_H - PAD) / ROW_H).floor() as usize;
    assert!(room < MAX_ROWS, "the fixture rail is the shorter case");
    assert_eq!(list.row_rects(rail()).len(), room);
    assert_eq!(list.hidden_rows(rail()), 20 - room);
}

#[test]
fn a_list_with_no_room_paints_no_rows_rather_than_out_of_the_panel() {
    let ui = three_threads();
    let list = panel(&ui, 0.0, "p1");
    // A rail shorter than its own header: nothing fits, so nothing is claimed —
    // hit-test and paint read the same answer.
    let cramped = Rect::xywh(1040.0, 40.0, 240.0, 50.0);
    assert!(list.row_rects(cramped).is_empty());
    assert_eq!(
        list.hit_test(cramped, Point2D::new(1100.0, 60.0)),
        CommentsPanelHit::Inside
    );
}

#[test]
fn an_empty_list_paints_its_own_reason() {
    let ui = CommentsUiState::default();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let list = panel(&ui, 0.0, "p1");
    list.paint(&mut cx, rail());
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
    assert!(
        painted.iter().any(|text| text.contains("Click the canvas")),
        "{painted:?}"
    );
}

#[test]
fn a_loading_and_a_failed_list_say_which_they_are() {
    let mut loading = CommentsUiState::default();
    loading.set_loading();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let list = panel(&loading, 0.0, "p1");
    list.paint(&mut cx, rail());
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
    let list = panel(&failed, 0.0, "p1");
    list.paint(&mut cx, rail());
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
fn a_row_paints_its_pin_number_its_state_and_its_replies() {
    let mut ui = CommentsUiState::default();
    let mut closed = thread(2, "p1", "Ada", None, "two", true);
    closed.comments.push(Comment {
        id: 22,
        author: CommentAuthor {
            id: Some("u2".to_string()),
            name: "Ada".to_string(),
            role: None,
        },
        body: "an answer".to_string(),
        created_at: 1_699_999_900,
    });
    ui.install_threads(vec![thread(1, "p1", "Kay", None, "one", false), closed]);
    let list = panel(&ui, 1_700_000_000_000.0, "p1");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    list.paint(&mut cx, rail());
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Kay"), "{painted:?}");
    assert!(painted.contains(&"Ada"), "{painted:?}");
    assert!(painted.contains(&"Open"), "{painted:?}");
    assert!(painted.contains(&"Resolved"), "{painted:?}");
    // The number a row carries is the number its pin shows.
    assert!(painted.contains(&"Pin 1"), "{painted:?}");
    assert!(painted.contains(&"Pin 2"), "{painted:?}");
    assert!(painted.contains(&"1 replies"), "{painted:?}");
    assert!(painted.contains(&"5m ago"), "{painted:?}");
}

#[test]
fn the_other_pages_are_named_at_the_foot_of_the_list() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        thread(1, "p1", "Kay", None, "one", false),
        thread(2, "p2", "Ada", None, "elsewhere", false),
    ]);
    let list = panel(&ui, 0.0, "p1");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    list.paint(&mut cx, rail());
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"1 on other pages"), "{painted:?}");
}

#[test]
fn the_hint_says_what_the_active_tool_does_with_a_click() {
    let ui = CommentsUiState::default();
    let list = panel(&ui, 0.0, "p1");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    list.paint(&mut cx, rail());
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, _)| text.contains("Click the canvas")),
        "{:?}",
        backend.texts
    );
}

#[test]
fn the_list_is_localized() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![thread(1, "p1", "Кай", None, "текст", false)]);
    let panel = CommentsPanel::new(
        Theme::dark(),
        Locale::Ru,
        rows(&ui, Locale::Ru, None, "p1"),
        false,
        None,
        0.0,
        0,
    );
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    panel.paint(&mut cx, rail());
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Комментарии"), "{painted:?}");
    assert!(painted.contains(&"Открыт"), "{painted:?}");
    assert!(painted.contains(&"Пин 1"), "{painted:?}");
}
