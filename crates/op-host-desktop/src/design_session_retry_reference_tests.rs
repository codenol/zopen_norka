//! The manual subtask "Retry" button and the turn's reference picture
//! (issue #95).
//!
//! A design turn grounded on a reference screenshot stores its request on the
//! turn's assistant bubble so `launch_subtask_retry_if_pending` can re-run one
//! failed section. That stash is JSON and `DesignRequest::reference_attachments`
//! is `#[serde(skip)]`, so the restored request is blind by construction. The
//! bytes still exist once, on the turn's user bubble (`begin_send` copies the
//! staged images there), and these tests pin the restore that reads them back:
//! a retried section is generated against the picture the rest of the design was
//! built from, and a turn sent without one gains nothing.

use super::*;
use op_editor_core::{ChatImage, ChatMessage, EditorState};
use op_host_native::WidgetHostNative;

/// A stashed request in the shape the retry stash produces: JSON, and therefore
/// without the two `#[serde(skip)]` reference fields.
fn stashed_request_json() -> String {
    serde_json::to_string(&op_orchestrator::DesignRequest {
        prompt: "make it like this".into(),
        concurrency: 1,
        ..Default::default()
    })
    .expect("a design request serializes")
}

fn host_with_turn(images: Vec<ChatImage>) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let mut user = ChatMessage::user("make it like this");
    user.images = images;
    host.editor_state_mut().chat.messages.push(user);
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant("designing"));
    host
}

fn png_bytes() -> Vec<u8> {
    vec![0x89, b'P', b'N', b'G', 7]
}

#[test]
fn a_retry_restores_the_turns_reference_image_from_its_user_bubble() {
    let host = host_with_turn(vec![ChatImage {
        id: 1,
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: png_bytes(),
    }]);
    // The assistant bubble is index 1; a retry click is keyed on it (or on one
    // of the turn's worker bubbles, which sit after it).
    let assistant_idx = 1;

    let mut request: op_orchestrator::DesignRequest =
        serde_json::from_str(&stashed_request_json()).expect("stashed request parses");
    assert!(
        request.reference_attachments.is_empty(),
        "the fixture must start from the blind state the stash produces"
    );

    restore_turn_reference_attachments(&host, assistant_idx, &mut request);

    assert_eq!(
        request.reference_attachments.len(),
        1,
        "the retried section must be generated against the turn's picture"
    );
    assert_eq!(request.reference_attachments[0].name, "reference.png");
    assert_eq!(request.reference_attachments[0].media_type, "image/png");
    assert_eq!(
        request.reference_attachments[0].data,
        png_bytes(),
        "the restored bytes must be the attached picture's own"
    );
}

/// The same restore reached from the retry's own message index: a worker bubble
/// of the turn is not the user bubble, so "nearest preceding user message" is
/// what has to resolve, not "the message before this one".
#[test]
fn a_worker_bubbles_retry_finds_its_turns_picture() {
    let mut host = host_with_turn(vec![ChatImage {
        id: 1,
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: png_bytes(),
    }]);
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant_streaming());
    let worker_idx = 2;

    let mut request: op_orchestrator::DesignRequest =
        serde_json::from_str(&stashed_request_json()).expect("stashed request parses");
    restore_turn_reference_attachments(&host, worker_idx, &mut request);

    assert_eq!(request.reference_attachments.len(), 1);
}

#[test]
fn a_retry_of_a_turn_without_a_picture_gains_nothing() {
    let host = host_with_turn(Vec::new());

    let mut request: op_orchestrator::DesignRequest =
        serde_json::from_str(&stashed_request_json()).expect("stashed request parses");
    restore_turn_reference_attachments(&host, 1, &mut request);

    assert!(
        request.reference_attachments.is_empty(),
        "a turn with no attached picture must not be handed one"
    );
}

/// A request that already carries its reference keeps it: the restore fills a
/// hole and never overwrites what the stash (or a future stash) supplied.
#[test]
fn a_retry_with_its_own_reference_is_left_alone() {
    let host = host_with_turn(vec![ChatImage {
        id: 1,
        name: "from-the-bubble.png".into(),
        media_type: "image/png".into(),
        data: vec![1],
    }]);
    let mut request: op_orchestrator::DesignRequest =
        serde_json::from_str(&stashed_request_json()).expect("stashed request parses");
    request.reference_attachments = vec![op_orchestrator::ReferenceAttachment {
        name: "from-the-request.png".into(),
        media_type: "image/png".into(),
        data: vec![2],
    }];

    restore_turn_reference_attachments(&host, 1, &mut request);

    assert_eq!(request.reference_attachments.len(), 1);
    assert_eq!(request.reference_attachments[0].name, "from-the-request.png");
}

/// The chat layer's own answer to "which pictures does the turn `msg_idx`
/// belongs to carry", on an index that is out of range — a stale retry click
/// against a pruned transcript must not panic.
#[test]
fn a_stale_message_index_yields_no_pictures() {
    let chat = op_editor_core::ChatState::default();
    assert!(chat.owning_turn_images(7).is_empty());
    let _ = EditorState::new();
}
