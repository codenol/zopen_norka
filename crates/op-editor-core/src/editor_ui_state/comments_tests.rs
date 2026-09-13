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

fn thread(id: i64, node: &str, bodies: &[&str]) -> CommentThread {
    CommentThread {
        id,
        node_id: node.to_string(),
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
    // The panel toggle is chrome, so it starts closed but is not "document".
    assert!(!state.panel_open);
}

#[test]
fn opening_a_thread_then_another_drops_the_first_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "n1", &["first"]),
        thread(2, "n2", &["second"]),
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
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.reply_draft.push_str("half typed");
    state.open(1);
    assert_eq!(state.draft(), "half typed");
}

#[test]
fn a_reload_that_lost_the_open_thread_closes_it() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.reply_draft.push_str("draft");
    state.install_threads(vec![]);
    assert!(state.open_thread.is_none());
    assert_eq!(state.draft(), "");
}

#[test]
fn a_reload_that_still_has_the_thread_leaves_it_open() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.install_threads(vec![thread(1, "n1", &["first", "answer"])]);
    assert!(state.is_open(1));
    assert_eq!(state.thread(1).unwrap().reply_count(), 1);
}

#[test]
fn an_upsert_replaces_in_place_and_appends_a_new_thread() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "n1", &["first"]),
        thread(2, "n2", &["second"]),
    ]);

    state.upsert_thread(thread(1, "n1", &["first", "answer"]));
    assert_eq!(state.thread_ids(), vec![1, 2]);
    assert_eq!(state.thread(1).unwrap().reply_count(), 1);

    state.upsert_thread(thread(3, "n3", &["third"]));
    assert_eq!(state.thread_ids(), vec![1, 2, 3]);
}

#[test]
fn the_ordinal_is_the_number_a_pin_shows() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(7, "n1", &["a"]), thread(9, "n2", &["b"])]);
    assert_eq!(state.ordinal(7), Some(1));
    assert_eq!(state.ordinal(9), Some(2));
    assert_eq!(state.ordinal(8), None);
}

#[test]
fn threads_are_found_by_the_node_their_pin_sits_on() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![
        thread(1, "n1", &["a"]),
        thread(2, "n2", &["b"]),
        thread(3, "n1", &["c"]),
    ]);
    let on_n1: Vec<i64> = state
        .threads_on_node("n1")
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    assert_eq!(on_n1, vec![1, 3]);
    assert!(state.threads_on_node("gone").is_empty());
}

#[test]
fn pin_mode_arms_the_next_click_and_its_own_off_switch() {
    let mut state = CommentsUiState::default();
    state.toggle_pin_mode();
    assert!(state.pin_mode);

    state.begin_thread_on("n4");
    assert_eq!(
        state.composer(),
        Some(CommentComposer::NewThread("n4".into()))
    );
    // Pin mode stays armed: a review is several pins in a row.
    assert!(state.pin_mode);

    state.toggle_pin_mode();
    assert!(!state.pin_mode);
    // Disarming abandons a composer that was waiting for its click.
    assert_eq!(state.composer(), None);
    assert_eq!(state.draft(), "");
}

#[test]
fn a_pin_click_on_an_element_replaces_an_open_thread() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.begin_thread_on("n2");
    // The click asked for a new pin, so the open thread must not keep the
    // popover: two anchors cannot share one composer.
    assert!(state.open_thread.is_none());
    assert_eq!(
        state.composer(),
        Some(CommentComposer::NewThread("n2".into()))
    );
}

#[test]
fn an_empty_node_id_never_opens_a_composer() {
    let mut state = CommentsUiState::default();
    state.begin_thread_on("");
    assert_eq!(state.composer(), None);
}

#[test]
fn sending_a_reply_queues_the_write_and_clears_the_draft() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
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
fn sending_a_new_thread_places_its_pin_and_disarms_pin_mode() {
    let mut state = CommentsUiState::default();
    state.toggle_pin_mode();
    state.begin_thread_on("n4");
    state.new_draft = "this button is too close to the edge".to_string();
    assert!(state.send());
    assert!(!state.pin_mode);
    assert_eq!(state.composer(), None);
    assert_eq!(
        state.take_requests(),
        vec![CommentRequest::Create {
            node_id: "n4".to_string(),
            text: "this button is too close to the edge".to_string(),
        }]
    );
}

#[test]
fn a_draft_of_nothing_but_spaces_is_not_sendable() {
    let mut state = CommentsUiState::default();
    state.begin_thread_on("n1");
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
    state.begin_thread_on("n1");
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
    state.install_threads(vec![thread(1, "n1", &["first"])]);
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
        node_id: "n9".to_string(),
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
    let mut closed = thread(2, "n2", &["done"]);
    closed.resolved = true;
    closed.resolved_at = Some(1_700_000_500);
    closed.resolved_by = Some("u2".to_string());
    closed.resolved_by_name = Some("Ada".to_string());
    let mut nameless = thread(3, "n3", &["also done"]);
    nameless.resolved = true;
    nameless.resolved_by_name = Some(String::new());
    state.install_threads(vec![thread(1, "n1", &["a"]), closed, nameless]);

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
fn a_document_change_forgets_the_conversation_but_keeps_the_panel() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.reply_draft = "half typed".to_string();
    state.toggle_pin_mode();
    state.panel_open = true;
    state.loading = true;
    state.error = Some("stale".to_string());

    state.clear_for_document();

    assert!(state.threads.is_empty());
    assert!(state.open_thread.is_none());
    assert_eq!(state.draft(), "");
    assert!(!state.pin_mode);
    assert!(state.error.is_none());
    assert!(!state.loading);
    assert!(!state.has_pending());
    // The panel a reviewer opened is still the panel they want.
    assert!(state.panel_open);
}

#[test]
fn a_loaded_answer_clears_the_spinner_and_the_previous_error() {
    let mut state = CommentsUiState::default();
    state.set_loading();
    assert!(state.loading);
    state.set_error("the daemon refused");
    assert!(!state.loading);
    assert!(state.error.is_some());
    state.install_threads(vec![thread(1, "n1", &["a"])]);
    assert!(state.error.is_none());
    assert!(!state.loading);
}

#[test]
fn cancelling_the_composer_discards_both_drafts() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.open(1);
    state.reply_draft = "reply".to_string();
    state.cancel_composer();
    assert_eq!(state.composer(), None);
    assert_eq!(state.reply_draft, "");

    state.begin_thread_on("n2");
    state.new_draft = "new".to_string();
    state.cancel_composer();
    assert_eq!(state.composer(), None);
    assert_eq!(state.new_draft, "");
}

#[test]
fn the_field_takes_the_keyboard_when_a_composer_opens_and_gives_it_back() {
    let mut state = CommentsUiState::default();
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    assert!(!state.composer_focused, "nothing to type into yet");

    state.open(1);
    assert!(state.composer_focused, "opening a thread focuses its field");

    state.blur_composer();
    assert!(!state.composer_focused);
    state.focus_composer();
    assert!(state.composer_focused);

    state.begin_thread_on("n2");
    assert!(
        state.composer_focused,
        "a pin click opens a field to type in"
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
    let mut state = CommentsUiState::default();
    assert!(!state.takes_keyboard(), "nothing is open");

    state.install_threads(vec![thread(1, "n1", &["first"])]);
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

    state.begin_thread_on("n2");
    assert!(state.takes_keyboard(), "a new thread's field owns it too");
}

#[test]
fn the_viewer_id_survives_a_document_change() {
    let mut state = CommentsUiState::default();
    state.set_viewer_id(Some("u1".to_string()));
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.clear_for_document();
    // Identity is not something the document said.
    assert_eq!(state.viewer_id.as_deref(), Some("u1"));
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
    state.install_threads(vec![thread(1, "n1", &["first"])]);
    state.set_loading();
    state.set_loading_done();
    assert!(!state.loading);
    assert_eq!(state.threads.len(), 1);
    assert!(state.error.is_none());
}
