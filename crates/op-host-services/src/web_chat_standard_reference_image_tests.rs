//! Route-level proof that a reference image attached to a web design turn
//! reaches the provider as pixels (issue #61).
//!
//! The wire tests in `chat_builtin_http_attachment_tests.rs` prove the
//! provider *can* inline an image it is handed. They cannot prove the web
//! route hands it one: everything between the browser's `attachments` array
//! and `provider.send` — the snapshot, the intent routing, the reference
//! brief, the orchestrator — could drop, replace, or fail to pass it, and the
//! turn would still look successful while the model answered about a file
//! name. So this test drives the real `stream_standard_turn` entry point
//! against a loopback capture server and asserts on the bytes the provider
//! itself posted.
//!
//! The assertion is deliberately about the body rather than about a helper
//! being called: the bug this closes shipped with every unit green because
//! nothing ever looked at what left the process.
//!
//! The turn is driven on its own thread and *not* awaited to completion.
//! Everything asserted here — the image on the wire, and the brief in the
//! planning prompt — happens before the orchestrator applies its first
//! command, and applying that command lays the page out, which initialises
//! Skia's font manager. On macOS that is a CoreText enumeration of every
//! installed font: tens of seconds, unrelated to attachments, and it would
//! make this test a test about fonts. The detached thread is killed by process
//! exit, and the harness does not join it.

use super::*;
use op_ai::chat_provider::EffortLevel;
use op_editor_core::BuiltinAgentKind;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

/// A PNG signature plus a payload byte, so the bytes are recognisably a raster
/// image to the media-type sniffer and to the assertions below.
fn png_bytes() -> Vec<u8> {
    vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x2a]
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
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

fn request_body(request: &str) -> Value {
    let start = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .expect("request body separator");
    serde_json::from_str(&request[start..]).expect("request body JSON")
}

fn body_text(body: &Value) -> String {
    serde_json::to_string(body).expect("serialize body")
}

/// A real listener, not a stub `ChatProvider`: a stub would let this test pass
/// while `attachment_transport` lied about the transport, which is exactly the
/// failure being guarded against. Every request is answered with a short
/// OpenAI-shaped completion so the turn keeps moving.
fn capture_endpoint() -> (
    String,
    std_mpsc::Receiver<String>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture server address");
    listener
        .set_nonblocking(true)
        .expect("nonblocking capture server");
    let (tx, rx) = std_mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking read");
                    let request = read_http_request(&mut stream);
                    let _ = tx.send(request);
                    let body =
                        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}/v1"), rx, server)
}

/// A daemon-owned built-in provider. The id deliberately does NOT carry the
/// browser-owned `builtin-` prefix, so the route dials it with the trusted
/// policy the operator's own settings get — the loopback capture endpoint is
/// reachable without touching the deployment's endpoint allowlist.
fn agent_config(id: &str, base_url: String) -> BuiltinAgentConfig {
    BuiltinAgentConfig {
        id: id.into(),
        preset: op_editor_core::BuiltinAgentPresetKey::Custom,
        display_name: "Capture".into(),
        kind: BuiltinAgentKind::OpenAiCompat,
        api_key: "sk-test".into(),
        models: vec!["vision-model".into()],
        base_url,
        enabled: true,
    }
}

fn reference_turn(attachments: Vec<ChatAttachment>) -> WebStandardTurnRequest {
    WebStandardTurnRequest {
        ai: AiStreamRequest {
            provider: None,
            builtin_provider_id: None,
            model: "vision-model".into(),
            skills: Vec::new(),
            user: "make it look like this picture".into(),
            max_output_tokens: 512,
            thinking: op_ai::chat_provider::ThinkingMode::Disabled,
            effort: EffortLevel::Low,
            transient_builtin: None,
        },
        document_json: None,
        editor_meta: None,
        selected_ids: Vec::new(),
        active_page_id: None,
        agent_team_size: Some(1),
        history: Vec::new(),
        attachments,
        transient_builtin: None,
    }
}

/// Drive one web design turn against the capture endpoint and return every
/// request body it posted, stopping as soon as `wanted` accepts one.
fn bodies_until<F>(attachments: Vec<ChatAttachment>, wanted: F) -> Vec<Value>
where
    F: Fn(&Value) -> bool,
{
    let (base_url, rx, _server) = capture_endpoint();
    let mut editor = EditorState::new();
    editor
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(agent_config("test-daemon-agent", base_url));
    let state = Arc::new(Mutex::new(WebCanvasState::new(editor, 3100)));
    let request = reference_turn(attachments);

    std::thread::spawn(move || {
        let hub = SseHub::default();
        let mut out = Vec::new();
        let _ = stream_standard_turn(&mut out, request, &state, &hub, None, None);
    });

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut bodies = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(request) => {
                let body = request_body(&request);
                let matched = wanted(&body);
                bodies.push(body);
                if matched {
                    break;
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    bodies
}

/// The whole point of issue #61: an image attached to a web design turn must
/// leave the process as an image. A body carrying only a path line — or a
/// notice that the attachment was not delivered — is the bug this test exists
/// to catch.
#[test]
fn web_design_turn_posts_the_reference_image_as_pixels() {
    // Wait for the planner's own request, which is the first body that proves
    // the inventory was carried past the brief call.
    let bodies = bodies_until(
        vec![ChatAttachment {
            name: "reference.png".into(),
            media_type: "image/png".into(),
            data: png_bytes(),
        }],
        |body| body_text(body).contains("REFERENCE SCREEN BRIEF"),
    );

    assert!(
        !bodies.is_empty(),
        "the design turn must reach the provider at all"
    );

    // ── The pixels ────────────────────────────────────────────────────────
    let mut image_carrier = None;
    for body in &bodies {
        let Some(messages) = body["messages"].as_array() else {
            continue;
        };
        for message in messages {
            let Some(parts) = message["content"].as_array() else {
                continue;
            };
            for part in parts.iter().filter(|p| p["type"] == "image_url") {
                let url = part["image_url"]["url"].as_str().unwrap_or_default();
                image_carrier = Some((body, url.to_string()));
            }
        }
    }
    let (carrier, url) = image_carrier.expect(
        "no request carried the reference image: the web route dropped the \
         attachment before the provider saw it",
    );
    let encoded = url
        .strip_prefix("data:image/png;base64,")
        .unwrap_or_else(|| panic!("expected a png data URL, got: {url}"));
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 payload"),
        png_bytes(),
        "the pixels on the wire must be the attachment's own bytes"
    );

    // ── It is the *inventory* call that carries them ─────────────────────
    // An image on some unrelated request would not ground the brief, and the
    // brief is what the planner is told to trust.
    assert!(
        body_text(carrier).contains("Describe this reference UI for planning"),
        "the image must ride the reference-brief call, which is the call whose \
         answer is injected into planning as an authoritative inventory"
    );

    // ── No fabrication bait ──────────────────────────────────────────────
    for body in &bodies {
        let text = body_text(body);
        assert!(
            !text.contains("NOT delivered"),
            "a turn that carried the image must not also tell the model it did \
             not: {text}"
        );
        assert!(
            !text.contains("attached image:"),
            "no request may announce the attachment as a path the model cannot \
             open: {text}"
        );
    }

    // ── The inventory reaches the planner ────────────────────────────────
    // The brief is the only channel through which the model that writes nodes
    // learns about the picture, so a grounded brief that never arrives is the
    // same failure one step later.
    assert!(
        bodies
            .iter()
            .any(|body| body_text(body).contains("REFERENCE SCREEN BRIEF")),
        "the reference brief must be injected into the planning prompt"
    );
}

/// The second half of issue #61: when the pixels cannot be delivered, the turn
/// must not be told to inventory a screenshot it never received. This drives
/// the same route with an attachment whose bytes are not a raster image at
/// all — the vision client refuses — and the brief must fall back to the
/// conservative text instead of inviting an invented inventory.
#[test]
fn undeliverable_reference_falls_back_instead_of_inviting_an_inventory() {
    let bodies = bodies_until(
        vec![ChatAttachment {
            // Labelled an image, but the bytes are not one: no vision wire
            // accepts this, so the brief cannot be written from pixels.
            name: "reference.png".into(),
            media_type: "image/png".into(),
            data: b"not really a png".to_vec(),
        }],
        |body| body_text(body).contains("vision brief unavailable"),
    );

    assert!(
        bodies
            .iter()
            .any(|body| body_text(body).contains("vision brief unavailable")),
        "the conservative fallback brief must reach the planner when pixels \
         could not be delivered"
    );

    for body in &bodies {
        let text = body_text(body);
        assert!(
            !text.contains("image_url"),
            "an undeliverable payload must not be posted as image input: {text}"
        );
        // The conservative brief is the honest outcome, and it must read as
        // one: no authoritative `## Reference screen brief` heading, no
        // invented `## Visible regions` inventory.
        assert!(
            !text.contains("Reference screen brief"),
            "a blind turn must not hand the planner a model-written inventory: {text}"
        );
        assert!(
            !text.contains("Visible regions"),
            "a blind turn must not invent visible regions: {text}"
        );
    }
}
