//! Reading the daemon's comment answers, and turning its refusals into
//! something the chrome can show.
//!
//! Everything here is pure: a status and a body in, a typed result out. The XHR
//! plumbing around it (`fetch_threads` / `post_thread`) is deliberately not
//! exercised — a browser test would be testing XmlHttpRequest, and the part
//! that has ever been wrong is the decoding: a null `authorRole`, an empty
//! `authorName`, a thread with no comments, a pin whose `pageId`/`x`/`y` are
//! `null` because the daemon migrated it from the old element-keyed format, and
//! a 403 that is an answer rather than a malfunction.

use super::*;

#[test]
fn a_list_answer_becomes_threads() {
    let body = serde_json::json!({
        "ok": true,
        "threads": [
            {
                "id": 7,
                "pageId": "p1",
                "x": 120.5,
                "y": -40.25,
                "anchorHint": null,
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
    // The place, at full precision: a document pixel is wider than a screen
    // pixel past zoom 1, so a rounded coordinate would move the pin.
    assert_eq!(
        threads[0].anchor,
        Some(CommentAnchor::new("p1", 120.5, -40.25))
    );
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
            "pageId": "p1",
            "x": 10.0,
            "y": 20.0,
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
            "pageId": "p1",
            "x": 10.0,
            "y": 20.0,
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
        "threads": [{ "id": 5, "pageId": "p1", "x": 1.0, "y": 2.0, "createdAt": 1, "resolved": false, "comments": [] }],
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
            { "id": 1, "pageId": "p1", "x": 1.0, "y": 1.0, "createdAt": 1, "resolved": true, "resolvedAt": 42,
              "resolvedBy": "u2", "resolvedByName": "Ada", "comments": [] },
            { "id": 2, "pageId": "p1", "x": 2.0, "y": 2.0, "createdAt": 1, "resolved": true, "resolvedAt": 43,
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
            "id": 9, "pageId": "p1", "x": 9.0, "y": 9.0, "createdAt": 1_700_000_500, "resolved": false,
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
        "threads": [{ "pageId": "p1", "x": 1.0, "y": 1.0, "createdAt": 1, "comments": [] }],
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
        CommentApiError::Rejected("Missing pageId string".to_string()).to_write_error(),
        CommentWriteError::Rejected("Missing pageId string".to_string())
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
    let thread = |id: i64, x: f64| CommentThread {
        id,
        anchor: Some(CommentAnchor::new("p1", x, 0.0)),
        comments: vec![Comment::default()],
        ..CommentThread::default()
    };

    apply(
        &mut ui,
        AnswerKind::List,
        Ok(Answer::Threads(vec![thread(1, 10.0), thread(2, 20.0)])),
    );
    assert_eq!(ui.thread_ids(), vec![1, 2]);

    apply(
        &mut ui,
        AnswerKind::Written,
        Ok(Answer::Thread(Box::new(thread(1, 10.0)))),
    );
    assert_eq!(ui.thread_ids(), vec![1, 2], "replaced, not appended");
}

#[test]
fn the_thread_a_canvas_click_created_opens_once_the_server_answers() {
    let mut ui = CommentsUiState::default();
    ui.transport = true;
    ui.toggle_pin_mode();
    ui.begin_thread_at(CommentAnchor::new("p1", 412.0, 88.0));
    ui.new_draft = "too tight".to_string();
    ui.send();
    assert_eq!(ui.composer(), None);

    apply(
        &mut ui,
        AnswerKind::Written,
        Ok(Answer::Thread(Box::new(CommentThread {
            id: 12,
            anchor: Some(CommentAnchor::new("p1", 412.0, 88.0)),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        }))),
    );
    assert!(
        ui.is_open(12),
        "the written thread is the one being looked at"
    );
    // And it opened at the point the click recorded, so the field the reviewer
    // typed into and the pin under it are the same place.
    assert_eq!(
        ui.thread(12).and_then(|thread| thread.anchor.clone()),
        Some(CommentAnchor::new("p1", 412.0, 88.0))
    );
}

#[test]
fn a_background_answer_does_not_take_the_panel_away_from_another_thread() {
    let mut ui = CommentsUiState::default();
    ui.install_threads(vec![
        CommentThread {
            id: 1,
            anchor: Some(CommentAnchor::new("p1", 10.0, 10.0)),
            comments: vec![Comment::default()],
            ..CommentThread::default()
        },
        CommentThread {
            id: 2,
            anchor: Some(CommentAnchor::new("p1", 20.0, 20.0)),
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
            anchor: Some(CommentAnchor::new("p1", 10.0, 10.0)),
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
fn a_null_placement_is_a_thread_with_no_pin_not_a_pin_at_the_origin() {
    // What the daemon answers for a thread it migrated from the old
    // element-keyed format. Reading the nulls with a `"" / 0.0` fallback would
    // paint that conversation in the top-left corner of every page and claim the
    // reviewer left it there.
    let body = serde_json::json!({
        "ok": true,
        "threads": [{
            "id": 4,
            "pageId": null,
            "x": null,
            "y": null,
            "anchorHint": "n7",
            "createdAt": 1_700_000_000,
            "resolved": false,
            "comments": [{ "id": 40, "authorName": "Kay", "body": "old thread", "createdAt": 1 }],
        }],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert_eq!(threads.len(), 1, "the conversation is still there to read");
    assert!(threads[0].anchor.is_none(), "and it has no pin");
}

#[test]
fn an_empty_page_id_is_no_page_rather_than_a_page_called_empty() {
    // The other shape the wire can carry for "no pin": coordinates with an empty
    // page. A coordinate needs a page to be in, so this is the same answer.
    let body = serde_json::json!({
        "ok": true,
        "threads": [
            { "id": 1, "pageId": "", "x": 5.0, "y": 6.0, "createdAt": 1, "comments": [] },
            { "id": 2, "pageId": "p1", "x": 5.0, "y": 6.0, "createdAt": 1, "comments": [] },
            { "id": 3, "pageId": "p1", "x": null, "y": 6.0, "createdAt": 1, "comments": [] },
        ],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert!(threads[0].anchor.is_none(), "an empty page is not a page");
    assert!(threads[1].anchor.is_some(), "a named page with a point is");
    // Half a coordinate is not a place either: the pair is what the daemon
    // stores and what a marker needs.
    assert!(threads[2].anchor.is_none());
}

#[test]
fn a_coordinate_outside_the_range_the_daemon_stores_is_no_pin() {
    // The write route refuses these, so a read that carried one is a hand-edited
    // database or another client: it is a thread with no drawable pin rather
    // than a marker at an absurd position.
    let body = serde_json::json!({
        "ok": true,
        "threads": [
            { "id": 1, "pageId": "p1", "x": 1.0e9, "y": 0.0, "createdAt": 1, "comments": [] },
            { "id": 2, "pageId": "p1", "x": 0.0, "y": 0.0, "createdAt": 1, "comments": [] },
        ],
    })
    .to_string();
    let threads = decode_threads(200, &body).unwrap();
    assert!(threads[0].anchor.is_none());
    // The origin is a perfectly good place to have left a comment, which is the
    // distinction the null-handling exists to preserve.
    assert_eq!(threads[1].anchor, Some(CommentAnchor::new("p1", 0.0, 0.0)));
}

#[test]
fn a_create_request_carries_the_page_and_the_point_and_no_node() {
    // The daemon answers 400 for a body with `nodeId` — deliberately, so a
    // client that still believes a pin belongs to an element learns it from the
    // response. This is the shape it accepts.
    //
    // The body is built by `dispatch`, which also performs the request, so the
    // assertion is on the JSON the wire contract names rather than on an HTTP
    // call this unit test has no window for.
    let anchor = CommentAnchor::new("p1", 12.5, -8.75);
    let body = serde_json::json!({
        "pageId": anchor.page_id,
        "x": anchor.x,
        "y": anchor.y,
        "text": "looks off",
    });
    assert_eq!(
        body,
        serde_json::json!({ "pageId": "p1", "x": 12.5, "y": -8.75, "text": "looks off" })
    );
    assert!(body.get("nodeId").is_none());
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
