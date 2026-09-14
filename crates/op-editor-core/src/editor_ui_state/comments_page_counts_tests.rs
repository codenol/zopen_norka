//! The one count behind a page's marker and the toolbar badge.
//!
//! What a reviewer can be wrong about here: a page's number including a
//! conversation pinned somewhere else, a resolved thread still being counted,
//! and — the case the two readings of the count exist for — a migrated,
//! pin-less thread either vanishing or being attributed to a page that never
//! held it.

use super::*;
use crate::editor_ui_state::comments::*;

fn comment(id: i64, body: &str) -> Comment {
    Comment {
        id,
        author: CommentAuthor {
            id: Some("u1".to_string()),
            name: "Kay".to_string(),
            role: Some("ux_ui".to_string()),
        },
        body: body.to_string(),
        created_at: 1_700_000_000,
    }
}

fn thread(id: i64, page: &str, resolved: bool) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, id as f64 * 10.0, id as f64 * 20.0)),
        created_at: 1_700_000_000,
        resolved,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![comment(id * 10, "hello")],
    }
}

/// A thread the daemon migrated from the old element-keyed format: no anchor.
fn migrated_thread(id: i64, resolved: bool) -> CommentThread {
    CommentThread {
        id,
        anchor: None,
        created_at: 1_699_000_000,
        resolved,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![comment(id * 10, "written before coordinates")],
    }
}

fn state_with(threads: Vec<CommentThread>) -> CommentsUiState {
    let mut state = CommentsUiState::default();
    state.install_threads(threads);
    state
}

#[test]
fn a_page_counts_the_open_threads_pinned_on_it() {
    let state = state_with(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        thread(3, "p2", false),
    ]);
    assert_eq!(state.page_comment_counts("p1").pinned, 2);
    assert_eq!(state.page_comment_counts("p2").pinned, 1);
    // A page nobody commented on is not a page with a count of zero, it is a
    // page with nothing to say — which is what every surface renders as nothing.
    assert_eq!(state.page_comment_counts("p3").pinned, 0);
}

#[test]
fn another_pages_pin_is_not_this_pages_thread() {
    // The page id is the whole membership test: a coordinate that would sit on
    // p1's canvas belongs to p2 and is counted there.
    let state = state_with(vec![thread(1, "p2", false)]);
    assert_eq!(state.page_comment_counts("p1").pinned, 0);
    assert_eq!(state.page_comment_counts("p1").unpinned, 0);
    assert_eq!(state.page_comment_counts("p2").pinned, 1);
}

#[test]
fn a_resolved_thread_stops_being_counted() {
    let state = state_with(vec![
        thread(1, "p1", false),
        thread(2, "p1", true),
        migrated_thread(3, true),
    ]);
    assert_eq!(
        state.page_comment_counts("p1"),
        PageCommentCounts {
            pinned: 1,
            unpinned: 0
        }
    );
}

#[test]
fn a_thread_with_no_pin_is_a_marker_on_no_page() {
    let state = state_with(vec![thread(1, "p1", false), migrated_thread(2, false)]);
    // No page draws it, so no page's marker may claim it — that is the whole
    // reason `pinned` exists beside `listed`.
    for page in ["p1", "p2", "p3"] {
        assert_eq!(
            state.page_comment_counts(page).pinned,
            usize::from(page == "p1"),
            "page {page} counted a pin-less thread"
        );
    }
    // It is still the rail's business, on every page, and the badge says so.
    assert_eq!(state.page_comment_counts("p1").unpinned, 1);
    assert_eq!(state.page_comment_counts("p2").unpinned, 1);
    assert_eq!(state.page_comment_counts("p2").listed(), 1);
}

#[test]
fn a_badge_number_is_the_marker_number_plus_the_pinless_threads() {
    // The invariant that keeps the two surfaces from being two counts: whatever
    // the document holds, the badge shows what the marker shows plus the
    // conversations the page cannot draw. A reader who finds them disagreeing by
    // more than that has found a bug; a reader who finds them disagreeing by
    // exactly that has found a migrated comment.
    let mut state = state_with(vec![
        thread(1, "p1", false),
        thread(2, "p1", true),
        thread(3, "p2", false),
        migrated_thread(4, false),
        migrated_thread(5, true),
    ]);
    for page in ["p1", "p2", "p3"] {
        let counts = state.page_comment_counts(page);
        assert_eq!(
            state.open_count_on_page(page),
            counts.pinned + counts.unpinned,
            "the toolbar badge and the page marker must come from one count"
        );
    }
    // …and with every thread pinned the two are the same number outright, which
    // is the ordinary document.
    state.install_threads(vec![thread(1, "p1", false), thread(2, "p1", false)]);
    assert_eq!(
        state.open_count_on_page("p1"),
        state.page_comment_counts("p1").pinned
    );
}

#[test]
fn the_counts_agree_with_the_lists_they_are_read_from() {
    // The counts are read out of the page's own two lists, so this states the
    // only thing that could silently drift: which threads each list holds.
    let state = state_with(vec![
        thread(1, "p1", false),
        thread(2, "p2", false),
        migrated_thread(3, false),
    ]);
    assert_eq!(
        state.page_comment_counts("p1").listed(),
        state
            .threads_on_page("p1")
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
    );
    assert_eq!(
        state.page_comment_counts("p1").pinned,
        state
            .pinned_on_page("p1")
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
    );
}
