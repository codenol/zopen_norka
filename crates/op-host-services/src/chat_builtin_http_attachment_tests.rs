//! Wire-level attachment tests for the built-in HTTP provider (issue #61).
//!
//! These capture the REAL request body the provider posts, so they prove the
//! image left the process instead of proving a helper was called: the bug
//! they close shipped with every unit green because no test ever looked at
//! the body. A loopback `TcpListener` stands in for the endpoint — no
//! external network, and the assertion is on bytes the provider itself wrote.

use super::*;
use op_ai::chat_provider::ChatToolResult;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

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

fn request_body(request: &str) -> Value {
    let body_start = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .expect("request body separator");
    serde_json::from_str(&request[body_start..]).expect("request body JSON")
}

fn capture_provider(kind: BuiltinAgentKind, base_url: String) -> ConfiguredBuiltinProvider {
    let config = BuiltinAgentConfig {
        id: "attachment-test".into(),
        preset: op_editor_core::BuiltinAgentPresetKey::Custom,
        display_name: "Attachment test".into(),
        kind,
        api_key: "sk-test".into(),
        models: vec!["vision-model".into()],
        base_url,
        enabled: true,
    };
    let mut provider = ConfiguredBuiltinProvider::from_builtin_agent(&config)
        .expect("ready attachment-test provider");
    provider.max_retries = 0;
    provider.min_gap = Duration::ZERO;
    provider
}

/// Send one request through a real provider against a loopback capture server
/// and return the JSON body the provider posted.
fn captured_body(kind: BuiltinAgentKind, request: ChatRequest) -> Value {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture server address");
    let (request_tx, request_rx) = std_mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request = read_http_request(&mut stream);
        request_tx.send(request).expect("capture request");
        // An empty SSE stream is enough: the test reads the request body, and
        // the provider closes the turn itself when the stream ends.
        let response = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                        content-length: 0\r\nconnection: close\r\n\r\n";
        stream
            .write_all(response.as_bytes())
            .expect("write SSE response");
    });

    let provider = capture_provider(kind, format!("http://{addr}/v1"));
    let _deltas: Vec<_> = provider.send(request).collect();
    let request = request_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("provider posted a request");
    server.join().expect("capture server exits");
    request_body(&request)
}

fn png_attachment() -> ChatAttachment {
    ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        // A real 8-byte PNG signature: the media type on the wire is derived
        // from these bytes, not from the label above.
        data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x01, 0x02],
    }
}

fn last_user_message(body: &Value) -> &Value {
    body["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .expect("a user message")
}

#[test]
fn openai_request_carries_the_screenshot_as_an_inline_data_url() {
    let body = captured_body(
        BuiltinAgentKind::OpenAiCompat,
        ChatRequest {
            user_message: "match this screenshot".into(),
            attachments: vec![png_attachment()],
            ..Default::default()
        },
    );

    let content = &last_user_message(&body)["content"];
    let parts = content
        .as_array()
        .expect("a turn with an image must send a content-parts array");
    let image = parts
        .iter()
        .find(|p| p["type"] == "image_url")
        .expect("an image_url part");
    let url = image["image_url"]["url"].as_str().expect("data URL string");
    let encoded = url
        .strip_prefix("data:image/png;base64,")
        .unwrap_or_else(|| panic!("expected a png data URL, got: {url}"));
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("base64 payload");
    assert_eq!(
        decoded,
        png_attachment().data,
        "the pixels on the wire must be the attachment's bytes"
    );

    // The text part still carries the turn's prompt, and it no longer names a
    // temp path the model could not open.
    let text = parts
        .iter()
        .find(|p| p["type"] == "text")
        .expect("a text part");
    assert!(text["text"]
        .as_str()
        .expect("text")
        .contains("match this screenshot"));
    assert!(
        !text["text"].as_str().unwrap().contains("attached image"),
        "an image that rides the body must not also be announced as a file path"
    );
}

#[test]
fn anthropic_request_carries_the_screenshot_as_a_base64_source_block() {
    let body = captured_body(
        BuiltinAgentKind::Anthropic,
        ChatRequest {
            user_message: "match this screenshot".into(),
            attachments: vec![png_attachment()],
            ..Default::default()
        },
    );

    let content = &last_user_message(&body)["content"];
    let blocks = content
        .as_array()
        .expect("a turn with an image must send content blocks");
    let image = blocks
        .iter()
        .find(|b| b["type"] == "image")
        .expect("an image block");
    assert_eq!(image["source"]["type"], "base64");
    assert_eq!(image["source"]["media_type"], "image/png");
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(image["source"]["data"].as_str().expect("data"))
        .expect("base64 payload");
    assert_eq!(decoded, png_attachment().data);
    assert!(blocks.iter().any(|b| b["type"] == "text"
        && b["text"]
            .as_str()
            .is_some_and(|t| t.contains("match this screenshot"))));
}

/// An image-only turn must not carry an empty text block: Anthropic rejects
/// `{"type":"text","text":""}` with a 400, which would fail the very turn the
/// image was attached for. (The system prompt is set here so the skill
/// preamble does not silently fill the empty message.)
#[test]
fn anthropic_image_only_turn_omits_the_empty_text_block() {
    let body = captured_body(
        BuiltinAgentKind::Anthropic,
        ChatRequest {
            system_prompt: "You inventory the attached screenshot.".into(),
            user_message: String::new(),
            attachments: vec![png_attachment()],
            ..Default::default()
        },
    );

    let blocks = last_user_message(&body)["content"]
        .as_array()
        .expect("content blocks")
        .clone();
    assert!(blocks.iter().any(|b| b["type"] == "image"));
    assert!(
        !blocks.iter().any(|b| b["type"] == "text"),
        "got: {blocks:?}"
    );
}

/// Requirement: a non-image attachment must never become an image block. A
/// text file has no pixels to inline, and the turn stays a plain string on
/// the wire — the historical shape, unchanged.
#[test]
fn text_attachment_never_becomes_an_image_block() {
    let body = captured_body(
        BuiltinAgentKind::OpenAiCompat,
        ChatRequest {
            user_message: "summarize notes.txt".into(),
            attachments: vec![ChatAttachment {
                name: "notes.txt".into(),
                media_type: "text/plain".into(),
                data: b"hello".to_vec(),
            }],
            ..Default::default()
        },
    );

    let content = &last_user_message(&body)["content"];
    let text = content
        .as_str()
        .expect("a text-only turn keeps the historical string content");
    assert!(text.contains("summarize notes.txt"));
    assert!(
        !serde_json::to_string(&body)
            .expect("serialize body")
            .contains("image_url"),
        "no part of a text-only turn may look like image input"
    );
}

/// An `image/png` label on JPEG bytes must not reach the wire as a PNG:
/// Anthropic decodes the base64 and rejects the mismatch, failing the turn.
#[test]
fn wire_media_type_follows_the_bytes_not_the_declared_label() {
    let body = captured_body(
        BuiltinAgentKind::OpenAiCompat,
        ChatRequest {
            user_message: "match this".into(),
            attachments: vec![ChatAttachment {
                name: "mislabelled.png".into(),
                media_type: "image/png".into(),
                data: vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10],
            }],
            ..Default::default()
        },
    );

    let url = last_user_message(&body)["content"]
        .as_array()
        .expect("content parts")
        .iter()
        .find(|p| p["type"] == "image_url")
        .expect("image part")["image_url"]["url"]
        .as_str()
        .expect("data URL")
        .to_string();
    assert!(
        url.starts_with("data:image/jpeg;base64,"),
        "a JPEG must not be posted as a PNG: {url}"
    );
}

// ── The tool-executing agent loop ───────────────────────────────────────────

struct NoopToolExecutor;

impl ChatToolExecutor for NoopToolExecutor {
    fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
        ChatToolResult {
            content: "{}".into(),
            is_error: false,
        }
    }
}

/// Serve agent-loop requests until `stop` is raised: capture each body and
/// answer with a plain text completion, so the loop ends without calling a
/// tool. Returns (base_url, captured requests, stop flag, server thread).
fn agent_loop_capture_server() -> (
    String,
    std_mpsc::Receiver<String>,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture server address");
    listener
        .set_nonblocking(true)
        .expect("nonblocking capture server");
    let (request_tx, request_rx) = std_mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("blocking per-connection read");
                    let request = read_http_request(&mut stream);
                    let _ = request_tx.send(request);
                    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
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
    (format!("http://{addr}/v1"), request_rx, stop, server)
}

/// The tool-executing agent loop cannot carry an attachment (it takes a
/// `user_prompt` string, and its canvas tools cannot open a local file). The
/// old prompt announced `[attached image: /tmp/…]` anyway, which is the bait
/// the model answered about; the turn must now say the attachment was NOT
/// delivered, and no part of the body may look like image input.
#[test]
fn tool_loop_turn_tells_the_model_the_attachment_was_not_delivered() {
    let (base_url, request_rx, stop, server) = agent_loop_capture_server();
    let provider = capture_provider(BuiltinAgentKind::OpenAiCompat, base_url).with_canvas_tools(
        vec![ChatToolDef {
            name: "get_screenshot".into(),
            description: "canvas read".into(),
            level: "read".into(),
            input_schema_json: "{}".into(),
        }],
        Arc::new(NoopToolExecutor),
    );
    assert_eq!(
        provider.attachment_transport(),
        AttachmentTransport::Dropped,
        "a tool-capable turn delivers nothing, and says so"
    );

    let _deltas: Vec<_> = provider
        .send(ChatRequest {
            user_message: "match this screenshot".into(),
            attachments: vec![png_attachment()],
            ..Default::default()
        })
        .collect();

    let request = request_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the agent loop posted a request");
    stop.store(true, Ordering::Release);
    server.join().expect("capture server exits");

    let body = request_body(&request);
    let content = last_user_message(&body)["content"]
        .as_str()
        .expect("the agent loop carries a plain user prompt");
    assert!(content.contains("match this screenshot"), "{content}");
    assert!(content.contains("NOT delivered"), "{content}");
    assert!(
        content.contains("reference.png"),
        "the attachment is still named so the answer can be actionable: {content}"
    );
    assert!(
        !content.contains("[attached image:") && !content.contains("/tmp/"),
        "no path the model cannot open may be announced: {content}"
    );
    assert!(
        !content.contains("[attached image"),
        "the undelivered notice must be distinguishable from a path line: {content}"
    );
    assert!(
        !serde_json::to_string(&body).unwrap().contains("image_url"),
        "the agent loop cannot inline images"
    );
}
