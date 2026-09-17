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
