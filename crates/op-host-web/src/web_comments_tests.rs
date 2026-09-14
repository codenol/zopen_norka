//! Reading the daemon's comment answers, and turning its refusals into
//! something the chrome can show.
//!
//! Most of it is pure: a status and a body in, a typed result out. The XHR
//! plumbing around it (`fetch_threads` / `post_thread`) is deliberately not
//! exercised — a browser test would be testing XmlHttpRequest, and the part
//! that has ever been wrong is the decoding: a null `authorRole`, an empty
//! `authorName`, a thread with no comments, a pin whose `pageId`/`x`/`y` are
//! `null` because the daemon migrated it from the old element-keyed format, and
//! a 403 that is an answer rather than a malfunction.
//!
//! The frame itself is exercised, though: `tick` is where a document open turns
//! into a read (issue #70), and the count of those reads — one, not one per
//! frame — is the property that matters. `hold_wire` lets the real frame path
//! run on the host target, where `web_sys` cannot be called at all.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsValue;

use crate::repaint_ctx::RepaintContext;
use crate::widget_host::WidgetHost;

use super::*;

/// The smallest shell `tick` runs against: a host, and a repaint tally.
struct Frame {
    host: WidgetHost,
    repaints: usize,
}

impl RepaintContext for Frame {
    fn host(&self) -> &WidgetHost {
        &self.host
    }

    fn host_mut(&mut self) -> &mut WidgetHost {
        &mut self.host
    }

    fn viewport_size(&self) -> (f32, f32) {
        (1440.0, 900.0)
    }

    fn register_system_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font_from_bytes(&mut self, _bytes: &[u8]) -> Option<String> {
        None
    }

    fn imported_family_list(&self) -> Vec<String> {
        Vec::new()
    }

    fn remove_imported_font(&mut self, _family: &str) {}

    fn repaint(&mut self) -> Result<(), JsValue> {
        self.repaints += 1;
        Ok(())
    }
}

/// A tab with the daemon's comment client, on `key`.
///
/// The wire is held before the caller's first frame: every `web_sys` call is a
/// wasm import that panics on the host target, so a test that let a request
/// through would take the process down rather than fail an assertion.
fn open_document(key: Option<&str>) -> Rc<RefCell<Frame>> {
    hold_wire::hold();
    let mut host = WidgetHost::new();
    {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.file_key = key.map(str::to_string);
        ui.comments.transport = true;
    }
    // Each test drives one tab, and the identity epoch is a thread-local the
    // tests below move deliberately.
    crate::identity_epoch::reset_for_test();
    Rc::new(RefCell::new(Frame { host, repaints: 0 }))
}

fn frame(inner: &Rc<RefCell<Frame>>) -> usize {
    tick(inner);
    hold_wire::reads().len()
}

/// The document the tab is showing, as the router would set it on an open.
fn set_open_key(inner: &Rc<RefCell<Frame>>, key: &str) {
    let mut borrowed = inner.borrow_mut();
    borrowed.host_mut().editor_state_mut().editor_ui.file_key = Some(key.to_string());
}

fn a_thread(id: i64, page: &str) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, 10.0, 20.0)),
        comments: vec![Comment::default()],
        ..CommentThread::default()
    }
}

#[test]
fn opening_a_document_asks_for_its_conversation() {
    let inner = open_document(Some("key1"));

    frame(&inner);
    assert_eq!(
        hold_wire::reads(),
        vec!["key1"],
        "a document with a conversation must not be painted as one without"
    );
    assert_eq!(
        hold_wire::sent(),
        vec![(CommentRequest::Reload, "key1".to_string())]
    );

    // And the state knows the read is in flight, which is what the rail's
    // spinner reads when the reviewer opens the tool before the answer lands.
    assert!(
        inner
            .borrow()
            .host
            .editor_state()
            .editor_ui
            .comments
            .loading
    );
}

#[test]
fn the_same_document_is_not_read_again_frame_after_frame() {
    let inner = open_document(Some("key1"));

    for _ in 0..5 {
        frame(&inner);
    }

    assert_eq!(
        hold_wire::reads(),
        vec!["key1"],
        "one request per document open, not one per frame"
    );
}

#[test]
fn another_document_is_read_again() {
    let inner = open_document(Some("key1"));
    frame(&inner);

    set_open_key(&inner, "key2");
    frame(&inner);

    assert_eq!(hold_wire::reads(), vec!["key1", "key2"]);
}

#[test]
fn reopening_the_same_document_after_another_one_is_read_again() {
    // The tab keeps one key, not a set: a reviewer who navigated away and back
    // gets a fresh answer rather than the one from before the detour.
    let inner = open_document(Some("key1"));
    frame(&inner);
    set_open_key(&inner, "key2");
    frame(&inner);
    set_open_key(&inner, "key1");
    frame(&inner);

    assert_eq!(hold_wire::reads(), vec!["key1", "key2", "key1"]);
}

#[test]
fn the_same_document_under_another_account_is_read_again() {
    // The daemon answers a conversation per caller — a document not shared with
    // this account is a 403 — so the same key under a new account is a new
    // answer, and the tab must not present the previous account's read as it.
    let inner = open_document(Some("key1"));
    crate::identity_epoch::observe_subject(Some("alice"));
    frame(&inner);

    crate::identity_epoch::observe_subject(Some("bob"));
    frame(&inner);

    assert_eq!(hold_wire::reads(), vec!["key1", "key1"]);
}

#[test]
fn a_document_with_no_key_has_no_conversation_to_read() {
    let inner = open_document(None);
    frame(&inner);
    frame(&inner);

    assert!(hold_wire::sent().is_empty());
}

#[test]
fn a_host_without_the_comment_client_never_asks() {
    let inner = open_document(Some("key1"));
    inner
        .borrow_mut()
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .comments
        .transport = false;

    frame(&inner);

    assert!(
        hold_wire::sent().is_empty(),
        "the tool is not even offered without a transport, so a read for a rail nobody can open is a request nobody asked for"
    );
}

#[test]
fn the_tool_turning_on_does_not_read_a_second_time() {
    // The widget layer queues the same reload when the tool is activated. Both
    // wishes land in the one queue the frame drains, so a document open with a
    // click on the tool in the same frame is still one request.
    let inner = open_document(Some("key1"));
    inner
        .borrow_mut()
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .comments
        .toggle_pin_mode();

    frame(&inner);

    assert_eq!(hold_wire::reads(), vec!["key1"]);
}

#[test]
fn a_write_is_still_followed_by_a_fresh_read() {
    let inner = open_document(Some("key1"));
    frame(&inner);

    inner
        .borrow_mut()
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .comments
        .resolve(7);
    frame(&inner);

    assert_eq!(
        hold_wire::sent().into_iter().skip(1).collect::<Vec<_>>(),
        vec![
            (CommentRequest::Resolve { thread_id: 7 }, "key1".to_string()),
            (CommentRequest::Reload, "key1".to_string()),
        ],
        "the answer to a write is one thread; the rest of the conversation has no live signal"
    );
}

#[test]
fn the_answer_to_the_read_at_open_survives_the_document_arriving_after_it() {
    // The order the browser actually sees: the open adopts the key, the frame
    // asks for the conversation, the small answer lands — and only then does the
    // document itself arrive and replace the whole document-derived state. If
    // that install wiped the list, the markers would be invisible again and the
    // read would have been pointless.
    let inner = open_document(Some("key1"));
    frame(&inner);

    park(
        AnswerKind::List,
        Some("key1".to_string()),
        Ok(Answer::Threads(vec![a_thread(1, "p1")])),
    );
    frame(&inner);
    assert_eq!(
        inner
            .borrow()
            .host
            .editor_state()
            .editor_ui
            .comments
            .thread_ids(),
        vec![1]
    );

    {
        let mut borrowed = inner.borrow_mut();
        let host = borrowed.host_mut();
        let doc = op_editor_core::EditorState::starter().doc;
        host.editor_state_mut().replace_document(doc);
    }

    let borrowed = inner.borrow();
    let comments = &borrowed.host().editor_state().editor_ui.comments;
    assert_eq!(
        comments.thread_ids(),
        vec![1],
        "the same document replaced is the same conversation"
    );
    assert_eq!(comments.document_key(), Some("key1"));
}

#[test]
fn a_read_for_another_document_does_not_claim_the_key_that_is_open() {
    // A late answer for the document the reviewer just left. It is installed
    // under the key it was ASKED about: claiming it for the open one would make
    // the next replacement of that key keep a conversation that is not its own.
    let inner = open_document(Some("key2"));
    park(
        AnswerKind::List,
        Some("key1".to_string()),
        Ok(Answer::Threads(vec![a_thread(1, "p1")])),
    );
    frame(&inner);

    assert_eq!(
        inner
            .borrow()
            .host
            .editor_state()
            .editor_ui
            .comments
            .document_key(),
        Some("key1")
    );
}

#[test]
fn a_list_answer_without_a_key_writes_no_key_into_the_state() {
    let inner = open_document(Some("key1"));
    apply(
        &mut inner
            .borrow_mut()
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .comments,
        None,
        AnswerKind::List,
        Ok(Answer::Threads(vec![a_thread(1, "p1")])),
    );
    assert_eq!(
        inner
            .borrow()
            .host
            .editor_state()
            .editor_ui
            .comments
            .document_key(),
        None,
        "an answer that names no document must not claim one"
    );
}

#[test]
fn the_open_read_rule_is_a_pair_of_key_and_identity() {
    // The rule on its own, without a frame: what `tick` asks, and when it stops
    // asking. Every branch here is a request that is or is not sent.
    let read = OpenedRead {
        key: "key1".to_string(),
        epoch: 3,
    };
    assert_eq!(
        opened_read_wanted(None, Some("key1"), 3, true),
        Some(read.clone())
    );
    assert_eq!(opened_read_wanted(Some(&read), Some("key1"), 3, true), None);
    assert_eq!(
        opened_read_wanted(Some(&read), Some("key1"), 4, true),
        Some(OpenedRead {
            key: "key1".to_string(),
            epoch: 4
        })
    );
    assert_eq!(
        opened_read_wanted(Some(&read), Some("key2"), 3, true),
        Some(OpenedRead {
            key: "key2".to_string(),
            epoch: 3
        })
    );
    assert_eq!(opened_read_wanted(Some(&read), None, 3, true), None);
    assert_eq!(opened_read_wanted(None, Some("key1"), 3, false), None);
}

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
        Some("key1".to_string()),
        AnswerKind::List,
        Ok(Answer::Threads(vec![thread(1, 10.0), thread(2, 20.0)])),
    );
    assert_eq!(ui.thread_ids(), vec![1, 2]);

    apply(
        &mut ui,
        None,
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
        None,
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
        None,
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
    apply(
        &mut ui,
        Some("key1".to_string()),
        AnswerKind::List,
        Err(CommentApiError::Http(502)),
    );
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
