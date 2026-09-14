//! Popover geometry, hit-testing and paint.
//!
//! The properties being held here are the ones a reviewer would notice: the
//! buttons are where a click finds them, the panel stays inside the canvas, a
//! long comment never paints over the field below it, and an outside press is
//! the press that closes it.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::editor_ui_state::{
    Comment, CommentAnchor, CommentAuthor, CommentThread, CommentsUiState,
};

fn canvas() -> Rect {
    Rect::xywh(240.0, 40.0, 800.0, 600.0)
}

fn comment(id: i64, name: &str, role: Option<&str>, body: &str, at: u64) -> Comment {
    Comment {
        id,
        author: CommentAuthor {
            id: Some(format!("u{id}")),
            name: name.to_string(),
            role: role.map(str::to_string),
        },
        body: body.to_string(),
        created_at: at,
    }
}

fn thread_ui(comments: Vec<Comment>) -> CommentsUiState {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![CommentThread {
        id: 1,
        anchor: Some(CommentAnchor::new("p1", 120.0, 80.0)),
        created_at: 1_700_000_000,
        resolved: false,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments,
    }]);
    ui.open(1);
    ui
}

fn popover(ui: &CommentsUiState, now: f64) -> CommentThreadPopover {
    let model =
        CommentPopoverModel::for_comments(ui, Locale::EnUs, None, ui.composer_focused).unwrap();
    CommentThreadPopover::new(model, Theme::dark(), now)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn a_thread_with_no_composer_has_no_model() {
    let ui = CommentsUiState::default();
    assert!(CommentPopoverModel::for_comments(&ui, Locale::EnUs, None, false).is_none());
}

#[test]
fn the_buttons_are_where_a_click_finds_them() {
    let ui = thread_ui(vec![comment(
        10,
        "Kay",
        Some("ux_ui"),
        "hello",
        1_700_000_000,
    )]);
    let popover = popover(&ui, 1_700_000_600_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());

    assert_eq!(
        popover.hit_test(rect, center(CommentThreadPopover::close_rect(rect))),
        CommentPopoverHit::Close
    );
    assert_eq!(
        popover.hit_test(rect, center(CommentThreadPopover::send_rect(rect))),
        CommentPopoverHit::Send
    );
    assert_eq!(
        popover.hit_test(rect, center(CommentThreadPopover::input_rect(rect))),
        CommentPopoverHit::FocusInput
    );
    assert_eq!(
        popover.hit_test(rect, center(CommentThreadPopover::resolution_rect(rect))),
        CommentPopoverHit::Resolution
    );
}

#[test]
fn the_three_controls_never_overlap_each_other() {
    let ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    let popover = popover(&ui, 1_700_000_600_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    let input = CommentThreadPopover::input_rect(rect);
    let send = CommentThreadPopover::send_rect(rect);
    let resolution = CommentThreadPopover::resolution_rect(rect);
    // Disjoint, so a click has exactly one meaning.
    assert!(input.origin.x + input.size.x <= send.origin.x);
    // The field sits above the action row, not on top of it.
    assert!(input.origin.y + input.size.y <= resolution.origin.y);
    assert_eq!(
        popover.hit_test(rect, center(resolution)),
        CommentPopoverHit::Resolution
    );
    assert_eq!(
        popover.hit_test(rect, center(send)),
        CommentPopoverHit::Send
    );
}

#[test]
fn one_point_inside_the_panel_yields_one_answer() {
    let ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    let popover = popover(&ui, 1_700_000_600_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    // The body area, which is no control at all.
    let body = Point2D::new(rect.origin.x + 20.0, rect.origin.y + 70.0);
    assert_eq!(popover.hit_test(rect, body), CommentPopoverHit::Inside);
    assert_eq!(
        popover.hit_test(rect, Point2D::new(rect.origin.x - 20.0, rect.origin.y)),
        CommentPopoverHit::Outside
    );
}

#[test]
fn a_pin_near_the_right_edge_flips_the_panel_to_its_other_side() {
    let ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    let popover = popover(&ui, 1_700_000_600_000.0);
    let region = canvas();
    let near_edge = Rect::xywh(
        region.origin.x + region.size.x - 30.0,
        region.origin.y + 100.0,
        22.0,
        22.0,
    );
    let rect = popover.rect_at(near_edge, region);
    assert!(
        rect.origin.x + rect.size.x <= near_edge.origin.x,
        "the panel must not cover the marker it was opened from"
    );
    assert!(rect.origin.x >= region.origin.x);
}

#[test]
fn the_panel_stays_inside_the_canvas_however_it_is_anchored() {
    let ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    let popover = popover(&ui, 1_700_000_600_000.0);
    let region = canvas();
    for anchor in [
        Rect::xywh(-500.0, -500.0, 22.0, 22.0),
        Rect::xywh(9_000.0, 9_000.0, 22.0, 22.0),
        Rect::xywh(region.origin.x, region.origin.y, 22.0, 22.0),
    ] {
        let rect = popover.rect_at(anchor, region);
        assert!(rect.origin.x >= region.origin.x);
        assert!(rect.origin.y >= region.origin.y);
        assert!(rect.origin.x + rect.size.x <= region.origin.x + region.size.x);
        assert!(rect.origin.y + rect.size.y <= region.origin.y + region.size.y);
    }
}

#[test]
fn a_longer_conversation_is_a_taller_panel() {
    let one = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    let three = thread_ui(vec![
        comment(10, "Kay", None, "hello", 1_700_000_000),
        comment(11, "Ada", None, "one more", 1_700_000_100),
        comment(12, "Cy", None, "and another", 1_700_000_200),
    ]);
    assert!(popover(&three, 0.0).height() > popover(&one, 0.0).height());
}

#[test]
fn a_body_is_budgeted_a_bounded_number_of_lines() {
    assert_eq!(budget_lines("", MAX_BODY_LINES), 1);
    assert_eq!(budget_lines("short", MAX_BODY_LINES), 1);
    assert_eq!(
        budget_lines(&"a".repeat(CHARS_PER_LINE + 1), MAX_BODY_LINES),
        2
    );
    // However long it is, the panel's height was computed from this ceiling —
    // so a novel cannot paint over the field below it.
    assert_eq!(
        budget_lines(&"a".repeat(100_000), MAX_BODY_LINES),
        MAX_BODY_LINES
    );
}

#[test]
fn a_hundred_paragraphs_do_not_grow_the_panel_without_bound() {
    let ui = thread_ui(vec![comment(
        10,
        "Kay",
        None,
        &"word ".repeat(4_000),
        1_700_000_000,
    )]);
    let popover = popover(&ui, 0.0);
    let region = canvas();
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), region);
    assert!(rect.size.y <= region.size.y);
}

#[test]
fn extra_replies_are_counted_rather_than_shown() {
    let mut comments = vec![comment(10, "Kay", None, "opening", 1_700_000_000)];
    for index in 0..6 {
        comments.push(comment(
            11 + index,
            "Ada",
            None,
            "reply",
            1_700_000_100 + index as u64,
        ));
    }
    let ui = thread_ui(comments);
    let model = CommentPopoverModel::for_comments(&ui, Locale::EnUs, None, false).unwrap();
    assert_eq!(model.hidden_replies(), 6 - MAX_VISIBLE_REPLIES);
    // And the panel reserves a line to say so.
    let popover = popover(&ui, 0.0);
    assert!(popover.height() > 200.0);
}

#[test]
fn the_closed_state_changes_the_action_the_panel_offers() {
    let mut ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    assert!(!popover(&ui, 0.0).model().resolved());
    ui.threads[0].resolved = true;
    ui.threads[0].resolved_by_name = Some("Ada".to_string());
    let closed = popover(&ui, 0.0);
    assert!(closed.model().resolved());
    // A closed thread is taller: the banner naming who closed it is part of it.
    ui.threads[0].resolved = false;
    assert!(closed.height() > popover(&ui, 0.0).height());
}

#[test]
fn sending_needs_text_and_stays_within_the_servers_ceiling() {
    let mut ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    assert!(!popover(&ui, 0.0).model().can_send());
    ui.reply_draft = "  ".to_string();
    assert!(!popover(&ui, 0.0).model().can_send());
    ui.reply_draft = "a reply".to_string();
    assert!(popover(&ui, 0.0).model().can_send());
    ui.reply_draft = "я".repeat(op_editor_core::editor_ui_state::MAX_COMMENT_CHARS + 1);
    assert!(!popover(&ui, 0.0).model().can_send());
}

#[test]
fn a_thread_being_written_has_no_resolve_button_and_its_own_placeholder() {
    let mut ui = CommentsUiState::default();
    ui.transport = true;
    ui.toggle_pin_mode();
    ui.begin_thread_at(CommentAnchor::new("p1", 400.0, 300.0));
    let model = CommentPopoverModel::for_comments(&ui, Locale::EnUs, None, true).unwrap();
    assert!(model.thread_id().is_none());
    assert!(!model.reply_placeholder);
    // Nothing typed yet, so the field offers its placeholder rather than a send.
    assert!(!model.can_send());

    let popover = CommentThreadPopover::new(model, Theme::dark(), 0.0);
    let region = canvas();
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), region);
    // Nothing to close or reopen, so that band is not a resolution target.
    assert_eq!(
        popover.hit_test(rect, center(CommentThreadPopover::resolution_rect(rect))),
        CommentPopoverHit::Inside
    );

    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    popover.paint(&mut cx, rect);
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(
        painted.iter().any(|text| text.contains("Write a comment")),
        "the new-thread placeholder is the one painted: {painted:?}"
    );
    assert!(
        painted.iter().any(|text| text == &"Send"),
        "the send button is there from the start, dimmed: {painted:?}"
    );
    assert!(
        !painted.iter().any(|text| text == &"Resolve"),
        "there is nothing to resolve yet: {painted:?}"
    );
}

#[test]
fn a_thread_shows_its_author_the_age_and_the_body() {
    let ui = thread_ui(vec![comment(
        10,
        "Kay",
        Some("ux_ui"),
        "hello there",
        1_699_999_700,
    )]);
    let popover = popover(&ui, 1_700_000_000_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    popover.paint(&mut cx, rect);
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Kay"), "{painted:?}");
    assert!(painted.contains(&"5m ago"), "{painted:?}");
    assert!(painted.contains(&"hello there"), "{painted:?}");
    assert!(painted.contains(&"Resolve"), "{painted:?}");
    assert!(painted.contains(&"Reply…"), "{painted:?}");
}

#[test]
fn a_resolved_thread_offers_a_reopen_and_names_who_closed_it() {
    let mut ui = thread_ui(vec![comment(10, "Kay", None, "hello", 1_700_000_000)]);
    ui.threads[0].resolved = true;
    ui.threads[0].resolved_by_name = Some("Ada".to_string());
    let popover = popover(&ui, 1_700_000_600_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    popover.paint(&mut cx, rect);
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Reopen"), "{painted:?}");
    assert!(painted.contains(&"Resolved"), "{painted:?}");
    assert!(painted.contains(&"Resolved by Ada"), "{painted:?}");
}

#[test]
fn the_numbers_and_names_are_localized_not_hard_coded() {
    let ui = thread_ui(vec![comment(10, "Кай", None, "привет", 1_699_999_700)]);
    // Nothing typed: the field paints its own localized placeholder.
    let model = CommentPopoverModel::for_comments(&ui, Locale::Ru, None, true).unwrap();
    let popover = CommentThreadPopover::new(model, Theme::dark(), 1_700_000_000_000.0);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    popover.paint(&mut cx, rect);
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(painted.contains(&"Решено"), "{painted:?}");
    assert!(painted.contains(&"Отправить"), "{painted:?}");
    assert!(painted.contains(&"Ответить…"), "{painted:?}");
    assert!(painted.contains(&"5м назад"), "{painted:?}");
}

#[test]
fn an_empty_thread_paints_a_frame_instead_of_nothing() {
    // The daemon's list is a LEFT JOIN, so a thread with no comments is a shape
    // that can arrive; the panel must not be a zero-height box around it.
    let ui = thread_ui(vec![]);
    let popover = popover(&ui, 0.0);
    assert!(popover.height() > HEADER_H + PAD);
    let rect = popover.rect_at(Rect::xywh(400.0, 300.0, 22.0, 22.0), canvas());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    popover.paint(&mut cx, rect);
    assert!(!backend.round_fills.is_empty(), "the frame is painted");
}
