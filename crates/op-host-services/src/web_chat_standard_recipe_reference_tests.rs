//! Route-level proof that a reference image changes what a web design turn is
//! based on (issue #65).
//!
//! The browser posts design turns to `/api/ai/standard`, and that route holds
//! the request's own attachment list — so whether the turn carries a picture is
//! known exactly there. Twice it decided otherwise: the pre-classification
//! placement read the fact, but `stream_new_design_route` placed a recipe from
//! the prompt's words alone, and once a recipe is placed it comes with the
//! `doc:recipe-base` Require rule telling the model to keep it. A person who
//! attached a screenshot and wrote nothing about it therefore still got a
//! library screen laid over their picture.
//!
//! These tests drive the real `stream_standard_turn` entry point against a
//! loopback capture server, so the assertion is about what the turn actually
//! did — the rule that left the process and the node that landed on the canvas
//! — rather than about a helper having been called.

use super::*;
use op_ai::chat_provider::EffortLevel;
use op_editor_core::{BuiltinAgentKind, NodeId};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

/// The recipe whose words the prompts below match, and the master a placement
/// would clone. Read from the kit rather than hard-coded, so a renamed recipe
/// fails loudly here instead of silently testing nothing.
const RECIPE_ID: &str = "ops-servers-screen";

/// The document a launched session opens: the starter frame plus the Skala
/// library masters, which is what a recipe placement clones.
///
/// `new_skala_editor_state` is the daemon's own entry point, but under
/// `cargo test` it deliberately finds no library (`skala_library_candidates`
/// returns nothing for a test binary, so unit tests neither pay for the 2.4 MB
/// merge nor drift File → New assertions). This test needs the masters — the
/// placement being guarded is a clone of one — so it merges the same file the
/// daemon merges, by an explicit path.
pub(super) fn daemon_session_document() -> EditorState {
    let mut state = EditorState::starter();
    let library =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/skala-spectrum.lib.op");
    op_pen_loader::merge_library_into_state(&mut state, &library.to_string_lossy())
        .expect("the repo ships the Skala library the daemon merges");
    state
}

/// The name the recipe's placed root carries — a clone of the library master
/// keeps the master's name.
pub(super) fn recipe_master_name(state: &EditorState) -> String {
    let recipe = op_editor_core::session_kit()
        .recipes
        .iter()
        .find(|r| r.id == RECIPE_ID)
        .expect("the kit still ships the ops recipe");
    state
        .components
        .find_by_id(&NodeId::new(recipe.template.clone()))
        .expect("the kit still ships the recipe's master")
        .name
        .clone()
}

/// A prompt that matches the ops recipe on the kit's own words.
pub(super) const RECIPE_PROMPT: &str = "собери экран: список коммутаторов с фильтром";

/// One PNG signature plus a payload byte, so the bytes are recognisably a
/// raster image to the sniffer on the provider path.
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
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
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

/// The parked capture thread plus the flag that retires it.
///
/// The stop flag has to be owned by the test: the thread blocks on a
/// non-blocking `accept` loop, so nothing else — not the listener, not the
/// channel — ever ends it, and joining it without this hangs the test binary.
struct CaptureServer {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl CaptureServer {
    fn stop(self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.thread.join();
    }
}

/// Serve every request the turn makes with a short OpenAI-shaped completion,
/// recording each body. A real listener is used rather than a stub
/// `ChatProvider` because the whole route — classification, placement, planning
/// — has to run for the assertion to mean anything.
fn capture_endpoint() -> (String, std_mpsc::Receiver<String>, CaptureServer) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let addr = listener.local_addr().expect("capture server address");
    listener
        .set_nonblocking(true)
        .expect("nonblocking capture server");
    let (tx, rx) = std_mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    // `stop` is returned inside the guard below, so the loop can be ended.
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking read");
                    let request = read_http_request(&mut stream);
                    let _ = tx.send(request);
                    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                                data: [DONE]\n\n";
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
    (
        format!("http://{addr}/v1"),
        rx,
        CaptureServer {
            stop,
            thread: server,
        },
    )
}

/// A daemon-owned built-in provider. The id deliberately does NOT carry the
/// browser-owned `builtin-` prefix, so the route dials it under the trusted
/// policy and the loopback endpoint needs no allowlist entry.
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

pub(super) fn design_turn(
    prompt: &str,
    attachments: Vec<ChatAttachment>,
) -> WebStandardTurnRequest {
    WebStandardTurnRequest {
        ai: AiStreamRequest {
            provider: None,
            builtin_provider_id: None,
            model: "vision-model".into(),
            skills: Vec::new(),
            user: prompt.into(),
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

/// Drive one real design turn on a fresh session canvas and hand back the
/// document it left behind, the document it started from, and every request the
/// provider saw.
fn run_turn(
    prompt: &str,
    attachments: Vec<ChatAttachment>,
) -> (
    Vec<String>,
    op_editor_core::EditorState,
    op_editor_core::EditorState,
) {
    let (base_url, rx, server) = capture_endpoint();
    let mut editor = daemon_session_document();
    editor
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(agent_config("test-daemon-agent", base_url));
    let before = editor.clone();
    let state = Mutex::new(WebCanvasState::new(editor, 3100));
    let mut out = Vec::new();

    stream_standard_turn(
        &mut out,
        design_turn(prompt, attachments),
        &state,
        &SseHub::default(),
        None,
        None,
    )
    .expect("the turn streams");

    let mut bodies = Vec::new();
    while let Ok(request) = rx.recv_timeout(Duration::from_millis(250)) {
        bodies.push(request);
    }
    server.stop();

    assert!(
        !bodies.is_empty(),
        "the design turn must reach the provider at all; SSE was: {}",
        String::from_utf8_lossy(&out)
    );
    let editor = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .editor
        .clone();
    (bodies, editor, before)
}

fn node_names(state: &op_editor_core::EditorState) -> Vec<String> {
    state
        .active_children()
        .iter()
        .map(|node| {
            use op_editor_core::PenNodeExt as _;
            node.base().name.clone().unwrap_or_default()
        })
        .collect()
}

/// Issue #65, the half the route still got wrong. An attached picture with no
/// word about it is a reference turn, so no recipe may be placed, and the
/// `doc:recipe-base` rule that a placement adds may not reach the model.
#[test]
fn an_attached_picture_keeps_the_recipe_off_a_matching_prompt() {
    // The prompt on its own matches the ops recipe, so this turn would be based
    // on it — the attachment is the only thing standing in the way.
    let (bodies, editor, before) = run_turn(
        RECIPE_PROMPT,
        vec![ChatAttachment {
            name: "reference.png".into(),
            media_type: "image/png".into(),
            data: png_bytes(),
        }],
    );

    let master_name = recipe_master_name(&before);
    for body in &bodies {
        assert!(
            !body.contains("Recipe already placed"),
            "a reference turn must not be told to adapt a recipe the product \
             placed over the user's picture: {body}"
        );
    }
    assert!(
        !node_names(&editor).iter().any(|name| name == &master_name),
        "the recipe's master ({master_name}) must not be on the canvas of a \
         reference turn; the canvas holds: {:?}",
        node_names(&editor)
    );
}

/// The control: the same words with nothing attached still get their recipe, so
/// the gate above is the attachment and not a blanket "never place a recipe".
#[test]
fn the_same_prompt_without_a_picture_still_gets_the_recipe() {
    let (_bodies, editor, before) = run_turn(RECIPE_PROMPT, Vec::new());
    let master_name = recipe_master_name(&before);

    assert!(
        node_names(&editor).iter().any(|name| name == &master_name),
        "a turn with no picture attached asks for the ops recipe in its own \
         words — it must still get it; the canvas holds: {:?}",
        node_names(&editor)
    );
}
