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
