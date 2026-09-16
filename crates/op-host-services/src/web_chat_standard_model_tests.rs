use super::selected_model_id;
use super::*;
use std::sync::{Arc, Mutex};

/// A provider that remembers the turn it was asked for, so a route's budget
/// and thinking policy are observable instead of assumed.
struct RequestRecorder {
    seen: Arc<Mutex<Option<ChatRequest>>>,
}

impl ChatProvider for RequestRecorder {
    fn provider_label(&self) -> &str {
        "recorder"
    }

    fn send(&self, request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        *self.seen.lock().expect("seen lock") = Some(request);
        Box::new(std::iter::once(ChatDelta::Done {
            stop_reason: StopReason::EndTurn,
        }))
    }
}

fn recorded_modify_turn(model: Option<&str>) -> ChatRequest {
    let state = Mutex::new(super::tests::modify_target_state());
    let seen = Arc::new(Mutex::new(None));
    let provider = RequestRecorder { seen: seen.clone() };
    let plan = super::tests::modify_plan();

    super::stream_modify_route(
        &mut Vec::new(),
        plan,
        &provider,
        model,
        &state,
        &crate::web_canvas_server::SseHub::default(),
        None,
    )
    .expect("the modify turn answers");

    let guard = seen.lock().expect("seen lock");
    guard
        .as_ref()
        .cloned()
        .expect("the provider was asked for a turn")
}

/// A rewrite of a screen and the model's hidden reasoning share one output
/// budget. The configured model reasons until the budget is gone (measured:
/// 19 s of reasoning, 0 characters of answer, #179), and this route used to
/// leave thinking on for it — the documented cause of a rewrite that changed
/// the headers and not the data.
#[test]
fn modify_route_turns_thinking_off_for_a_model_that_starves_on_it() {
    let request = recorded_modify_turn(Some("deepseek-v4-flash-vision-exp"));
    assert_eq!(request.thinking, ThinkingMode::Disabled);
}

/// Models outside that list reason without starving content, so the route must
/// not override them — losing their thinking would be a quality regression.
#[test]
fn modify_route_keeps_thinking_for_a_model_that_does_not_starve() {
    let request = recorded_modify_turn(Some("claude-opus-4"));
    assert_eq!(request.thinking, ThinkingMode::Adaptive);
}

/// The rewrite budget stays what it is: the policy change is about reasoning,
/// not about how many tokens the answer gets.
#[test]
fn modify_route_budget_follows_the_rewrite_size() {
    assert_eq!(recorded_modify_turn(None).max_output_tokens, 8192);
}

#[test]
fn structured_builtin_keeps_its_concrete_model_for_standard_route_policy() {
    let request = crate::ai_proxy::parse_ai_stream_body(
        r#"{"builtinProviderId":"account:secondary","model":"builtin:account:secondary:shared:model","user":"hello"}"#,
    )
    .expect("request parses");

    assert_eq!(
        selected_model_id(&request, &op_editor_core::EditorState::new()).as_deref(),
        Some("shared:model")
    );
}

#[test]
fn old_web_structured_builtin_recovers_the_unique_saved_model_profile() {
    let request = crate::ai_proxy::parse_ai_stream_body(
        r#"{"provider":"codex-cli","model":"builtin:account:secondary:shared:model","user":"hello"}"#,
    )
    .expect("request parses");
    let mut snapshot = op_editor_core::EditorState::new();
    snapshot.editor_ui.agent_settings.add_builtin_agent_config(
        "Account",
        "sk-test",
        "shared:model",
        op_editor_core::BuiltinAgentKind::OpenAiCompat,
        "https://api.example.com/v1",
    );
    snapshot.editor_ui.agent_settings.builtin_agents[0].id = "account:secondary".into();

    assert_eq!(
        selected_model_id(&request, &snapshot).as_deref(),
        Some("shared:model")
    );
}
