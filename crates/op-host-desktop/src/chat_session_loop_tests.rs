//! Loop-finalize + tab-binding + design-turn starter-frame tests for
//! `ChatSession`, split from `chat_session_tests.rs` to keep both files
//! under the repo's 800-line cap. Wired in as `chat_session::loop_tests`
//! so `use super::*` still resolves against `chat_session` itself.

use super::launch::clear_fresh_starter_frame_for_design;
use super::*;
use op_ai::chat_history::{trim_chat_history, DEFAULT_MAX_CHARS, DEFAULT_MAX_MESSAGES};
use op_ai::chat_provider::{ChatDelta, ChatRequest, EchoProvider, StopReason};
use op_editor_core::ChatMessage;
use op_host_services::chat_system_prompt::{
    build_chat_system_prompt, chat_history_from_transcript,
};

/// Scripted provider for the Step-4 finalize proof: it inserts a roleless
/// `Header` frame via a tool call, then calls `executor.finalize()` (the
/// reserved loop-finalize op), then streams Done — exercising the host-side
/// `LOOP_FINALIZE_OP` interception against the live document.
struct FinalizeLoopProvider {
    executor: std::sync::Arc<dyn op_ai::chat_provider::ChatToolExecutor>,
}

struct FinalizeLoopIter {
    executor: std::sync::Arc<dyn op_ai::chat_provider::ChatToolExecutor>,
    step: u8,
}

impl Iterator for FinalizeLoopIter {
    type Item = ChatDelta;
    fn next(&mut self) -> Option<ChatDelta> {
        self.step += 1;
        match self.step {
            1 => {
                // Insert a roleless frame named "Header" — role inference in
                // the backstop must later assign it the navbar role.
                let args = r#"{"kind":"frame","name":"Header","x":"0","y":"0","width":"1200","height":"64"}"#;
                let _ = self.executor.execute("insert_node", args);
                Some(ChatDelta::TextDelta("inserted".into()))
            }
            2 => {
                // Loop-end: run the structural backstop against the live doc.
                self.executor.finalize();
                Some(ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                })
            }
            _ => None,
        }
    }
}

impl op_ai::chat_provider::ChatProvider for FinalizeLoopProvider {
    fn provider_label(&self) -> &str {
        "finalize-loop"
    }
    fn send(&self, _request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        Box::new(FinalizeLoopIter {
            executor: self.executor.clone(),
            step: 0,
        })
    }
}

#[test]
fn pump_runs_loop_finalize_backstop_against_live_state() {
    // Proof for Track-1 Step 4: the loop-end `finalize()` flows worker → tool
    // channel → `execute_tool_requests` LOOP_FINALIZE_OP interception →
    // `op_orchestrator::apply_loop_finalize` against the live document, so a
    // roleless "Header" frame gets the navbar role resolved.
    use op_editor_core::PenNodeExt;
    let mut host = WidgetHostNative::new();
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant_streaming());

    let (executor, tool_rx) = op_host_services::chat_canvas_tools::chat_tool_channel();
    let provider = Box::new(FinalizeLoopProvider {
        executor: std::sync::Arc::new(executor),
    });
    let mut current = Some(ChatSession::start_with_tools(
        provider,
        ChatRequest {
            user_message: "build a header".into(),
            max_output_tokens: 64,
            ..Default::default()
        },
        Some(tool_rx),
    ));

    for _ in 0..2000 {
        pump(&mut host, &mut current, None, None, (1200.0, 800.0));
        if current.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(current.is_none(), "turn must finish");

    // The backstop resolved the roleless Header frame's navbar role on the
    // live document.
    let header = host
        .editor_state()
        .active_children()
        .iter()
        .find(|n| n.base().name.as_deref() == Some("Header"))
        .expect("Header frame inserted");
    assert_eq!(
        header.base().role.as_deref(),
        Some("navbar"),
        "loop-end finalize must run apply_loop_finalize against the live state"
    );
}

#[test]
fn pump_writes_to_bound_tab_not_the_active_tab() {
    // MT.3 session-per-tab: a run launched on tab 0 must keep streaming into
    // tab 0 even after the user switches the active tab to a new tab 1.
    let mut host = WidgetHostNative::new();
    // Tab 0 sends and gets the streaming assistant bubble.
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant_streaming());

    let provider = Box::new(EchoProvider {
        script: vec![
            ChatDelta::TextDelta("Hel".into()),
            ChatDelta::TextDelta("lo".into()),
            ChatDelta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ],
    });
    let mut current = Some(ChatSession::start(
        provider,
        ChatRequest {
            user_message: "hi".into(),
            max_output_tokens: 256,
            ..Default::default()
        },
    ));

    // Run is bound to tab 0; the user then opens + switches to tab 1.
    host.editor_state_mut().chat.new_tab();
    assert_eq!(host.editor_state().chat.active_index(), 1);

    for _ in 0..2000 {
        // running_tab = Some(0) — NOT the active tab (1).
        pump(&mut host, &mut current, Some(0), None, (1200.0, 800.0));
        if current.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(current.is_none(), "turn must finish");

    // The deltas landed in tab 0 (the bound tab), not the active tab 1.
    let tab1_msgs = host.editor_state().chat.messages.len();
    assert_eq!(tab1_msgs, 0, "active tab 1 must stay empty");
    host.editor_state_mut().chat.switch_to(0);
    let tab0 = host.editor_state().chat.messages.last().unwrap();
    assert_eq!(tab0.content, "Hello");
    assert!(!tab0.streaming);
}

#[test]
fn launch_populates_system_prompt_and_history() {
    // The per-turn request assembly (GAP #31): transcript → trimmed
    // history; the system prompt resolves per-turn. Verified through
    // the helpers launch_if_pending composes.
    let mut host = WidgetHostNative::new();
    let chat = &mut host.editor_state_mut().chat;
    chat.messages.push(ChatMessage::user("first question"));
    chat.messages.push(ChatMessage::assistant("first answer"));
    chat.messages.push(ChatMessage::user("make it red"));
    chat.messages.push(ChatMessage::assistant_streaming());

    let history = trim_chat_history(
        &chat_history_from_transcript(&host.editor_state().chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(
        history.len(),
        2,
        "prior turns only — in-flight turn excluded"
    );
    assert_eq!(history[0].1, "first question");
    assert_eq!(history[1].1, "first answer");

    let system = build_chat_system_prompt(host.editor_state(), "make it red");
    // The prompt is assembled from the resolved skills plus the rules policy,
    // so it no longer opens with a fixed sentence. What matters is that the
    // session's rules reached it and that no template placeholder leaked in —
    // the old assertion pinned a literal that the rules work removed, which
    // made this test fail for a change that was correct.
    assert!(
        system.contains("WORKING AGREEMENT"),
        "the session's rules must lead the prompt: {system:.200}"
    );
    assert!(
        !system.contains("{{"),
        "an unresolved template placeholder reached the model: {system:.200}"
    );
}

#[test]
fn design_turn_clears_fresh_starter_frame() {
    let mut state = op_editor_core::EditorState::starter();

    assert!(clear_fresh_starter_frame_for_design(&mut state));
    assert!(state.active_children().is_empty());
    assert!(state.selection.is_empty());
}

#[test]
fn design_turn_clears_loaded_empty_starter_frame() {
    let mut state = op_editor_core::EditorState::starter();
    state.doc.version = "1.0.0".into();
    state.clear_selection();

    assert!(clear_fresh_starter_frame_for_design(&mut state));
    assert!(state.active_children().is_empty());
    assert!(state.selection.is_empty());
}

#[test]
fn design_turn_preserves_non_starter_documents() {
    let mut state = op_editor_core::EditorState::starter();
    state.active_children_mut().clear();

    assert!(!clear_fresh_starter_frame_for_design(&mut state));
    assert!(state.active_children().is_empty());
}

#[test]
fn design_turn_preserves_starter_frame_with_user_content() {
    let mut state = op_editor_core::EditorState::starter();
    let mut next_id = 20;
    state.create_node_for_tool(
        op_editor_core::Tool::Rect,
        &mut next_id,
        24.0,
        32.0,
        120.0,
        80.0,
    );

    assert!(!clear_fresh_starter_frame_for_design(&mut state));
    assert_eq!(state.active_children().len(), 2);
}

#[test]
fn starter_ghost_lives_from_clear_until_design_root_or_idle() {
    use super::launch::reconcile_starter_ghost;

    // Clearing the starter captures its rect as the ghost.
    let mut state = op_editor_core::EditorState::starter();
    assert!(clear_fresh_starter_frame_for_design(&mut state));
    assert_eq!(
        state.editor_ui.starter_ghost,
        Some([0.0, 0.0, 1200.0, 800.0]),
        "ghost snapshots the starter rect"
    );

    // Session running, canvas still empty → ghost stays.
    assert!(!reconcile_starter_ghost(&mut state, true));
    assert!(state.editor_ui.starter_ghost.is_some());

    // The design root lands → ghost retires.
    let root: jian_ops_schema::node::PenNode = serde_json::from_str(
        r#"{ "type": "frame", "id": "d1", "name": "Music Home", "width": 402, "height": 874 }"#,
    )
    .expect("root");
    state.active_children_mut().push(root);
    assert!(reconcile_starter_ghost(&mut state, true));
    assert!(state.editor_ui.starter_ghost.is_none());

    // Turn dies with nothing produced → ghost also retires.
    let mut failed = op_editor_core::EditorState::starter();
    assert!(clear_fresh_starter_frame_for_design(&mut failed));
    assert!(reconcile_starter_ghost(&mut failed, false));
    assert!(failed.editor_ui.starter_ghost.is_none());
}
