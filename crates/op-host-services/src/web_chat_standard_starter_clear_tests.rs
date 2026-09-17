//! What a turn that draws nothing does to the page (issue #202).
//!
//! Two of the eight corpus prompts ended with an **empty canvas**: an English
//! design keyword made `op_orchestrator::classify_intent` return `Design`, so
//! the route cleared the starter frame *before* it was routed, and the second
//! classifier then picked the chat route — which never writes to the document.
//! `pages[0]` went from 1 node to 0, the turn reported `done`, and the single
//! version bump of the turn *was* the deletion.
//!
//! The assertion here is about what the canvas holds: a turn that ends on the
//! chat route must leave the page as it found it, and the control below pins the
//! other half of the rule — a turn that draws still drops the blank starter frame
//! (issue #184).

use super::recipe_reference_tests::{
    agent_config, capture_endpoint, daemon_session_document, design_turn, recipe_master_name,
    CaptureServer, RECIPE_PROMPT,
};
use super::*;

// ── #202: the starter clear is a document mutation, and it belongs to a turn
//    that will draw ──────────────────────────────────────────────────────────

/// The two English prompts of the corpus, verbatim (`.openpencil-tmp/gq2`).
/// Both are a `Design` word for `op_orchestrator::classify_intent` — which is
/// what made the clear fire before the route was known — and both were answered
/// by the chat route, which writes nothing.
const SETTINGS_EN_PROMPT: &str = "Design a settings page with sections Profile, Notifications, \
                                  Security and a Save button, plus a left navigation list";
const PRICING_EN_PROMPT: &str = "Create a pricing page with three plan cards, monthly and yearly \
                                 toggle, and a FAQ section";

/// Every node name in the active page, depth first, as the document serialises —
/// the page's actual contents rather than a node count.
fn active_page_inventory(state: &Mutex<WebCanvasState>) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        if let Some(name) = value.get("name").and_then(Value::as_str) {
            out.push(name.to_string());
        }
        for child in value
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(child, out);
        }
    }
    let guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let roots: Vec<Value> = guard
        .editor
        .active_children()
        .iter()
        .map(|node| serde_json::to_value(node).unwrap_or(Value::Null))
        .collect();
    let mut names = Vec::new();
    for root in &roots {
        walk(root, &mut names);
    }
    names
}

fn document_version(state: &Mutex<WebCanvasState>) -> u64 {
    state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .document_version_for_test()
}

/// A daemon-shaped session — starter frame plus the Skala kit, which is what
/// `/api/file/new` leaves behind — whose one built-in agent answers **every**
/// call with a plain `ok`. That answer carries no routing tag, so the route
/// classifier falls back to chat: the misroute of the measurement, reproduced
/// without a model and without guessing what the model saw.
fn chat_routed_session() -> (
    Mutex<WebCanvasState>,
    std::sync::mpsc::Receiver<String>,
    CaptureServer,
) {
    let (base_url, rx, server) = capture_endpoint();
    let mut editor = daemon_session_document();
    editor
        .editor_ui
        .agent_settings
        .builtin_agents
        .push(agent_config("test-daemon-agent", base_url));
    (Mutex::new(WebCanvasState::new(editor, 3100)), rx, server)
}

#[test]
fn a_design_worded_turn_that_routes_to_chat_leaves_the_page_as_it_found_it() {
    // The invariant, at the route: the clear may only happen on a turn that will
    // draw. A turn that answers in words must leave the page — and the version
    // the browser polls — exactly as it found them.
    for prompt in [SETTINGS_EN_PROMPT, PRICING_EN_PROMPT] {
        assert!(
            matches!(
                op_orchestrator::classify_intent(prompt),
                op_orchestrator::Intent::Design
            ),
            "the fixture prompt must be a design word for the keyword classifier, or this test \
             would not exercise the clear at all: {prompt}"
        );
        let (state, rx, server) = chat_routed_session();
        let before = active_page_inventory(&state);
        let before_version = document_version(&state);
        assert_eq!(
            before.len(),
            1,
            "the daemon opens on nothing but its blank starter frame: {before:?}"
        );

        let mut out = Vec::new();
        stream_standard_turn(
            &mut out,
            design_turn(prompt, Vec::new()),
            &state,
            &SseHub::default(),
            None,
            None,
        )
        .expect("the turn streams");

        while rx
            .recv_timeout(std::time::Duration::from_millis(250))
            .is_ok()
        {}
        server.stop();
        let streamed = String::from_utf8(out).expect("utf8 sse");

        assert!(
            streamed.contains(r#""delta":"ok""#),
            "the turn must be answered by the chat route — it streams the provider's own words \
             and never touches the document; SSE was: {streamed}"
        );
        assert!(
            !streamed.contains("<!-- APPLIED -->"),
            "no route applied anything: {streamed}"
        );
        assert_eq!(
            active_page_inventory(&state),
            before,
            "a turn that drew nothing must leave the page as it found it ({prompt})"
        );
        assert_eq!(
            document_version(&state),
            before_version,
            "and must not bump the version either: the version bump of the failing turn *was* \
             the deletion ({prompt})"
        );
    }
}

#[test]
fn the_control_a_design_worded_turn_that_draws_still_drops_the_starter_frame() {
    // The other half of the rule. The recipe prompt matches the kit's own words,
    // so the host places that base and the turn rewrites it: the page must NOT
    // be left holding the starter frame beside the placed screen (issue #184).
    let (bodies, editor, before) = {
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
            design_turn(RECIPE_PROMPT, Vec::new()),
            &state,
            &SseHub::default(),
            None,
            None,
        )
        .expect("the turn streams");
        let mut bodies = Vec::new();
        while let Ok(request) = rx.recv_timeout(std::time::Duration::from_millis(250)) {
            bodies.push(request);
        }
        server.stop();
        let editor = state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .editor
            .clone();
        (bodies, editor, before)
    };

    assert!(!bodies.is_empty(), "the turn must reach the provider");
    let master_name = recipe_master_name(&before);
    let names: Vec<String> = editor
        .active_children()
        .iter()
        .map(|node| {
            use op_editor_core::PenNodeExt as _;
            node.base().name.clone().unwrap_or_default()
        })
        .collect();
    assert!(
        names.iter().any(|name| name == &master_name),
        "the drawing route still places its recipe base: {names:?}"
    );
    assert_eq!(
        names.len(),
        1,
        "and the blank starter frame is not left beside it: {names:?}"
    );
}
