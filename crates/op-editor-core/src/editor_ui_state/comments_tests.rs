//! Tests for the comment-thread UI state.

use super::*;

fn author(id: Option<&str>, name: &str, role: Option<&str>) -> CommentAuthor {
    CommentAuthor {
        id: id.map(str::to_string),
        name: name.to_string(),
        role: role.map(str::to_string),
    }
}

fn comment(id: i64, body: &str) -> Comment {
    Comment {
        id,
        author: author(Some("u1"), "Kay", Some("ux_ui")),
        body: body.to_string(),
        created_at: 1_700_000_000,
    }
}

/// A thread on `page`, a whole number of document units apart per id.
///
/// The position is derived from the id so a test that reasons about a pin's
/// place can name it without a second literal per case.
fn thread(id: i64, page: &str, bodies: &[&str]) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, id as f64 * 10.0, id as f64 * 20.0)),
        created_at: 1_700_000_000,
        resolved: false,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: bodies
            .iter()
            .enumerate()
            .map(|(index, body)| comment(id * 10 + index as i64, body))
            .collect(),
    }
}

/// The state a test starts from: comments available, nothing loaded.
///
/// `transport` is what a host declares when it carries the daemon's comment
/// client; the toolbar offers the tool only then, and the mode refuses to turn
/// on without it, so every test that exercises the tool has to say so.
fn state_with_transport() -> CommentsUiState {
    let mut state = CommentsUiState::default();
    state.transport = true;
    state
}

#[test]
fn a_fresh_state_is_quiet_and_empty() {
    let state = CommentsUiState::default();
    assert!(state.threads.is_empty());
    assert!(!state.loading);
    assert!(state.error.is_none());
    assert!(state.open_thread.is_none());
    assert!(!state.pin_mode);
    assert!(state.composer().is_none());
    assert!(!state.has_pending());
    // No host has declared a comment client yet, so neither the rail nor the
    // tool it belongs to is on offer.
    assert!(!state.transport);
    assert!(!state.rail_visible());
}

#[test]
fn opening_a_thread_then_another_drops_the_first_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "p1", &["first"]),
        thread(2, "p1", &["second"]),
    ]);

    state.open(1);
    state.reply_draft.push_str("a sentence for thread one");
    assert_eq!(state.draft(), "a sentence for thread one");

    state.open(2);
    // Carrying the draft over would send one reviewer's sentence into another
    // conversation.
    assert_eq!(state.draft(), "");
    assert!(state.is_open(2));
    assert_eq!(state.composer(), Some(CommentComposer::Thread(2)));
}

#[test]
fn reopening_the_same_thread_keeps_its_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft.push_str("half typed");
    state.open(1);
    assert_eq!(state.draft(), "half typed");
}

#[test]
fn a_reload_that_lost_the_open_thread_closes_it() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft.push_str("draft");
    state.install_threads(vec![]);
    assert!(state.open_thread.is_none());
    assert_eq!(state.draft(), "");
}

#[test]
fn a_reload_that_still_has_the_thread_leaves_it_open() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.install_threads(vec![thread(1, "p1", &["first", "answer"])]);
    assert!(state.is_open(1));
    assert_eq!(state.thread(1).unwrap().reply_count(), 1);
}

#[test]
fn an_upsert_replaces_in_place_and_appends_a_new_thread() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "p1", &["first"]),
        thread(2, "p1", &["second"]),
    ]);

    state.upsert_thread(thread(1, "p1", &["first", "answer"]));
    assert_eq!(state.thread_ids(), vec![1, 2]);
    assert_eq!(state.thread(1).unwrap().reply_count(), 1);

    state.upsert_thread(thread(3, "p1", &["third"]));
    assert_eq!(state.thread_ids(), vec![1, 2, 3]);
}

#[test]
fn the_ordinal_is_the_number_a_pin_shows() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(7, "p1", &["a"]), thread(9, "p1", &["b"])]);
    assert_eq!(state.ordinal(7), Some(1));
    assert_eq!(state.ordinal(9), Some(2));
    assert_eq!(state.ordinal(8), None);
}

#[test]
fn threads_are_found_by_the_page_their_pin_sits_on() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "p1", &["a"]),
        thread(2, "p2", &["b"]),
        thread(3, "p1", &["c"]),
    ]);
    let on_p1: Vec<i64> = state
        .threads_on_page("p1")
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    assert_eq!(on_p1, vec![1, 3]);
    assert!(state.threads_on_page("gone").is_empty());
}

#[test]
fn a_thread_with_no_pin_is_listed_everywhere_and_numbered_nowhere() {
    // The daemon migrated threads from the element-keyed format have no page and
    // no coordinates. Dropping them would hide a conversation nobody could find;
    // numbering them would print a number no marker carries.
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "p1", &["a"]),
        CommentThread {
            id: 2,
            anchor: None,
            comments: vec![comment(20, "old")],
            ..CommentThread::default()
        },
    ]);
    assert_eq!(state.threads_on_page("p1").len(), 2);
    assert_eq!(state.threads_on_page("p2").len(), 1);
    assert!(state.pinned_on_page("p1").iter().all(|t| t.id == 1));
    assert_eq!(state.ordinal(1), Some(1));
    assert_eq!(state.ordinal(2), None);
    // It is outstanding work on whichever page the reviewer is looking at, and
    // it is *not* "on another page" — it is on none.
    assert_eq!(state.open_count_on_page("p2"), 1);
    assert_eq!(
        state.open_count_elsewhere("p1"),
        0,
        "nothing pinned is elsewhere from p1"
    );
    assert_eq!(
        state.open_count_elsewhere("p2"),
        1,
        "p1's pinned thread is elsewhere from p2 — the pin-less one never is"
    );
}

#[test]
fn a_coordinate_the_daemon_would_refuse_is_not_placeable() {
    // The write route refuses these with a 400, so the client must not offer the
    // pin in the first place — and a read that carried one anyway is a thread
    // with no drawable marker rather than a marker at an absurd position.
    assert!(CommentAnchor::new("p1", 0.0, 0.0).is_placeable());
    assert!(CommentAnchor::new("p1", -9_999_999.0, 9_999_999.0).is_placeable());
    for anchor in [
        CommentAnchor::new("p1", f64::NAN, 0.0),
        CommentAnchor::new("p1", 0.0, f64::INFINITY),
        CommentAnchor::new("p1", MAX_COMMENT_COORDINATE + 1.0, 0.0),
        CommentAnchor::new("p1", 0.0, -MAX_COMMENT_COORDINATE - 1.0),
    ] {
        assert!(!anchor.is_placeable(), "{anchor:?}");
    }
    let mut state = CommentsUiState::default();
    state.install_threads(vec![CommentThread {
        id: 1,
        anchor: Some(CommentAnchor::new("p1", 1.0e9, 0.0)),
        ..CommentThread::default()
    }]);
    assert!(
        state.threads_on_page("p1").len() == 1,
        "the thread is listed"
    );
    assert!(state.pinned_on_page("p1").is_empty(), "with no pin to draw");
}

#[test]
fn a_pages_list_numbers_its_own_threads_and_the_badge_counts_them() {
    // The rail lists one page and the pins are placed on one page, so the
    // number a marker shows has to be the index in THAT list — a document-wide
    // index would number a pin by threads the reviewer cannot see.
    let mut state = CommentsUiState::default();
    let mut closed = thread(4, "p1", &["done"]);
    closed.resolved = true;
    state.install_threads(vec![
        thread(1, "p1", &["a"]),
        thread(2, "p2", &["b"]),
        thread(3, "p1", &["c"]),
        closed,
    ]);

    assert_eq!(state.ordinal(1), Some(1));
    assert_eq!(state.ordinal(3), Some(2));
    assert_eq!(state.ordinal(2), Some(1), "p2's own list starts at one");
    assert_eq!(state.open_count_on_page("p1"), 2);
    assert_eq!(state.open_count_on_page("p2"), 1);
    // The rest of the review is counted, not listed: the rail shows one page.
    assert_eq!(state.open_count_elsewhere("p1"), 1);
    assert_eq!(state.open_count_elsewhere("p2"), 2);
    assert_eq!(state.open_count(), 3);
}

#[test]
fn an_anchor_that_could_not_be_painted_is_refused() {
    let mut state = state_with_transport();
    state.begin_thread_at(CommentAnchor::new("p1", f64::NAN, 4.0));
    assert_eq!(state.composer(), None, "a NaN pin is nowhere to point at");
    state.begin_thread_at(CommentAnchor::new("p1", 4.0, f64::INFINITY));
    assert_eq!(state.composer(), None);
    state.begin_thread_at(CommentAnchor::new("p1", 4.0, 8.0));
    assert!(state.composer().is_some());
}

#[test]
fn pin_mode_arms_the_next_click_and_its_own_off_switch() {
    let mut state = state_with_transport();
    state.toggle_pin_mode();
    assert!(state.pin_mode);
    // Selecting the tool is also what fills the rail, and the list it opens has
    // to be asked for: the daemon pushes no signal for comments.
    assert!(state.rail_visible());
    assert_eq!(state.take_requests(), vec![CommentRequest::Reload]);

    state.begin_thread_at(CommentAnchor::new("p4", 120.0, 64.0));
    assert_eq!(
        state.composer(),
        Some(CommentComposer::NewThread(CommentAnchor::new(
            "p4", 120.0, 64.0
        )))
    );
    // The tool stays active: a review is several comments in a row, and the
    // mode ends the way every other tool's does.
    assert!(state.pin_mode);

    state.toggle_pin_mode();
    assert!(!state.pin_mode);
    // Leaving the tool abandons a composer that was waiting for its click.
    assert_eq!(state.composer(), None);
    assert_eq!(state.draft(), "");
}

#[test]
fn a_host_without_a_comment_client_cannot_select_the_tool() {
    // A stray call — a stale script, a future host — must not blank the rail on
    // a build that has nothing to put in it.
    let mut state = CommentsUiState::default();
    assert!(!state.transport);
    state.begin_mode();
    assert!(!state.pin_mode);
    assert!(!state.rail_visible());
    assert!(!state.has_pending(), "and nothing is asked of the daemon");
}

#[test]
fn end_mode_closes_the_rail_and_whatever_was_half_written() {
    let mut state = state_with_transport();
    state.begin_mode();
    state.begin_thread_at(CommentAnchor::new("p1", 10.0, 20.0));
    state.new_draft = "half typed".to_string();
    state.end_mode();
    assert!(!state.pin_mode);
    assert!(!state.rail_visible());
    assert_eq!(state.composer(), None);
    assert_eq!(state.draft(), "");
    assert!(!state.takes_keyboard(), "and it gives the keyboard back");
}

#[test]
fn a_canvas_click_replaces_an_open_thread() {
    let mut state = state_with_transport();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.begin_thread_at(CommentAnchor::new("p1", 30.0, 40.0));
    // The click asked for a new pin, so the open thread must not keep the
    // popover: two anchors cannot share one composer.
    assert!(state.open_thread.is_none());
    assert_eq!(
        state.composer(),
        Some(CommentComposer::NewThread(CommentAnchor::new(
            "p1", 30.0, 40.0
        )))
    );
}

#[test]
fn sending_a_reply_queues_the_write_and_clears_the_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft = "  looks good  ".to_string();
    assert!(state.can_send());
    assert!(state.send());
    assert_eq!(state.draft(), "");
    assert_eq!(
        state.take_requests(),
        vec![CommentRequest::Reply {
            thread_id: 1,
            text: "looks good".to_string(),
        }]
    );
}

#[test]
fn sending_a_new_thread_carries_the_point_it_was_written_at() {
    let mut state = state_with_transport();
    state.toggle_pin_mode();
    state.take_requests();
    state.begin_thread_at(CommentAnchor::new("p1", 412.5, 88.25));
    state.new_draft = "this button is too close to the edge".to_string();
    assert!(state.send());
    assert_eq!(state.composer(), None);
    // The tool stays active after a send: the pin is placed by the server's
    // answer, and the reviewer leaves the mode by picking another tool.
    assert!(state.pin_mode);
    assert_eq!(
        state.take_requests(),
        vec![CommentRequest::Create {
            anchor: CommentAnchor::new("p1", 412.5, 88.25),
            text: "this button is too close to the edge".to_string(),
        }]
    );
}

#[test]
fn a_draft_of_nothing_but_spaces_is_not_sendable() {
    let mut state = CommentsUiState::default();
    state.begin_thread_at(CommentAnchor::new("p1", 1.0, 2.0));
    state.new_draft = "   \n\t ".to_string();
    assert!(!state.can_send());
    assert!(!state.send());
    assert!(!state.has_pending());
    // The draft survives, because the user is still typing.
    assert_eq!(state.draft(), "   \n\t ");
}

#[test]
fn a_draft_over_the_servers_ceiling_is_not_sendable() {
    let mut state = CommentsUiState::default();
    state.begin_thread_at(CommentAnchor::new("p1", 1.0, 2.0));
    state.new_draft = "я".repeat(MAX_COMMENT_CHARS);
    assert!(state.can_send(), "the bound itself is accepted");
    state.new_draft.push('я');
    assert!(!state.can_send(), "one past the bound is refused");
}

#[test]
fn sending_nothing_at_all_queues_nothing() {
    let mut state = CommentsUiState::default();
    assert!(!state.send());
    assert!(!state.has_pending());
}

#[test]
fn resolve_and_reopen_are_separate_requests() {
    let mut state = CommentsUiState::default();
    state.resolve(3);
    state.reopen(4);
    assert_eq!(
        state.take_requests(),
        vec![
            CommentRequest::Resolve { thread_id: 3 },
            CommentRequest::Reopen { thread_id: 4 },
        ]
    );
}

#[test]
fn a_second_reload_request_before_the_drain_is_absorbed() {
    let mut state = CommentsUiState::default();
    state.request_reload();
    state.request_reload();
    assert_eq!(state.take_requests(), vec![CommentRequest::Reload]);
    // After the drain a new ask is queued again.
    state.request_reload();
    assert!(state.has_pending());
}

#[test]
fn a_refusal_is_recorded_without_restoring_the_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft = "text the server refused".to_string();
    state.send();
    // The host has taken the write; what it brings back is the refusal.
    assert_eq!(state.take_requests().len(), 1);
    state.note_write_error(CommentWriteError::Refused);
    assert_eq!(state.error.as_deref(), Some("comments.error.refused"));
    // Retyping is the user's call: putting the text back into a field they
    // believe they sent is how the same comment arrives twice.
    assert_eq!(state.draft(), "");
    assert!(!state.has_pending(), "a 403 queues no automatic retry");
}

#[test]
fn a_missing_thread_asks_for_a_reload() {
    let mut state = CommentsUiState::default();
    state.note_write_error(CommentWriteError::Gone);
    assert_eq!(state.error.as_deref(), Some("comments.error.gone"));
    assert_eq!(state.take_requests(), vec![CommentRequest::Reload]);
}

#[test]
fn every_write_error_names_its_own_key() {
    assert_eq!(
        CommentWriteError::Refused.message_key(),
        "comments.error.refused"
    );
    assert_eq!(CommentWriteError::Gone.message_key(), "comments.error.gone");
    assert_eq!(
        CommentWriteError::Rejected("Missing text string".into()).message_key(),
        "comments.error.rejected"
    );
    assert_eq!(
        CommentWriteError::Transport("the request never left".into()).message_key(),
        "comments.error.transport"
    );
    assert_eq!(
        CommentWriteError::UnsavedDocument.message_key(),
        "comments.error.unsaved"
    );
}

#[test]
fn a_thread_without_comments_is_a_shape_the_model_survives() {
    // The daemon's list is a LEFT JOIN, so a thread with no comments is
    // reachable — a panel that indexed `comments[0]` would panic on it.
    let mut state = CommentsUiState::default();
    state.install_threads(vec![CommentThread {
        id: 5,
        anchor: Some(CommentAnchor::new("p1", 10.0, 20.0)),
        created_at: 1_700_000_000,
        resolved: false,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![],
    }]);
    let held = state.thread(5).unwrap();
    assert!(held.first().is_none());
    assert!(held.opener().is_none());
    assert_eq!(held.reply_count(), 0);
}

#[test]
fn a_resolved_thread_keeps_its_resolver_and_the_open_count_drops() {
    let mut state = CommentsUiState::default();
    let mut closed = thread(2, "p1", &["done"]);
    closed.resolved = true;
    closed.resolved_at = Some(1_700_000_500);
    closed.resolved_by = Some("u2".to_string());
    closed.resolved_by_name = Some("Ada".to_string());
    let mut nameless = thread(3, "p1", &["also done"]);
    nameless.resolved = true;
    nameless.resolved_by_name = Some(String::new());
    state.install_threads(vec![thread(1, "p1", &["a"]), closed, nameless]);

    assert_eq!(state.open_count(), 1);
    assert_eq!(state.thread(2).unwrap().resolved_by_label(), Some("Ada"));
    // The local operator's empty name is not a name to paint.
    assert_eq!(state.thread(3).unwrap().resolved_by_label(), None);
}

#[test]
fn the_local_operator_is_an_author_without_an_account() {
    let anonymous = author(None, "", None);
    assert!(anonymous.is_local_operator());
    let account = author(Some("u1"), "Kay", Some("qa"));
    assert!(!account.is_local_operator());
}

#[test]
fn a_document_change_forgets_the_conversation_and_the_tool() {
    let mut state = state_with_transport();
    state.install_threads_for_key(Some("key1".to_string()), vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft = "half typed".to_string();
    state.toggle_pin_mode();
    state.loading = true;
    state.error = Some("stale".to_string());
    state.take_requests();

    // Another document: a different key is a different conversation, and a pin
    // dropped on this one would land on a page that is not its page.
    state.clear_for_document(Some("key2"));

    assert!(state.threads.is_empty());
    assert!(state.open_thread.is_none());
    assert_eq!(state.draft(), "");
    // The comment tool goes with the document: a pin is a point on a page, and
    // the next document's coordinates are not that page's.
    assert!(!state.pin_mode);
    assert!(!state.rail_visible());
    assert!(state.error.is_none());
    assert!(!state.loading);
    assert!(!state.has_pending());
    assert_eq!(state.document_key(), None);
    // What the host is, rather than what the document said, survives.
    assert!(state.transport);
}

#[test]
fn the_same_document_replaced_keeps_its_conversation() {
    // The whole point of the key: an AI turn, an external MCP write or a
    // collaboration commit replaces the DOCUMENT, not the conversation. Wiping
    // here is what made a marker disappear between the read at open and the
    // document arriving a round trip later.
    let mut state = state_with_transport();
    state.install_threads_for_key(Some("key1".to_string()), vec![thread(1, "p1", &["first"])]);
    state.open(1);

    state.clear_for_document(Some("key1"));

    assert_eq!(state.thread_ids(), vec![1]);
    assert!(state.is_open(1), "the reviewer's thread stays open");
    assert_eq!(state.document_key(), Some("key1"));
}

#[test]
fn a_document_that_was_never_read_is_not_the_same_as_an_empty_one() {
    // No key has been read yet, so nothing may be kept: an install of the same
    // "no key" would otherwise leave the previous document's threads in place.
    let mut state = state_with_transport();
    state.install_threads(vec![thread(1, "p1", &["first"])]);

    state.clear_for_document(None);

    assert!(state.threads.is_empty());
    assert_eq!(state.document_key(), None);
}

#[test]
fn an_account_change_forgets_the_conversation_whatever_the_document_is() {
    // The daemon answers a conversation per account, so the key matching is not
    // enough: a tab that has just stopped speaking for the account whose words
    // it holds must not paint them for the account that replaced it.
    let mut state = state_with_transport();
    state.install_threads_for_key(Some("key1".to_string()), vec![thread(1, "p1", &["first"])]);

    state.forget_threads();

    assert!(state.threads.is_empty());
    assert_eq!(
        state.document_key(),
        None,
        "and the next replacement of key1 must not keep an empty list as if it were read"
    );
}

#[test]
fn a_loaded_answer_clears_the_spinner_and_the_previous_error() {
    let mut state = CommentsUiState::default();
    state.set_loading();
    assert!(state.loading);
    state.set_error("the daemon refused");
    assert!(!state.loading);
    assert!(state.error.is_some());
    state.install_threads(vec![thread(1, "p1", &["a"])]);
    assert!(state.error.is_none());
    assert!(!state.loading);
}

#[test]
fn cancelling_the_composer_discards_both_drafts() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    state.reply_draft = "reply".to_string();
    state.cancel_composer();
    assert_eq!(state.composer(), None);
    assert_eq!(state.reply_draft, "");

    state.begin_thread_at(CommentAnchor::new("p1", 5.0, 6.0));
    state.new_draft = "new".to_string();
    state.cancel_composer();
    assert_eq!(state.composer(), None);
    assert_eq!(state.new_draft, "");
}

#[test]
fn the_field_takes_the_keyboard_when_a_composer_opens_and_gives_it_back() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    assert!(!state.composer_focused, "nothing to type into yet");

    state.open(1);
    assert!(state.composer_focused, "opening a thread focuses its field");

    state.blur_composer();
    assert!(!state.composer_focused);
    state.focus_composer();
    assert!(state.composer_focused);

    state.begin_thread_at(CommentAnchor::new("p1", 5.0, 6.0));
    assert!(
        state.composer_focused,
        "a canvas click opens a field to type in"
    );

    state.cancel_composer();
    assert!(!state.composer_focused);
}

#[test]
fn focusing_the_field_without_a_composer_does_nothing() {
    let mut state = CommentsUiState::default();
    state.focus_composer();
    // There is no field to type into, so the keyboard must stay where it was.
    assert!(!state.composer_focused);
}

#[test]
fn the_field_answers_whether_it_owns_the_keyboard() {
    // One question, one answer: the host's "a text input owns the keyboard"
    // rule and every keyboard arm read this instead of re-deriving it from the
    // focus flag, which can outlive the popover (issue #49).
    let mut state = state_with_transport();
    assert!(!state.takes_keyboard(), "nothing is open");

    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.open(1);
    assert!(state.takes_keyboard());

    state.blur_composer();
    assert!(!state.takes_keyboard(), "a defocused field owns nothing");

    // The stale-flag case: focus left set while no composer is on screen.
    state.composer_focused = true;
    state.close();
    assert!(
        !state.takes_keyboard(),
        "a closed popover cannot keep the keyboard"
    );

    state.begin_thread_at(CommentAnchor::new("p1", 5.0, 6.0));
    assert!(state.takes_keyboard(), "a new thread's field owns it too");

    // Issue #49's shape, restated for the coordinate model: selecting the
    // comment tool and clicking the canvas is how the composer is reached, and
    // the field must own the keyboard from that click on — a bare letter has to
    // reach the draft rather than switch the tool.
    state.end_mode();
    state.begin_mode();
    state.begin_thread_at(CommentAnchor::new("p1", 7.0, 8.0));
    assert!(state.pin_mode, "the rail is showing the conversation");
    assert!(state.takes_keyboard());
}

#[test]
fn the_viewer_id_survives_a_document_change() {
    let mut state = CommentsUiState::default();
    state.set_viewer_id(Some("u1".to_string()));
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.clear_for_document(None);
    // Identity is not something the document said.
    assert_eq!(state.viewer_id.as_deref(), Some("u1"));
}

#[test]
fn a_document_replaced_under_the_same_key_keeps_its_conversation() {
    // Through the real install path, not the state method: opening a document
    // adopts the key first and the content arrives a round trip later, and the
    // small comment list is normally answered before the document it belongs to.
    // If the install wiped the list, that answer would be lost and the markers
    // would stay invisible until the tool was opened — the bug this exists for.
    let mut editor = crate::EditorState::starter();
    editor.editor_ui.file_key = Some("key1".to_string());
    editor
        .editor_ui
        .comments
        .install_threads_for_key(Some("key1".to_string()), vec![thread(1, "p1", &["first"])]);

    editor.replace_document(crate::EditorState::starter().doc);

    assert_eq!(editor.editor_ui.comments.thread_ids(), vec![1]);
    assert_eq!(editor.editor_ui.comments.document_key(), Some("key1"));
}

#[test]
fn a_document_replaced_under_another_key_leaves_its_conversation_behind() {
    let mut editor = crate::EditorState::starter();
    editor
        .editor_ui
        .comments
        .install_threads_for_key(Some("key1".to_string()), vec![thread(1, "p1", &["first"])]);
    // The open of another document: the route adopts the new key, then the
    // content lands.
    editor.editor_ui.file_key = Some("key2".to_string());

    editor.replace_document(crate::EditorState::starter().doc);

    assert!(
        editor.editor_ui.comments.threads.is_empty(),
        "one document's pins must never be painted over another's pages"
    );
    assert_eq!(editor.editor_ui.comments.document_key(), None);
}

#[test]
fn a_failure_ends_the_wait_as_well_as_recording_it() {
    let mut state = CommentsUiState::default();
    state.set_loading();
    state.note_write_error(CommentWriteError::Transport("no answer".into()));
    // A spinner beside a message reads as stuck, not as refused.
    assert!(!state.loading);
}

#[test]
fn ending_a_read_leaves_the_list_and_the_error_alone() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "p1", &["first"])]);
    state.set_loading();
    state.set_loading_done();
    assert!(!state.loading);
    assert_eq!(state.threads.len(), 1);
    assert!(state.error.is_none());
}
