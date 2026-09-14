//! The conversation routes against a real database: what a caller may do, what
//! comes back, and what a comment does not touch.
//!
//! The gate is proved beside the route table (`files_routes_access_tests`);
//! these drive whole requests through the daemon's entry point
//! ([`handle_web_canvas_request`]) against a store installed on the state, which
//! is the layer where a right, an owner and a row all have to agree.
//!
//! The store is installed on the state (`WebCanvasState::documents`) rather than
//! through `NORKA_DOCUMENTS_DIR` for the reason the file routes give: a test that
//! set the variable would race every other test in this binary that resolves a
//! directory.

use super::*;
use crate::document_store::DocumentEntry;
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::{
    handle_web_canvas_request, RequestAccess, ServeMode, WebCanvasState,
};
use op_editor_core::access::RoleSet;
use op_editor_core::EditorState;

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

fn error_code(reply: &WebReply) -> String {
    body_json(reply)["error"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

/// A verified account holding `roles`, whose display name is its id.
fn account(user_id: &str, roles: &[&str]) -> ResolvedIdentity {
    named_account(user_id, user_id, roles)
}

/// The same, with a name of its own — so what the route records can be told
/// apart from the id it keyed on.
fn named_account(user_id: &str, display_name: &str, roles: &[&str]) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: display_name.into(),
        roles: RoleSet::from_wire(roles),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

/// A stored document belonging to `owner`, with a real file behind it.
fn seed(store: &DocumentDb, name: &str, owner: Option<&str>) -> DocumentEntry {
    document_store::create_with(store, Some(name), owner, |path| {
        crate::doc_io::save_to_path(&op_pen_loader::new_skala_editor_state(), path)
            .map_err(|error| DocumentStoreError::Io(format!("save {}: {error}", path.display())))
    })
    .expect("seed a document")
}

/// The daemon state of the local operator, backed by `store`.
fn local_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    state
}

/// The daemon state of an online tenant, backed by `store`.
fn tenant_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(store.clone());
    state
}

/// The comments route of a document.
fn comments(entry: &DocumentEntry) -> String {
    format!("/api/files/{}/comments", entry.key)
}

/// The id a thread reply carried.
fn thread_id(reply: &WebReply) -> i64 {
    body_json(reply)["thread"]["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("no thread id in {}", reply.body))
}

/// The bodies of one thread's comments, in order.
fn bodies(thread: &serde_json::Value) -> Vec<String> {
    thread["comments"]
        .as_array()
        .unwrap_or_else(|| panic!("no comments array in {thread}"))
        .iter()
        .map(|comment| {
            comment["body"]
                .as_str()
                .unwrap_or_else(|| panic!("no body in {comment}"))
                .to_string()
        })
        .collect()
}

#[test]
fn a_documents_conversation_is_opened_answered_closed_and_read_back() {
    let dir = TempDir::new("comment-routes");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);

    // A document nobody has commented on answers an empty list, not a 404: the
    // document is there, and "nothing said yet" is a state a panel draws.
    let none_yet = handle_web_canvas_request("GET", &route, "", &mut state, &access);
    assert_eq!(none_yet.status, "200 OK", "{}", none_yet.body);
    assert_eq!(
        body_json(&none_yet)["threads"].as_array().map(Vec::len),
        Some(0)
    );

    // Opening a thread at a point on a page. The coordinates are the PAGE's,
    // not the screen's, so they are what the client measured in the document.
    let opened = handle_web_canvas_request(
        "POST",
        &route,
        r#"{"pageId":"page-1","x":120.5,"y":-40.25,"text":"  Fix the padding  "}"#,
        &mut state,
        &access,
    );
    assert_eq!(opened.status, "200 OK", "{}", opened.body);
    let thread = body_json(&opened)["thread"].clone();
    assert_eq!(thread["pageId"], "page-1");
    assert_eq!(thread["x"], 120.5);
    assert_eq!(thread["y"], -40.25);
    assert_eq!(
        thread.get("nodeId"),
        None,
        "the element the pin used to sit on is not part of the answer any more"
    );
    assert_eq!(
        thread["anchorHint"],
        serde_json::Value::Null,
        "a comment placed now points at no element"
    );
    assert_eq!(thread["resolved"], false);
    assert_eq!(thread["resolvedAt"], serde_json::Value::Null);
    assert_eq!(
        bodies(&thread),
        vec!["Fix the padding"],
        "the text is trimmed"
    );
    // The local daemon has no accounts: nobody is named, and nothing is
    // invented on their behalf.
    assert_eq!(thread["comments"][0]["authorId"], serde_json::Value::Null);
    assert_eq!(thread["comments"][0]["authorName"], "");
    let id = thread_id(&opened);

    // Answering it: the whole thread comes back, so a client that just showed
    // the conversation can replace what it holds.
    let replied = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/reply"),
        r#"{"text":"Done"}"#,
        &mut state,
        &access,
    );
    assert_eq!(replied.status, "200 OK", "{}", replied.body);
    assert_eq!(thread_id(&replied), id);
    assert_eq!(
        bodies(&body_json(&replied)["thread"]),
        vec!["Fix the padding", "Done"]
    );

    // The list carries the same thread, with both comments in order.
    let listed = handle_web_canvas_request("GET", &route, "", &mut state, &access);
    let threads = body_json(&listed)["threads"].clone();
    assert_eq!(threads.as_array().map(Vec::len), Some(1));
    assert_eq!(threads[0]["pageId"], "page-1");
    assert_eq!(threads[0]["x"], 120.5);
    assert_eq!(threads[0]["y"], -40.25);
    assert_eq!(bodies(&threads[0]), vec!["Fix the padding", "Done"]);

    // Closed, and opened again.
    let closed = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/resolve"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(body_json(&closed)["thread"]["resolved"], true);
    assert_eq!(
        body_json(&closed)["thread"]["pageId"], "page-1",
        "a closed thread is still a comment somebody has to find on the canvas"
    );
    assert_eq!(
        body_json(&closed)["thread"]["comments"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let reopened = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/reopen"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(body_json(&reopened)["thread"]["resolved"], false);
    let listed = handle_web_canvas_request("GET", &route, "", &mut state, &access);
    assert_eq!(body_json(&listed)["threads"][0]["resolved"], false);
}

#[test]
fn a_comment_is_not_part_of_the_document() {
    // The property that makes this a conversation ABOUT a document rather than
    // a change to it: a thread can be opened, answered and closed without the
    // document's version moving, its content changing, its file being written
    // or its row being touched. Everything a save would move is checked here,
    // because a comment that bumped any of it would show up as an unsaved
    // change and invalidate a collaboration hash.
    let dir = TempDir::new("comment-not-document");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);

    let path = document_store::path_for(store.dir(), &entry.key).expect("path");
    let version_before = state.version;
    let document_before = serde_json::to_string(&state.editor.doc).expect("serialize");
    let file_before = std::fs::read(&path).expect("read the document");
    let row_before = document_store::find(&store, &entry.key)
        .expect("find")
        .expect("the row");

    let opened = handle_web_canvas_request(
        "POST",
        &route,
        r#"{"pageId":"page-1","x":12,"y":34,"text":"hello"}"#,
        &mut state,
        &access,
    );
    assert_eq!(opened.status, "200 OK", "{}", opened.body);
    let id = thread_id(&opened);
    assert_eq!(
        handle_web_canvas_request(
            "POST",
            &format!("{route}/{id}/reply"),
            r#"{"text":"there"}"#,
            &mut state,
            &access,
        )
        .status,
        "200 OK"
    );
    assert_eq!(
        handle_web_canvas_request(
            "POST",
            &format!("{route}/{id}/resolve"),
            "",
            &mut state,
            &access,
        )
        .status,
        "200 OK"
    );

    assert_eq!(
        state.version, version_before,
        "the document version did not move"
    );
    assert_eq!(
        serde_json::to_string(&state.editor.doc).expect("serialize"),
        document_before,
        "the document's content is unchanged"
    );
    assert_eq!(
        std::fs::read(&path).expect("read the document"),
        file_before,
        "and its file was not written"
    );
    let row_after = document_store::find(&store, &entry.key)
        .expect("find")
        .expect("the row");
    assert_eq!(row_after.updated_at, row_before.updated_at);
    assert_eq!(row_after.size, row_before.size);
    assert_eq!(row_after.name, row_before.name);

    // And the version the browser polls says the same thing.
    let polled = handle_web_canvas_request("GET", "/api/mcp/version", "", &mut state, &access);
    assert_eq!(body_json(&polled)["version"], version_before);
}

#[test]
fn a_contributor_opens_a_thread_and_may_not_write_the_document() {
    // The split `DocumentAction::Comment` exists for, through the routes.
    let dir = TempDir::new("comment-contributor");
    let store = dir.open();
    let entry = seed(&store, "Shared", Some("userA"));
    let mut state = tenant_state(&store);
    let analyst = named_account("userB", "Boris", &["analyst"]);
    let access = RequestAccess::online("userA", &analyst, true);
    let route = comments(&entry);

    let opened = handle_web_canvas_request(
        "POST",
        &route,
        r#"{"pageId":"page-1","x":12,"y":34,"text":"hello"}"#,
        &mut state,
        &access,
    );
    assert_eq!(opened.status, "200 OK", "{}", opened.body);
    let thread = body_json(&opened)["thread"].clone();
    assert_eq!(thread["comments"][0]["authorId"], "userB");
    assert_eq!(
        thread["comments"][0]["authorName"], "Boris",
        "the name recorded is the verified identity's, not the body's"
    );

    // The same caller, the same key: the document itself is out of reach.
    for (method, path, body) in [
        (
            "POST",
            format!("/api/files/{}/save", entry.key),
            "{}".to_string(),
        ),
        (
            "POST",
            format!("/api/files/{}/rename", entry.key),
            r#"{"name":"x"}"#.to_string(),
        ),
        ("DELETE", format!("/api/files/{}", entry.key), String::new()),
    ] {
        let reply = handle_web_canvas_request(method, &path, &body, &mut state, &access);
        assert_eq!(
            reply.status, "403 Forbidden",
            "{method} {path}: {}",
            reply.body
        );
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
}

#[test]
fn a_guest_given_a_link_to_read_is_refused_the_conversation() {
    // The operator's rule for a guest: read, and nothing else. Reading the
    // comments is reading the document — writing one is not.
    let dir = TempDir::new("comment-guest");
    let store = dir.open();
    let entry = seed(&store, "Shared", Some("userA"));
    let mut state = tenant_state(&store);
    let guest = account("userB", &[]);
    let access = RequestAccess::online("userA", &guest, true);
    let route = comments(&entry);

    let read = handle_web_canvas_request("GET", &route, "", &mut state, &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);
    assert_eq!(
        body_json(&read)["threads"].as_array().map(Vec::len),
        Some(0)
    );

    for (method, path, body) in [
        (
            "POST",
            route.clone(),
            r#"{"pageId":"page-1","x":12,"y":34,"text":"hello"}"#.to_string(),
        ),
        (
            "POST",
            format!("{route}/1/reply"),
            r#"{"text":"hello"}"#.to_string(),
        ),
        ("POST", format!("{route}/1/resolve"), String::new()),
        ("POST", format!("{route}/1/reopen"), String::new()),
    ] {
        let reply = handle_web_canvas_request(method, &path, &body, &mut state, &access);
        assert_eq!(
            reply.status, "403 Forbidden",
            "{method} {path}: {}",
            reply.body
        );
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }
    // Refused, and nothing was written.
    assert_eq!(
        document_comments::list_threads(&store, &entry.key).expect("list"),
        Some(Vec::new())
    );
}

#[test]
fn a_stranger_cannot_reach_a_conversation_by_key() {
    // The same question the file routes ask, asked of the conversation: a key
    // names a row in a directory every account shares, and a caller who may not
    // address the document may not read or write what is pinned to it.
    let dir = TempDir::new("comment-stranger");
    let store = dir.open();
    let theirs = seed(&store, "Theirs", Some("userB"));
    let mut state = tenant_state(&store);
    let stranger = account("userA", &["admin"]);
    let access = RequestAccess::online("userA", &stranger, false);
    let route = comments(&theirs);

    for (method, path, body) in [
        ("GET", route.clone(), String::new()),
        (
            "POST",
            route.clone(),
            r#"{"pageId":"page-1","x":12,"y":34,"text":"hello"}"#.to_string(),
        ),
        (
            "POST",
            format!("{route}/1/reply"),
            r#"{"text":"hello"}"#.to_string(),
        ),
        ("POST", format!("{route}/1/resolve"), String::new()),
    ] {
        let reply = handle_web_canvas_request(method, &path, &body, &mut state, &access);
        assert_eq!(
            reply.status, "403 Forbidden",
            "{method} {path}: {}",
            reply.body
        );
        // The administrator role changed nothing: reach is about whose
        // document it is, and roles never decide what may be seen.
        assert_eq!(error_code(&reply), "tenant-not-shared", "{method} {path}");
    }
}

#[test]
fn a_thread_is_closed_by_its_author_or_by_whoever_may_edit_the_document() {
    let dir = TempDir::new("comment-resolve-rights");
    let store = dir.open();
    let entry = seed(&store, "Shared", Some("userA"));
    let mut state = tenant_state(&store);
    let route = comments(&entry);

    // An analyst opens a thread: it is theirs to close.
    let analyst = named_account("userB", "Boris", &["analyst"]);
    let author = RequestAccess::online("userA", &analyst, true);
    let opened = handle_web_canvas_request(
        "POST",
        &route,
        r#"{"pageId":"page-1","x":12,"y":34,"text":"hello"}"#,
        &mut state,
        &author,
    );
    assert_eq!(opened.status, "200 OK", "{}", opened.body);
    let id = thread_id(&opened);

    // Another analyst may answer in it and may not close it: they may take part
    // in the conversation, and the thread is not theirs.
    let other = named_account("userC", "Vera", &["qa"]);
    let peer = RequestAccess::online("userA", &other, true);
    let replied = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/reply"),
        r#"{"text":"looking"}"#,
        &mut state,
        &peer,
    );
    assert_eq!(replied.status, "200 OK", "{}", replied.body);
    let refused = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/resolve"),
        "",
        &mut state,
        &peer,
    );
    assert_eq!(refused.status, "403 Forbidden", "{}", refused.body);
    assert_eq!(error_code(&refused), "read-only-role");
    // …and the refusal is only about closing: the reply they wrote is there.
    assert_eq!(
        body_json(&handle_web_canvas_request(
            "GET", &route, "", &mut state, &peer
        ))["threads"][0]["comments"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    // The author closes their own.
    let closed = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/resolve"),
        "",
        &mut state,
        &author,
    );
    assert_eq!(closed.status, "200 OK", "{}", closed.body);
    assert_eq!(body_json(&closed)["thread"]["resolved"], true);
    assert_eq!(body_json(&closed)["thread"]["resolvedBy"], "userB");
    assert_eq!(body_json(&closed)["thread"]["resolvedByName"], "Boris");

    // An editor closes anyone's — which is what triaging a review is.
    let designer = named_account("userD", "Dina", &["ux_ui"]);
    let editor = RequestAccess::online("userA", &designer, true);
    let reopened = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/reopen"),
        "",
        &mut state,
        &editor,
    );
    assert_eq!(reopened.status, "200 OK", "{}", reopened.body);
    assert_eq!(body_json(&reopened)["thread"]["resolved"], false);
    let closed_again = handle_web_canvas_request(
        "POST",
        &format!("{route}/{id}/resolve"),
        "",
        &mut state,
        &editor,
    );
    assert_eq!(closed_again.status, "200 OK", "{}", closed_again.body);
    assert_eq!(body_json(&closed_again)["thread"]["resolvedBy"], "userD");
}

#[test]
fn a_thread_this_document_does_not_have_is_not_found() {
    let dir = TempDir::new("comment-missing-thread");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);

    for (path, body) in [
        (format!("{route}/9999/reply"), r#"{"text":"hello"}"#),
        (format!("{route}/9999/resolve"), ""),
        (format!("{route}/9999/reopen"), ""),
    ] {
        let reply = handle_web_canvas_request("POST", &path, body, &mut state, &access);
        assert_eq!(reply.status, "404 Not Found", "{path}: {}", reply.body);
        // Its own wording: the document was found — it is the row the key
        // named — and what is missing is the thread the path asked for.
        assert_eq!(error_code(&reply), "No such comment thread", "{path}");
    }

    // The same routes on a key that names no document at all, which is a
    // different statement about a different thing.
    let absent = "aaaaaaaa00000009";
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/files/{absent}/comments"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(reply.status, "404 Not Found", "{}", reply.body);
    assert_eq!(error_code(&reply), "document not found");
}

#[test]
fn a_request_the_route_cannot_read_is_refused_with_a_reason() {
    let dir = TempDir::new("comment-bad-requests");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);

    let long_text = "x".repeat(document_comments::MAX_COMMENT_CHARS + 1);
    let long_page = "p".repeat(document_comments::MAX_PAGE_ID_CHARS + 1);
    let cases: &[(&str, String, &str)] = &[
        (&route, String::new(), "Expected a JSON object"),
        (&route, "{ not json".to_string(), "Expected a JSON object"),
        (
            &route,
            serde_json::json!({ "pageId": "page-1", "x": 1, "y": 2 }).to_string(),
            "Missing text string",
        ),
        (
            &route,
            serde_json::json!({ "pageId": "page-1", "x": 1, "y": 2, "text": "   " }).to_string(),
            "Missing text string",
        ),
        // A pin needs all three: the same two numbers exist on every page, so
        // coordinates with no page are not a place.
        (
            &route,
            serde_json::json!({ "x": 1, "y": 2, "text": "hi" }).to_string(),
            "Missing pageId string",
        ),
        (
            &route,
            serde_json::json!({ "pageId": "  ", "x": 1, "y": 2, "text": "hi" }).to_string(),
            "Missing pageId string",
        ),
        (
            &route,
            serde_json::json!({ "pageId": long_page, "x": 1, "y": 2, "text": "hi" }).to_string(),
            "pageId is longer than 128 characters",
        ),
        (
            &route,
            serde_json::json!({ "pageId": "page-1", "x": "left", "y": 2, "text": "hi" })
                .to_string(),
            "x must be a number",
        ),
        // `null` reads as "no value", which is a missing coordinate rather than
        // a place at zero.
        (
            &route,
            serde_json::json!({ "pageId": "page-1", "x": 1, "y": null, "text": "hi" })
                .to_string(),
            "y must be a number",
        ),
        // A number too large for an `f64` never reaches the coordinate check:
        // serde_json refuses the literal, so the body is not JSON at all. Kept
        // here because it is the shape of the answer a caller sending one gets,
        // and because it is what makes `NotACoordinate::NotFinite` a guard
        // against this server's own future rather than against the wire.
        (
            &route,
            r#"{"pageId":"page-1","x":1e400,"y":2,"text":"hi"}"#.to_string(),
            "Expected a JSON object",
        ),
        // Past the bound is refused, not clamped: a client that sent this has a
        // bug, and moving its pin to the edge of the world would hide the bug
        // behind a comment that is now in the wrong place.
        (
            &route,
            serde_json::json!({
                "pageId": "page-1",
                "x": document_comments::MAX_COORDINATE * 2.0,
                "y": 2,
                "text": "hi"
            })
            .to_string(),
            "x is further from the origin than 10000000",
        ),
        (
            &route,
            serde_json::json!({
                "pageId": "page-1",
                "x": 1,
                "y": -document_comments::MAX_COORDINATE - 1.0,
                "text": "hi"
            })
            .to_string(),
            "y is further from the origin than 10000000",
        ),
        // The retired field, and the one place a client migrating to
        // coordinates is told what changed instead of being left to infer it.
        (
            &route,
            serde_json::json!({
                "nodeId": "n1", "pageId": "page-1", "x": 1, "y": 2, "text": "hi"
            })
            .to_string(),
            "nodeId is no longer accepted: place a comment with pageId, x and y",
        ),
        (
            &route,
            serde_json::json!({ "nodeId": "n1", "text": "hi" }).to_string(),
            "nodeId is no longer accepted: place a comment with pageId, x and y",
        ),
        (
            &route,
            serde_json::json!({ "pageId": "page-1", "x": 1, "y": 2, "text": long_text })
                .to_string(),
            "Comment text is longer than 4000 characters",
        ),
        (
            &format!("{route}/not-a-number/reply"),
            r#"{"text":"hi"}"#.to_string(),
            "Invalid comment thread id",
        ),
        (
            &format!("{route}/0/reply"),
            r#"{"text":"hi"}"#.to_string(),
            "Invalid comment thread id",
        ),
        (
            &format!("{route}/9999/reply"),
            String::new(),
            "Expected a JSON object",
        ),
    ];
    for (path, body, expected) in cases {
        let reply = handle_web_canvas_request("POST", path, body, &mut state, &access);
        assert_eq!(reply.status, "400 Bad Request", "{path}: {}", reply.body);
        assert_eq!(error_code(&reply), *expected, "{path}");
    }
    // None of them wrote anything: a refused request leaves no thread, and no
    // half-written pin either.
    assert_eq!(
        document_comments::list_threads(&store, &entry.key).expect("list"),
        Some(Vec::new())
    );
}

#[test]
fn the_edges_of_a_pin_are_accepted_and_what_is_past_them_is_not() {
    // The bound is a real number in a real answer, so it is pinned on both
    // sides: the far corner is a place somebody may legitimately work, and one
    // step past it is not. `MAX_COORDINATE` itself must be accepted — a limit
    // that is exclusive is a limit that is off by one for whoever reads it.
    let dir = TempDir::new("comment-pin-bounds");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);
    let bound = document_comments::MAX_COORDINATE;

    for (x, y) in [(bound, -bound), (0.0, 0.0), (-0.5, 0.5)] {
        let body = serde_json::json!({
            "pageId": "page-1", "x": x, "y": y, "text": "here"
        })
        .to_string();
        let reply = handle_web_canvas_request("POST", &route, &body, &mut state, &access);
        assert_eq!(reply.status, "200 OK", "{body}: {}", reply.body);
        assert_eq!(body_json(&reply)["thread"]["x"], x);
        assert_eq!(body_json(&reply)["thread"]["y"], y);
    }
}

#[test]
fn a_coordinate_that_is_not_a_place_is_refused_whatever_the_wire_can_carry() {
    // NaN and the infinities are the values JSON has no literal for, so a body
    // cannot carry them — the check is here anyway, on the function rather than
    // through a request, because "the store never holds a coordinate that
    // cannot be drawn" should not depend on somebody else's number parsing. The
    // wire-level half of the same case is in
    // `a_request_the_route_cannot_read_is_refused_with_a_reason`: an integer
    // too large for the bound is refused there.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let refused = coordinate(value, Axis::X);
        assert!(
            matches!(
                refused,
                Err(CommentRequestError::BadCoordinate {
                    axis: Axis::X,
                    why: NotACoordinate::NotFinite
                })
            ),
            "{value} was not refused as a place: {refused:?}"
        );
    }
    // And the finite ones the bound allows go through, unchanged: no rounding
    // on the way in.
    assert_eq!(coordinate(12.5, Axis::Y), Ok(12.5));
    assert_eq!(coordinate(-0.0, Axis::Y), Ok(-0.0));
}

#[test]
fn a_thread_placed_before_pins_were_coordinates_is_listed_without_one() {
    // What migration 3 leaves in a database that already had conversations: a
    // thread with an element hint and no coordinates. The route has to answer
    // it as a thread — its conversation is real and a panel has to show it —
    // with `pageId: null`, which is the client's signal that this one has no
    // pin to draw. The alternative, hiding it, would delete a review from a
    // list because the schema under it moved.
    let dir = TempDir::new("comment-legacy-thread");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let route = comments(&entry);

    store
        .conn()
        .execute(
            "INSERT INTO comment_threads (document_key, anchor_hint, created_at, resolved)
             VALUES (?1, 'n7', 7, 0)",
            rusqlite::params![entry.key],
        )
        .expect("a thread in the migrated shape");
    store
        .conn()
        .execute(
            "INSERT INTO comments (thread_id, author_id, author_name, body, created_at)
             SELECT id, 'userA', 'Anya', 'from the old build', 7
               FROM comment_threads WHERE document_key = ?1",
            rusqlite::params![entry.key],
        )
        .expect("its comment");

    let listed = handle_web_canvas_request("GET", &route, "", &mut state, &access);
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    let thread = body_json(&listed)["threads"][0].clone();
    assert_eq!(thread["pageId"], serde_json::Value::Null);
    assert_eq!(thread["x"], serde_json::Value::Null);
    assert_eq!(thread["y"], serde_json::Value::Null);
    assert_eq!(thread["anchorHint"], "n7");
    assert_eq!(bodies(&thread), vec!["from the old build"]);
}

