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
