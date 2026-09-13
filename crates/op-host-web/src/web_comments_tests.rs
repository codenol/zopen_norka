//! Reading the daemon's comment answers, and turning its refusals into
//! something the chrome can show.
//!
//! Everything here is pure: a status and a body in, a typed result out. The XHR
//! plumbing around it (`fetch_threads` / `post_thread`) is deliberately not
//! exercised — a browser test would be testing XmlHttpRequest, and the part
//! that has ever been wrong is the decoding: a null `authorRole`, an empty
//! `authorName`, a thread with no comments, a 403 that is an answer rather than
//! a malfunction.

use super::*;

#[test]
fn a_list_answer_becomes_threads() {
    let body = serde_json::json!({
        "ok": true,
        "threads": [
            {
                "id": 7,
                "nodeId": "n1",
                "createdAt": 1_700_000_000,
                "resolved": false,
                "resolvedAt": null,
                "resolvedBy": null,
                "resolvedByName": null,
                "comments": [
                    {
                        "id": 70,
                        "authorId": "u1",
                        "authorName": "Kay",
                        "authorRole": "ux_ui",
                        "body": "this spacing looks off",
                        "createdAt": 1_700_000_000,
                    }
                ],
            }
        ],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].id, 7);
    assert_eq!(threads[0].node_id, "n1");
    assert!(!threads[0].resolved);
    assert_eq!(threads[0].comments[0].author.id.as_deref(), Some("u1"));
    assert_eq!(threads[0].comments[0].author.name, "Kay");
    assert_eq!(threads[0].comments[0].author.role.as_deref(), Some("ux_ui"));
}

#[test]
fn a_null_role_stays_null_and_an_empty_name_stays_empty() {
    // The local operator: no account, no name, no role. All three are the
    // server's own statements and none of them may be invented here.
    let body = serde_json::json!({
        "ok": true,
        "threads": [{
            "id": 1,
            "nodeId": "n1",
            "createdAt": 1_700_000_000,
            "resolved": false,
            "resolvedAt": null,
            "resolvedBy": null,
            "resolvedByName": null,
            "comments": [{
                "id": 10,
                "authorId": null,
                "authorName": "",
                "authorRole": null,
                "body": "local operator wrote this",
                "createdAt": 1_700_000_000,
            }],
        }],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    let author = &threads[0].comments[0].author;
    assert!(author.is_local_operator());
    assert_eq!(author.name, "");
    assert!(author.role.is_none());
}

#[test]
fn an_unknown_role_travels_through_untouched() {
    // The colour is the widget layer's business; this layer's job is to not
    // lose what the hub said.
    let body = serde_json::json!({
        "ok": true,
        "threads": [{
            "id": 1,
            "nodeId": "n1",
            "createdAt": 0,
            "resolved": false,
            "comments": [{
                "id": 1, "authorId": "u9", "authorName": "Ada",
                "authorRole": "chief-vibes-officer", "body": "hi", "createdAt": 0,
            }],
        }],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert_eq!(
        threads[0].comments[0].author.role.as_deref(),
        Some("chief-vibes-officer")
    );
}

#[test]
fn a_thread_with_no_comments_is_kept_rather_than_dropped() {
    // The daemon's list is a LEFT JOIN: this shape is reachable.
    let body = serde_json::json!({
        "ok": true,
        "threads": [{ "id": 5, "nodeId": "n9", "createdAt": 1, "resolved": false, "comments": [] }],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert_eq!(threads.len(), 1);
    assert!(threads[0].comments.is_empty());
}

#[test]
fn a_resolved_thread_carries_its_resolver_and_a_nameless_one_drops_the_name() {
    let body = serde_json::json!({
        "ok": true,
        "threads": [
            { "id": 1, "nodeId": "n1", "createdAt": 1, "resolved": true, "resolvedAt": 42,
              "resolvedBy": "u2", "resolvedByName": "Ada", "comments": [] },
            { "id": 2, "nodeId": "n2", "createdAt": 1, "resolved": true, "resolvedAt": 43,
              "resolvedBy": null, "resolvedByName": "", "comments": [] },
        ],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert_eq!(threads[0].resolved_at, Some(42));
    assert_eq!(threads[0].resolved_by_label(), Some("Ada"));
    // The local operator's empty name is not a name to paint.
    assert_eq!(threads[1].resolved_by_label(), None);
}

#[test]
fn a_write_answer_becomes_one_thread() {
    let body = serde_json::json!({
        "ok": true,
        "thread": {
            "id": 9, "nodeId": "n2", "createdAt": 1_700_000_500, "resolved": false,
            "resolvedAt": null, "resolvedBy": null, "resolvedByName": null,
            "comments": [{ "id": 90, "authorId": "u1", "authorName": "Kay",
                           "authorRole": "admin", "body": "answered", "createdAt": 1_700_000_500 }],
        },
    })
    .to_string();
    let thread = decode_thread(200, &body).unwrap();
    assert_eq!(thread.id, 9);
    assert_eq!(thread.reply_count(), 0);
}

#[test]
fn the_three_refusals_are_three_named_answers() {
    // 403 is a normal answer to an ordinary situation — a read-only link, a
    // document not shared with this account — so it must not read as a fault.
    assert_eq!(decode_threads(403, "{}"), Err(CommentApiError::Refused));
    assert_eq!(decode_threads(404, "{}"), Err(CommentApiError::NotFound));
    assert_eq!(
        decode_threads(
            400,
            &serde_json::json!({ "ok": false, "error": "Missing text string" }).to_string()
        ),
        Err(CommentApiError::Rejected("Missing text string".to_string()))
    );
    assert_eq!(decode_threads(500, "{}"), Err(CommentApiError::Http(500)));
    assert_eq!(
        decode_threads(200, "not json"),
        Err(CommentApiError::Malformed)
    );
    assert_eq!(
        decode_threads(200, &serde_json::json!({ "ok": false }).to_string()),
        Err(CommentApiError::Malformed)
    );
}

#[test]
fn a_refusal_carries_the_servers_own_words() {
    let body = serde_json::json!({
        "ok": false,
        "error": "Comment text is longer than 4000 characters",
    })
    .to_string();
    assert_eq!(
        decode_thread(400, &body),
        Err(CommentApiError::Rejected(
            "Comment text is longer than 4000 characters".to_string()
        ))
    );
}

#[test]
fn an_answer_with_no_thread_at_all_is_its_own_failure() {
    let ok_without_a_thread = serde_json::json!({ "ok": true, "thread": null }).to_string();
    assert_eq!(
        decode_thread(200, &ok_without_a_thread),
        Err(CommentApiError::MalformedThread)
    );
}

#[test]
fn a_thread_without_an_id_is_not_a_thread() {
    // Without the id nothing can be opened, answered, or closed, so this is a
    // malformed answer rather than a degraded one.
    let body = serde_json::json!({
        "ok": true,
        "threads": [{ "nodeId": "n1", "createdAt": 1, "comments": [] }],
    })
    .to_string();
    assert_eq!(decode_threads(200, &body).unwrap().len(), 0);
}

#[test]
fn each_wire_failure_becomes_the_message_the_reviewer_reads() {
    assert_eq!(
        CommentApiError::Refused.to_write_error(),
        CommentWriteError::Refused
    );
    assert_eq!(
        CommentApiError::NotFound.to_write_error(),
        CommentWriteError::Gone
    );
    assert_eq!(
        CommentApiError::Rejected("Missing nodeId string".to_string()).to_write_error(),
        CommentWriteError::Rejected("Missing nodeId string".to_string())
    );
    for transport in [
        CommentApiError::RequestFailed,
        CommentApiError::Http(502),
        CommentApiError::Malformed,
        CommentApiError::MalformedThread,
    ] {
        assert!(matches!(
            transport.to_write_error(),
            CommentWriteError::Transport(_)
        ));
    }
}

#[test]
fn a_refusal_does_not_ask_for_a_reload_but_a_missing_thread_does() {
    let mut ui = CommentsUiState::default();
    note_failure(&mut ui, &CommentApiError::Refused);
    assert_eq!(ui.error.as_deref(), Some("comments.error.refused"));
    assert!(!ui.has_pending(), "a refusal is final, not a retry");

    let mut ui = CommentsUiState::default();
    note_failure(&mut ui, &CommentApiError::NotFound);
    assert_eq!(ui.error.as_deref(), Some("comments.error.gone"));
    assert_eq!(ui.take_requests(), vec![CommentRequest::Reload]);
}

#[test]
fn a_list_answer_installs_and_a_written_thread_is_upserted() {
    let mut ui = CommentsUiState::default();
    let thread = |id: i64, node: &str| CommentThread {
        id,
        node_id: node.to_string(),
        comments: vec![Comment::default()],
        ..CommentThread::default()
    };

    apply(
        &mut ui,
        AnswerKind::List,
        Ok(Answer::Threads(vec![thread(1, "n1"), thread(2, "n2")])),
    );
    assert_eq!(ui.thread_ids(), vec![1, 2]);

    apply(
        &mut ui,
        AnswerKind::Written,
        Ok(Answer::Thread(Box::new(thread(1, "n1")))),
    );
    assert_eq!(ui.thread_ids(), vec![1, 2], "replaced, not appended");
}

#[test]
fn the_thread_a_pin_click_created_opens_once_the_server_answers() {
    let mut ui = CommentsUiState::default();
    ui.toggle_pin_mode();
    ui.begin_thread_on("n4");
    ui.new_draft = "too tight".to_string();
    ui.send();
    assert_eq!(ui.composer(), None);
    // The composer is gone while the write is in flight; the pin click left the
    // element behind for the answer to open.
    ui.pin_node = Some("n4".to_string());

    apply(
        &mut ui,
        AnswerKind::Written,
        Ok(Answer::Thread(Box::new(CommentThread {
            id: 12,
            node_id: "n4".to_string(),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        }))),
    );
    assert!(
        ui.is_open(12),
        "the written thread is the one being looked at"
    );
}

#[test]
fn a_background_answer_does_not_take_the_panel_away_from_another_thread() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        CommentThread {
            id: 1,
            node_id: "n1".to_string(),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        },
        CommentThread {
            id: 2,
            node_id: "n2".to_string(),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        },
    ]);
    ui.open(2);
    apply(
        &mut ui,
        AnswerKind::Written,
        Ok(Answer::Thread(Box::new(CommentThread {
            id: 1,
            node_id: "n1".to_string(),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        }))),
    );
    assert!(ui.is_open(2), "the reviewer moved on and stays there");
}

#[test]
fn a_failed_list_leaves_a_message_and_no_spinner() {
    let mut ui = CommentsUiState::default();
    ui.set_loading();
    apply(&mut ui, AnswerKind::List, Err(CommentApiError::Http(502)));
    assert!(!ui.loading);
    assert_eq!(ui.error.as_deref(), Some("comments.error.transport"));
}

#[test]
fn every_request_builds_the_route_the_daemon_serves() {
    // Not a tautology: the family is `/api/files/<key>/comments`, and a typo in
    // a path is a 404 the reviewer would read as "your comment was lost".
    assert_eq!(comments_path("abc123", ""), "/api/files/abc123/comments");
    assert_eq!(
        comments_path("abc123", "/7/reply"),
        "/api/files/abc123/comments/7/reply"
    );
    assert_eq!(
        comments_path("abc123", "/7/resolve"),
        "/api/files/abc123/comments/7/resolve"
    );
    assert_eq!(
        comments_path("abc123", "/7/reopen"),
        "/api/files/abc123/comments/7/reopen"
    );
}
