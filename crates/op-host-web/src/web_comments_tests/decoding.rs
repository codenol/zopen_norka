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
