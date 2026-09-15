use super::*;
use op_ai::chat_provider::{AttachmentTransport, StopReason};
use op_editor_core::EditorState;
use std::sync::Mutex;

#[test]
fn validation_prompt_preserves_intentional_horizontal_scrollers() {
    let prompt = validation_system_prompt();
    assert!(
        prompt.contains("clip=true") && prompt.contains("intentional horizontal scroller"),
        "validation prompt must distinguish scroll intent from overflow: {prompt}"
    );
}

#[test]
fn validation_prompt_does_not_outline_chart_marks() {
    let prompt = validation_system_prompt();
    assert!(
        prompt.contains("chart bars") && prompt.contains("Do not add borders"),
        "validation prompt must distinguish chart marks from card surfaces: {prompt}"
    );
}

#[test]
fn validation_prompt_limits_image_review_to_rendering_integrity() {
    let prompt = validation_system_prompt();
    assert!(prompt.contains("IMAGE REVIEW SCOPE — PRESENTATION ONLY"));
    assert!(prompt.contains("Do NOT judge image subject relevance"));
    assert!(prompt.contains("A correctly displayed image passes validation"));
}

// ── RealScreenshotProvider ───────────────────────────────────────────────

/// An empty document has no page content → the real provider returns
/// `None` (loop skips the round), never panics.
#[test]
fn real_screenshot_provider_returns_none_on_empty_doc() {
    let state = EditorState::new();
    assert!(RealScreenshotProvider.capture_root_frame(&state).is_none());
}

/// A document with real content renders to a base64 PNG payload.
#[test]
fn real_screenshot_provider_renders_png_for_a_populated_doc() {
    let doc: jian_ops_schema::PenDocument = serde_json::from_str(
        r##"{
            "version":"1.0",
            "children":[
                {"type":"frame","id":"root","name":"Root",
                 "x":0,"y":0,"width":200,"height":120,
                 "fill":[{"type":"solid","color":"#ffffff"}],
                 "children":[
                    {"type":"rectangle","id":"r1","name":"Box",
                     "x":10,"y":10,"width":80,"height":40,
                     "fill":[{"type":"solid","color":"#3b82f6"}]}
                 ]}
            ]
        }"##,
    )
    .expect("fixture parses");
    let state = EditorState::from_document(doc);
    let b64 = RealScreenshotProvider
        .capture_root_frame(&state)
        .expect("populated doc renders a screenshot");
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .expect("valid base64");
    assert_eq!(
        &png[..4],
        &[0x89, b'P', b'N', b'G'],
        "must be a PNG payload"
    );
}

// ── ChatVisionLlmClient ──────────────────────────────────────────────────

/// A scripted provider that records the `ChatRequest` it received and
/// replies with a fixed JSON verdict.
struct RecordingVisionProvider {
    seen: Arc<Mutex<Vec<ChatRequest>>>,
    reply: String,
}

impl ChatProvider for RecordingVisionProvider {
    fn provider_label(&self) -> &str {
        "recording-vision"
    }
    // This double carries attachments (it is handed the whole
    // `ChatRequest`), so it must say so — the vision client refuses to
    // call a transport that declares `Dropped`.
    fn attachment_transport(&self) -> AttachmentTransport {
        AttachmentTransport::InlineImage
    }
    fn send(&self, request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.seen.lock().unwrap().push(request);
        Box::new(
            vec![
                ChatDelta::TextDelta(self.reply.clone()),
                ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                },
            ]
            .into_iter(),
        )
    }
}

fn b64_png() -> String {
    use base64::Engine as _;
    // The full 8-byte PNG signature: the client now derives the wire
    // media type from the payload's magic, so a truncated header is no
    // longer a PNG as far as the transport is concerned.
    base64::engine::general_purpose::STANDARD
        .encode([0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

fn b64_jpeg() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10])
}

fn vision_req(image_base64: &str) -> VisionCallRequest {
    VisionCallRequest {
        system: "You are a design validator. Return JSON.".into(),
        message: "Analyze this screenshot.".into(),
        image_base64: image_base64.to_string(),
        model: Some("vision-model".into()),
        provider: None,
        timeout: std::time::Duration::from_secs(30),
    }
}

/// The real client sends the screenshot as an image attachment, inlines
/// the system prompt into the user message, and returns the reply text.
#[test]
fn chat_vision_client_sends_image_attachment_and_returns_text() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(RecordingVisionProvider {
        seen: seen.clone(),
        reply: r#"{"issues":[],"fixes":[],"qualityScore":9}"#.into(),
    });
    let client = ChatVisionLlmClient::new(provider).with_model(Some("vision-model".into()));

    let resp = client.validate(vision_req(&b64_png()));

    // Reply text surfaced verbatim for parse_validation_response.
    match resp {
        VisionResponse::Text(t) => {
            assert!(t.contains("qualityScore"), "got: {t}");
        }
        VisionResponse::Skipped { reason } => panic!("unexpected skip: {reason:?}"),
    }

    // The request carried exactly one image attachment with the PNG bytes.
    let reqs = seen.lock().unwrap();
    let r = reqs.first().expect("provider was called");
    assert_eq!(r.attachments.len(), 1, "one screenshot attachment");
    assert!(r.attachments[0].is_image(), "attachment is an image");
    assert_eq!(r.attachments[0].media_type, "image/png");
    assert_eq!(&r.attachments[0].data[..4], &[0x89, b'P', b'N', b'G']);
    // System prompt inlined into the user message (CLI providers ignore
    // the system field).
    assert!(
        r.user_message.contains("design validator"),
        "system prompt inlined; got: {}",
        r.user_message
    );
    assert!(r.user_message.contains("Analyze this screenshot"));
    assert_eq!(r.model.as_deref(), Some("vision-model"));
}

#[test]
fn chat_vision_client_does_not_forward_acp_capability_marker_as_model() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(RecordingVisionProvider {
        seen: seen.clone(),
        reply: r#"{"issues":[],"fixes":[],"qualityScore":9}"#.into(),
    });
    let client =
        ChatVisionLlmClient::new(provider).with_model(Some("acp:custom/vendor".to_string()));
    let mut request = vision_req(&b64_png());
    request.model = Some("acp:custom/vendor".to_string());

    let response = client.validate(request);

    assert!(matches!(response, VisionResponse::Text(_)));
    let requests = seen.lock().unwrap();
    assert_eq!(
        requests.first().expect("provider was called").model,
        None,
        "ACP catalog identity is a capability marker, not a transport model"
    );
}

/// A non-base64 screenshot string can't drive a vision call → Skipped.
#[test]
fn chat_vision_client_skips_on_bad_base64() {
    let provider = Arc::new(RecordingVisionProvider {
        seen: Arc::new(Mutex::new(Vec::new())),
        reply: "{}".into(),
    });
    let client = ChatVisionLlmClient::new(provider);
    let resp = client.validate(vision_req("@@@ not base64 @@@"));
    assert!(matches!(resp, VisionResponse::Skipped { .. }));
}

/// An `Error` delta downgrades to `Skipped` (never crashes the turn).
#[test]
fn chat_vision_client_skips_on_provider_error() {
    struct ErroringProvider;
    impl ChatProvider for ErroringProvider {
        fn provider_label(&self) -> &str {
            "erroring"
        }
        fn attachment_transport(&self) -> AttachmentTransport {
            AttachmentTransport::InlineImage
        }
        fn send(&self, _r: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
            Box::new(
                vec![
                    ChatDelta::Error("quota exhausted".into()),
                    ChatDelta::Done {
                        stop_reason: StopReason::Aborted,
                    },
                ]
                .into_iter(),
            )
        }
    }
    let client = ChatVisionLlmClient::new(Arc::new(ErroringProvider));
    let resp = client.validate(vision_req(&b64_png()));
    match resp {
        VisionResponse::Skipped { reason } => {
            assert!(reason.unwrap().contains("quota exhausted"));
        }
        VisionResponse::Text(_) => panic!("error should downgrade to Skipped"),
    }
}

/// An empty reply downgrades to `Skipped` (parse_validation_response
/// would otherwise treat "" as a parse failure anyway).
#[test]
fn chat_vision_client_skips_on_empty_reply() {
    let provider = Arc::new(RecordingVisionProvider {
        seen: Arc::new(Mutex::new(Vec::new())),
        reply: "   ".into(),
    });
    let client = ChatVisionLlmClient::new(provider);
    let resp = client.validate(vision_req(&b64_png()));
    assert!(matches!(resp, VisionResponse::Skipped { .. }));
}

/// A transport that drops attachments must not be asked at all: a
/// text-only "vision" call answers about the file name it can see, and
/// that invented answer would be returned as grounded text (issue #61).
struct DroppingVisionProvider {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl ChatProvider for DroppingVisionProvider {
    fn provider_label(&self) -> &str {
        "drops-attachments"
    }
    // No `attachment_transport` override: the trait default (`Dropped`)
    // is exactly the declaration under test.
    fn send(&self, _r: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::new(
            vec![
                // What a real model does when it is given a path instead of
                // pixels: a fluent, fully invented inventory.
                ChatDelta::TextDelta(
                    "## Visible regions\n- sidebar\n- KPI cards\n- analytics charts".into(),
                ),
                ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                },
            ]
            .into_iter(),
        )
    }
}

#[test]
fn chat_vision_client_refuses_a_transport_that_drops_images() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let client = ChatVisionLlmClient::new(Arc::new(DroppingVisionProvider {
        calls: calls.clone(),
    }));

    let resp = client.validate(vision_req(&b64_png()));

    match resp {
        VisionResponse::Skipped { reason } => {
            let reason = reason.expect("a skip must carry its reason");
            assert!(
                reason.contains("drops-attachments"),
                "the reason must name the transport so an operator can act: {reason}"
            );
        }
        VisionResponse::Text(t) => panic!("a dropped image must not read as vision text: {t}"),
    }
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the model must not be called at all when the image cannot reach it"
    );
}

/// The reference brief is the consumer that turned a fabricated answer
/// into authoritative planning input: with a transport that delivers no
/// pixels it must fall back to the conservative brief instead of
/// accepting whatever text comes back.
#[test]
fn reference_brief_stays_conservative_when_the_transport_drops_images() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let client = ChatVisionLlmClient::new(Arc::new(DroppingVisionProvider {
        calls: calls.clone(),
    }));
    let attachments = vec![op_orchestrator::ReferenceAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
    }];

    let brief = op_orchestrator::reference_brief::resolve_reference_brief(
        &attachments,
        &client,
        None,
        None,
    )
    .expect("an attached image always yields some brief");

    // Lines unique to the invented reply — the conservative brief itself
    // mentions KPI cards only to forbid them.
    assert!(
        !brief.contains("- sidebar") && !brief.contains("- analytics charts"),
        "the invented inventory must not reach the planner: {brief}"
    );
    assert!(
        brief.contains("fallback"),
        "the conservative fallback is the honest brief here: {brief}"
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no vision call may be spent on a transport that cannot deliver the image"
    );
}

/// Positive control for the test above: the same call through a transport
/// that does deliver returns the model's inventory.
#[test]
fn reference_brief_uses_model_text_when_the_transport_delivers() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let client = ChatVisionLlmClient::new(Arc::new(RecordingVisionProvider {
        seen,
        reply: "## Visible regions\n- sidebar\n- filters\n\n## Absent modules\n- no KPI cards"
            .into(),
    }));
    let attachments = vec![op_orchestrator::ReferenceAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
    }];

    let brief = op_orchestrator::reference_brief::resolve_reference_brief(
        &attachments,
        &client,
        None,
        None,
    )
    .expect("brief");

    assert!(brief.contains("## Reference screen brief"), "{brief}");
    assert!(brief.contains("- sidebar"), "{brief}");
    assert!(!brief.contains("fallback"), "{brief}");
}

/// A payload no vision wire accepts (an SVG is XML, not pixels) is not
/// vision input, however it was labelled on the way in.
#[test]
fn chat_vision_client_skips_a_payload_that_is_not_a_raster_image() {
    use base64::Engine as _;
    let provider = Arc::new(RecordingVisionProvider {
        seen: Arc::new(Mutex::new(Vec::new())),
        reply: "## Visible regions\n- invented".into(),
    });
    let svg = base64::engine::general_purpose::STANDARD
        .encode(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#);

    match ChatVisionLlmClient::new(provider).validate(vision_req(&svg)) {
        VisionResponse::Skipped { reason } => {
            assert!(
                reason.unwrap().contains("png/jpeg/gif/webp"),
                "the reason must say what the payload is not"
            );
        }
        VisionResponse::Text(_) => panic!("an SVG cannot ground a vision answer"),
    }
}

/// The wire media type follows the bytes. A JPEG reported as a PNG used
/// to be labelled `image/png` — and Anthropic decodes the base64 and
/// rejects the mismatch, which failed the whole turn.
#[test]
fn chat_vision_client_labels_the_attachment_from_its_bytes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(RecordingVisionProvider {
        seen: seen.clone(),
        reply: "{}".into(),
    });

    let _ = ChatVisionLlmClient::new(provider).validate(vision_req(&b64_jpeg()));

    let reqs = seen.lock().unwrap();
    let attachment = &reqs.first().expect("provider was called").attachments[0];
    assert_eq!(attachment.media_type, "image/jpeg");
    assert_eq!(
        attachment.name, "design-screenshot.jpeg",
        "a path transport writes this name to disk, so the extension has to match the bytes"
    );
}

// ── End-to-end wiring test ───────────────────────────────────────────────
//
// Drives `op_orchestrator::run_post_generation_validation` with the REAL
// host providers (`RealScreenshotProvider` renders an actual PNG of the
// live document; `ChatVisionLlmClient` runs a fake-but-real-shaped
// `ChatProvider`). Proves the wiring is LIVE end-to-end: screenshot →
// vision → a safe fix lands on the live document. The default-off variant
// proves the stubs keep the loop a no-op (default path unchanged).

use op_orchestrator::DocSink;

/// Minimal `DocSink` over a live `EditorState` (mirrors
/// `pre_validator::tests::TestSink`).
struct LiveSink {
    editor: EditorState,
}
impl DocSink for LiveSink {
    fn state(&self) -> &EditorState {
        &self.editor
    }
    fn apply(&mut self, cmd: op_editor_core::EditorCommand) -> bool {
        self.editor.apply(cmd)
    }
    fn begin_undo_batch(&mut self) {}
    fn end_undo_batch(&mut self) {}
}

/// A `ChatProvider` that returns a fixed JSON validation verdict and
/// records how many times it was asked (proves the real vision client
/// actually called the provider, i.e. the loop reached the vision step).
struct ScriptedVisionProvider {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    reply: String,
}
impl ChatProvider for ScriptedVisionProvider {
    fn provider_label(&self) -> &str {
        "scripted-vision"
    }
    fn attachment_transport(&self) -> AttachmentTransport {
        AttachmentTransport::InlineImage
    }
    fn send(&self, _r: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::new(
            vec![
                ChatDelta::TextDelta(self.reply.clone()),
                ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                },
            ]
            .into_iter(),
        )
    }
}

/// Build a renderable doc whose active page holds >= 30 nodes (clears the
/// `VALIDATION_NODE_COUNT_THRESHOLD = 30` gate) including a frame named
/// `target` the vision verdict will fix.
fn doc_over_threshold() -> jian_ops_schema::PenDocument {
    let mut children = String::new();
    // The fix target: a frame with cornerRadius 0 the verdict bumps to 24.
    children.push_str(
        r##"{"type":"frame","id":"target","name":"Target",
            "x":10,"y":10,"width":120,"height":48,"cornerRadius":0,
            "fill":[{"type":"solid","color":"#3b82f6"}]}"##,
    );
    // Pad to >= 30 total nodes (1 root + 1 target + 30 fillers).
    for i in 0..30 {
        children.push_str(&format!(
            r##",{{"type":"rectangle","id":"r{i}","name":"R{i}",
                "x":0,"y":{y},"width":40,"height":20,
                "fill":[{{"type":"solid","color":"#e2e8f0"}}]}}"##,
            y = 60 + i * 24
        ));
    }
    let json = format!(
        r##"{{"version":"1.0","children":[
            {{"type":"frame","id":"root","name":"Root",
              "x":0,"y":0,"width":200,"height":900,
              "fill":[{{"type":"solid","color":"#ffffff"}}],
              "children":[{children}]}}
        ]}}"##
    );
    serde_json::from_str(&json).expect("fixture parses")
}

fn request_with_validation(enabled: bool) -> op_orchestrator::DesignRequest {
    op_orchestrator::DesignRequest {
        prompt: "p".into(),
        model: None,
        provider: None,
        rules: Vec::new(),
        concurrency: 1,
        continuation_context: None,
        append_context: None,
        validation_enabled: enabled,
        visual_ref_enabled: false,
        pinned_style_guide: None,
        reference_attachments: Vec::new(),
        reference_brief: None,
    }
}

/// REAL providers + validation_enabled=true → the loop captures a real
/// PNG, calls the vision provider, and applies the cornerRadius safe fix
/// to the live document. (The vision call only happens because the real
/// screenshot returned `Some(png)` — proving the screenshot wiring is
/// live, not the stub's `None`.)
#[test]
fn real_providers_drive_loop_capture_vision_and_apply_fix() {
    let mut sink = LiveSink {
        editor: EditorState::from_document(doc_over_threshold()),
    };

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // qualityScore 5 (< threshold 8) so the loop proceeds to apply fixes.
    let provider = Arc::new(ScriptedVisionProvider {
        calls: calls.clone(),
        reply: r#"{"issues":["target corner radius too sharp"],
                   "fixes":[{"nodeId":"target","property":"cornerRadius","value":24}],
                   "structuralFixes":[],"qualityScore":5}"#
            .into(),
    });

    let screenshot = RealScreenshotProvider;
    let vision = ChatVisionLlmClient::new(provider);
    let pre_validator = op_orchestrator::SkippedPreValidator;
    let abort = op_orchestrator::AbortFlag::new();
    let mut events: Vec<op_orchestrator::Progress> = Vec::new();

    let summary = op_orchestrator::run_post_generation_validation(
        &mut sink,
        &pre_validator,
        &screenshot,
        &vision,
        "validator system prompt",
        &request_with_validation(true),
        &mut |p| events.push(p),
        &abort,
    )
    .expect("loop runs");

    // The vision provider was actually called — proves screenshot → vision
    // wiring is live (stub would have returned None and skipped this).
    assert!(
        calls.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "real vision provider must have been called at least once"
    );
    // At least one fix was applied to the live document.
    assert!(
        summary.total_applied >= 1,
        "expected the cornerRadius fix to apply, got {}",
        summary.total_applied
    );
    assert!(summary.rounds_run >= 1, "at least one vision round ran");

    // The fix actually LANDED on the live document: the target frame's
    // container cornerRadius is now Uniform(24). Reading the concrete
    // post-mutation value (not just the applied count) proves the fix
    // wrote through the real sink, not a counter that lied.
    let target = op_editor_core::walkers::find_node(
        sink.state().active_children(),
        &op_editor_core::node_id::NodeId::new("target"),
    )
    .expect("target node present");
    match target {
        jian_ops_schema::node::PenNode::Frame(f) => {
            use jian_ops_schema::node::container::CornerRadius;
            assert!(
                matches!(f.container.corner_radius, Some(CornerRadius::Uniform(r)) if (r - 24.0).abs() < 1e-6),
                "cornerRadius fix must be written to the live document, got {:?}",
                f.container.corner_radius
            );
        }
        other => panic!("target is not a frame: {other:?}"),
    }

    // Progress stream reached a real vision round.
    assert!(
        events
            .iter()
            .any(|p| matches!(p, op_orchestrator::Progress::ValidationRoundStarted { .. })),
        "a vision round must have started"
    );
}

/// Default-off proof: the STUB providers (selected when
/// `OPENPENCIL_VISION_VALIDATION` is unset) keep the loop a no-op even at
/// node_count >= 30 with validation_enabled=true — zero rounds, document
/// untouched. This is the byte-for-byte default path.
#[test]
fn stub_providers_keep_loop_a_noop_even_above_threshold() {
    let mut sink = LiveSink {
        editor: EditorState::from_document(doc_over_threshold()),
    };
    let before = sink.state().active_children().len();

    let screenshot = op_orchestrator::SkippedScreenshotProvider;
    let vision = op_orchestrator::SkippedVisionLlmClient;
    let pre_validator = op_orchestrator::SkippedPreValidator;
    let abort = op_orchestrator::AbortFlag::new();
    let mut events: Vec<op_orchestrator::Progress> = Vec::new();

    let summary = op_orchestrator::run_post_generation_validation(
        &mut sink,
        &pre_validator,
        &screenshot,
        &vision,
        "",
        &request_with_validation(true),
        &mut |p| events.push(p),
        &abort,
    )
    .expect("loop runs");

    assert_eq!(summary.total_applied, 0, "stub loop applies nothing");
    assert_eq!(summary.rounds_run, 0, "stub loop runs zero vision rounds");
    // Document untouched — cornerRadius stays 0, node count unchanged.
    assert_eq!(sink.state().active_children().len(), before);
    let target = op_editor_core::walkers::find_node(
        sink.state().active_children(),
        &op_editor_core::node_id::NodeId::new("target"),
    )
    .expect("target node present");
    match target {
        jian_ops_schema::node::PenNode::Frame(f) => {
            use jian_ops_schema::node::container::CornerRadius;
            assert!(
                matches!(f.container.corner_radius, Some(CornerRadius::Uniform(r)) if r == 0.0),
                "stub path must NOT mutate cornerRadius, got {:?}",
                f.container.corner_radius
            );
        }
        other => panic!("target is not a frame: {other:?}"),
    }
    // No vision round ever started (screenshot stub returned None first).
    assert!(
        !events
            .iter()
            .any(|p| matches!(p, op_orchestrator::Progress::ValidationRoundDone { .. })),
        "no vision round should complete on the stub path"
    );
}
