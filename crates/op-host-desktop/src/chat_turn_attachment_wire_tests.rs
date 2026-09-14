//! Wire-level attachment tests for the desktop launch path (issue #64).
//!
//! These drive the REAL entry point (`launch_if_pending`) against a loopback
//! HTTP endpoint that stands in for the model provider, and assert on the body
//! the provider itself posted. That is deliberate: the defect these tests
//! close (`pending_attachments` drained into the request of a route the turn
//! never took) shipped with every unit green, because no test ever looked at
//! what left the process — the same lesson issue #61 taught for the transport
//! layer. A helpful side effect of driving the launch path is that the
//! assertion cannot be satisfied by helper plumbing alone: the bytes have to
//! travel from `chat.pending_attachments` through the routing decision to the
//! HTTP body.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use op_editor_core::chat::ChatAttachment;
use op_editor_core::{
    AgentProvider, BuiltinAgentConfig, BuiltinAgentKind, BuiltinAgentPresetKey, EditorState,
    ModelEntry,
};

use super::*;

/// A PNG signature plus a few payload bytes. The transport picks images for
/// inline delivery by SNIFFING BYTES, not by trusting the media-type label, so
/// a fixture without a real signature would silently test the text path.
const PNG_BYTES: [u8; 14] = [
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, b'n', b'o', b'r', b'k', b'a', 0x01,
];

const PNG_BASE64: &str = "iVBORw0KGgpub3JrYQE=";
const INLINE_PREFIX: &str = "data:image/png;base64,";

fn png_attachment() -> ChatAttachment {
    ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: PNG_BYTES.to_vec(),
    }
}

/// A frame fixture for the modify route's target (a design the turn can edit
/// in place).
fn frame(
    id: &str,
    name: &str,
    children: Vec<jian_ops_schema::node::PenNode>,
) -> jian_ops_schema::node::PenNode {
    use op_editor_core::PenNodeExt;
    let mut node: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame",
        "id": id,
        "name": name,
        "width": 390,
        "height": 120,
        "children": []
    }))
    .expect("frame fixture");
    if let Some(kids) = node.children_mut() {
        *kids = children;
    }
    node
}

/// Read one HTTP request (headers + `content-length` body) off `stream`.
fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let n = stream.read(&mut chunk).expect("read request");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]);
        let content_len = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if buf.len() >= header_end + 4 + content_len {
            break;
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn request_body(request: &str) -> String {
    let body_start = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .expect("request body separator");
    // A binary-safe substring: the wire body is JSON, and the assertions below
    // look for markers inside it rather than parsing provider-specific shapes.
    request[body_start..].to_string()
}

/// A loopback endpoint that records up to `max_requests` bodies and answers
/// each with an empty SSE stream, so the provider closes the turn by itself.
/// The thread is deliberately left detached: it blocks on `accept` once the
/// test is done, which costs nothing at process exit.
fn capture_server(max_requests: usize) -> (String, std_mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture server address");
    let (body_tx, body_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        for _ in 0..max_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let request = read_http_request(&mut stream);
            let _ = body_tx.send(request_body(&request));
            let response = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                            content-length: 0\r\nconnection: close\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{addr}/v1"), body_rx)
}

/// Receive bodies until one satisfies `matches`, or the timeout runs out.
/// Returns every body seen, so a failure can show what the provider actually
/// got instead of only that nothing matched.
fn wait_for_body(
    rx: &std_mpsc::Receiver<String>,
    matches: impl Fn(&str) -> bool,
    timeout: Duration,
) -> Result<String, Vec<String>> {
    let deadline = std::time::Instant::now() + timeout;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(seen);
        }
        match rx.recv_timeout(left) {
            Ok(body) if matches(&body) => return Ok(body),
            Ok(body) => seen.push(body),
            Err(_) => return Err(seen),
        }
    }
}

/// A host whose selected model is a built-in (API-key) agent pointed at the
/// capture server, so the launch path builds a REAL provider that posts there.
fn host_with_builtin_provider(base_url: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.editor_state_mut()
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(BuiltinAgentConfig {
            id: "capture".into(),
            preset: BuiltinAgentPresetKey::Custom,
            display_name: "Capture".into(),
            kind: BuiltinAgentKind::OpenAiCompat,
            api_key: "sk-test".into(),
            models: vec!["capture-model".into()],
            base_url: base_url.to_string(),
            enabled: true,
        });
    host.editor_state_mut().chat.available_models = vec![ModelEntry::builtin(
        AgentProvider::ClaudeCode,
        "capture",
        "builtin:capture:capture-model",
        "Capture",
    )];
    host.editor_state_mut().chat.selected_model = 0;
    host
}

fn stage_image(host: &mut WidgetHostNative) {
    assert!(
        host.editor_state_mut()
            .chat
            .add_attachment(png_attachment()),
        "the fixture image must be staged for the turn"
    );
}

fn send(host: &mut WidgetHostNative, text: &str) {
    host.editor_state_mut().chat.set_input_text(text);
    assert!(
        host.editor_state_mut().chat.begin_send(),
        "the turn must be queued"
    );
}

fn launch(host: &mut WidgetHostNative) -> (bool, Option<ChatSession>, Option<DesignSession>) {
    let mut current_chat = None;
    let mut current_design = None;
    let launched = launch_if_pending(host, &mut current_chat, &mut current_design);
    (launched, current_chat, current_design)
}

fn assistant_text(host: &WidgetHostNative) -> String {
    host.editor_state()
        .chat
        .messages
        .last()
        .expect("the turn pushed an assistant bubble")
        .content
        .clone()
}

// ── modify route ────────────────────────────────────────────────────────────

/// The reported defect, at the wire: a modify turn used to build its request
/// with `..Default::default()`, so the attached picture had no path into it and
/// the model edited the selection blind while the person watched their
/// attachment disappear.
#[test]
fn a_modify_turn_posts_the_attached_image_instead_of_dropping_it() {
    let (base_url, bodies) = capture_server(6);
    let mut host = host_with_builtin_provider(&base_url);
    // A canvas with a real target: `should_launch_direct_modify` routes
    // modify-shaped wording on a selected element down this path.
    host.editor_state_mut().active_children_mut().clear();
    host.editor_state_mut().active_children_mut().push(frame(
        "screen",
        "Food App Home",
        vec![frame("popular-card", "Bella Napoli Pizzeria", Vec::new())],
    ));
    host.editor_state_mut()
        .set_single_selection(op_editor_core::NodeId::new("popular-card"));
    stage_image(&mut host);
    send(&mut host, "修改成饺子");

    let (launched, current_chat, _current_design) = launch(&mut host);

    assert!(launched, "the modify route must launch a turn");
    assert!(
        current_chat.is_some(),
        "the selection-scoped modify turn parks a chat session"
    );
    assert!(
        host.editor_state().chat.pending_attachments.is_empty(),
        "the turn's attachments are drained exactly once — the drain is what \
         used to happen too early for the other routes"
    );
    let body = wait_for_body(
        &bodies,
        |b| b.contains(INLINE_PREFIX),
        Duration::from_secs(15),
    )
    .unwrap_or_else(|seen| {
        panic!(
            "the modify turn must post the attached image, got {} body/bodies: {seen:?}",
            seen.len()
        )
    });
    assert!(
        body.contains(PNG_BASE64),
        "the attached bytes themselves must be on the wire: {body}"
    );
}

// ── chat route ──────────────────────────────────────────────────────────────

/// A chat turn carries the attachment too. The built-in tool loop cannot open a
/// local file, so delivery is the in-prompt notice that names the file — the
/// point is that the turn knows about it instead of answering as if nothing had
/// been attached (and instead of the file silently evaporating at launch).
#[test]
fn a_chat_turn_reaches_the_provider_with_the_attachment_accounted_for() {
    let (base_url, bodies) = capture_server(6);
    let mut host = host_with_builtin_provider(&base_url);
    stage_image(&mut host);
    send(&mut host, "привет, что ты умеешь?");

    let (launched, current_chat, _current_design) = launch(&mut host);

    assert!(launched, "the chat route must launch a turn");
    assert!(current_chat.is_some(), "a chat session is parked");
    assert!(host.editor_state().chat.pending_attachments.is_empty());
    let body = wait_for_body(
        &bodies,
        |b| b.contains("reference.png"),
        Duration::from_secs(15),
    )
    .unwrap_or_else(|seen| {
        panic!(
            "the chat turn must account for the attachment, got {} body/bodies: {seen:?}",
            seen.len()
        )
    });
    assert!(
        body.contains("NOT delivered"),
        "a transport that cannot carry the pixels must say so rather than let \
         the model describe a file it never saw: {body}"
    );
}

// ── design route ────────────────────────────────────────────────────────────

/// The design route never dropped attachments — this locks that in and covers
/// the drain-once refactor from the other side: the picture must reach the
/// pipeline that reads it (the multimodal reference brief for the orchestrator,
/// or the loop prompt when the tool loop takes the turn instead).
#[test]
fn a_design_turn_hands_the_attached_image_to_the_pipeline() {
    let (base_url, bodies) = capture_server(12);
    let mut host = host_with_builtin_provider(&base_url);
    stage_image(&mut host);
    send(&mut host, "draw a settings dashboard screen");

    let (launched, current_chat, current_design) = launch(&mut host);

    assert!(launched, "the design route must launch a turn");
    let orchestrator_ran = current_design.is_some();
    assert!(
        orchestrator_ran || current_chat.is_some(),
        "one of the two design routes must be running"
    );
    assert!(host.editor_state().chat.pending_attachments.is_empty());
    if orchestrator_ran {
        // The orchestrator turns the picture into a layout brief before it
        // plans, and that pass sends the image itself.
        let body = wait_for_body(
            &bodies,
            |b| b.contains(INLINE_PREFIX),
            Duration::from_secs(20),
        )
        .unwrap_or_else(|seen| {
            panic!(
                "the reference-brief pass must post the image, got {} body/bodies: {seen:?}",
                seen.len()
            )
        });
        assert!(body.contains(PNG_BASE64), "{body}");
    } else {
        let body = wait_for_body(
            &bodies,
            |b| b.contains("reference.png"),
            Duration::from_secs(20),
        )
        .unwrap_or_else(|seen| {
            panic!(
                "the design-loop turn must account for the attachment, got {} body/bodies: {seen:?}",
                seen.len()
            )
        });
        assert!(body.contains("NOT delivered"), "{body}");
    }
}

// ── the still-unclassified CLI standard-mode turn ───────────────────────────

/// The CLI standard-mode turn pre-builds BOTH of its possible requests before
/// the classifier runs on the worker, so each one has to carry the turn's
/// attachments on its own — the chat route because a chat answer about the
/// picture is one of the outcomes, the modify route because it used to be built
/// with `..Default::default()` and had no path for them at all.
///
/// This is a seam test: every provider on that route is a real CLI transport,
/// so the request cannot be observed on the wire in a unit test. It locks the
/// contract at the builders the launch path calls.
#[test]
fn both_requests_of_a_still_unclassified_turn_carry_the_attachments() {
    let mut state = EditorState::new();
    assert!(state.chat.add_attachment(png_attachment()));
    let attachments = TurnAttachments::drain(&mut state);

    let chat = chat_turn_attachments::plain_chat_request(
        "system".into(),
        "опиши эту картинку".into(),
        Vec::new(),
        op_ai::chat_provider::ThinkingMode::Adaptive,
        op_ai::chat_provider::EffortLevel::Low,
        None,
        &attachments,
    );
    assert_eq!(
        chat.attachments,
        vec![png_attachment()],
        "the chat route of the same turn must carry the picture"
    );

    // The modify plan needs a real target: a design on the page plus a
    // selection, which is what makes `build_modify_plan` produce a plan.
    let mut canvas = EditorState::new();
    canvas.active_children_mut().clear();
    canvas.active_children_mut().push(frame(
        "screen",
        "Food App Home",
        vec![frame("popular-card", "Bella Napoli Pizzeria", Vec::new())],
    ));
    canvas.set_single_selection(op_editor_core::NodeId::new("popular-card"));
    let plan = op_host_services::chat_intent::build_modify_plan(&canvas, "修改成饺子")
        .expect("the fixture canvas has a modification target");

    let modify = chat_turn_attachments::modify_request(
        plan,
        None,
        op_ai::chat_provider::ThinkingMode::Disabled,
        &attachments,
    );
    assert_eq!(
        modify.attachments,
        vec![png_attachment()],
        "the modify route of the same turn must carry the picture"
    );
    assert!(
        modify.user_message.contains("popular-card") || !modify.system_prompt.is_empty(),
        "the plan itself must still be the modify plan: {modify:?}"
    );
}

// ── refusal ─────────────────────────────────────────────────────────────────

/// When no transport can be built the turn does not run at all — and because
/// the attachments were already drained by then, the error text is the only
/// place left that can say they went nowhere. Silence here is the same defect
/// wearing a different hat.
#[test]
fn a_turn_that_cannot_launch_says_the_attachment_was_not_sent() {
    let mut host = WidgetHostNative::new();
    // An agent index with no provider route (the catalog's tail slot is
    // append-only, so this models a stale selection).
    host.editor_state_mut().editor_ui.chat_selected_agent = AgentProvider::ALL.len();
    stage_image(&mut host);
    send(&mut host, "опиши эту картинку");

    let (launched, current_chat, current_design) = launch(&mut host);

    assert!(
        launched,
        "the launch path reports the transcript change (the error bubble)"
    );
    assert!(current_chat.is_none() && current_design.is_none());
    let text = assistant_text(&host);
    assert!(
        text.contains("not available"),
        "the unwired agent is named honestly: {text}"
    );
    assert!(
        text.contains("1 attachment(s) were not sent"),
        "the person must be told the attachment did not go anywhere: {text}"
    );
}

/// The design route's reference channel is an image type, so a staged document
/// cannot ride it. The turn says so — in the prompt the planner reads and in
/// the transcript the person reads — instead of letting either side assume the
/// file arrived.
#[test]
fn a_design_route_names_the_files_it_cannot_carry() {
    let mut host = WidgetHostNative::new();
    let mut state = EditorState::new();
    assert!(state.chat.add_attachment(ChatAttachment {
        name: "brief.pdf".into(),
        media_type: "application/pdf".into(),
        data: vec![b'%', b'P', b'D', b'F'],
    }));
    let attachments = TurnAttachments::drain(&mut state);

    assert!(
        attachments.design_omission_note().contains("brief.pdf"),
        "the planner is told the document is unavailable"
    );
    assert!(attachments
        .design_omission_chat_note()
        .expect("the person is told too")
        .contains("brief.pdf"));

    // And the transcript note actually lands on the turn's own user bubble,
    // which is where a person looks for what they sent.
    host.editor_state_mut()
        .chat
        .messages
        .push(op_editor_core::ChatMessage::user("опиши макет"));
    note_design_attachment_omissions(&mut host, &attachments);
    let user_text = host.editor_state().chat.messages[0].content.clone();
    assert!(user_text.contains("brief.pdf"), "{user_text}");
}
