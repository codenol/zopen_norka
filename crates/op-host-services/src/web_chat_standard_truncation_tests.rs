//! A reply that was cut off is not a whole screen (issue #205).
//!
//! Every reply in the corpus that carried a document was cut off at the output
//! budget — `#2` mid-statement (`…fontFamily:"Rob`, 389 `{` against 380 `}`),
//! `#4`/`#6`/`#8` at exactly 16 384 SSE delta events, the request's
//! `max_output_tokens`. The partial tree was applied anyway and reported
//! `<!-- APPLIED -->` + `done`.
//!
//! The assertion here is about what the reply *says* and whether a partial
//! statement was applied: an incomplete reply is refused with the reason, and
//! the canvas is left as the turn found it, while the control proves a whole
//! reply still lands.

use super::recipe_reference_tests::design_turn;
use super::tests::{modify_plan, modify_target_state, ScriptedProvider};
use super::*;
use crate::web_chat_standard::recipe::{truncation_of, Truncation};

fn document_version(state: &Mutex<WebCanvasState>) -> u64 {
    state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .document_version_for_test()
}

/// The first root of the active page, as the document serialises it — what the
/// canvas actually holds after the turn.
fn live_root(state: &Mutex<WebCanvasState>) -> Value {
    let guard = state.lock().unwrap_or_else(|p| p.into_inner());
    serde_json::to_value(&guard.editor.active_children()[0]).expect("node serialises")
}

fn child_names(root: &Value) -> Vec<String> {
    root["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|child| child["name"].as_str().map(str::to_string))
        .collect()
}

/// The shape of the cut the corpus measured. `#2` was one statement that never
/// closed (`…fontFamily:"Rob`, 389 `{` against 380 `}`) and the parser still
/// recovered a fragment out of it — 214 nodes landed under the root — which is
/// exactly why the daemon applied it and called it a screen. This fixture is the
/// same shape in miniature: a statement whose inner objects are complete enough
/// to be salvaged, cut inside its last one.
///
/// That the reply still parses is the point of the test, so it is asserted
/// rather than assumed.
const CUT_SHORT_MODIFY_REPLY: &str = r##"<step title="Checking guidelines">Analyzing modification request...</step>
I("n217", {type:"frame", name:"Renamed", children:[{type:"text", name:"title", content:"Server list"}], width:100, height:100, layout:"vertical", fill:[{type:"solid", color:"#111111"##;

/// The same reply, whole. Applied in the control below, so the refusal above is
/// the truncation and not a reply the route could not parse.
const WHOLE_MODIFY_REPLY: &str = r##"<step title="Checking guidelines">Analyzing modification request...</step>
I("n217", {type:"frame", name:"Renamed", children:[{type:"text", name:"title", content:"Server list"}], width:100, height:100, layout:"vertical", fill:[{type:"solid", color:"#111111"}]});"##;

/// A provider that streams one reply and reports why it stopped. The stop reason
/// is the client's own honesty channel (`ChatDelta::Done { stop_reason }`), so a
/// test that wants a cut reply has to be able to send one.
struct StoppedProvider {
    response: String,
    stop_reason: StopReason,
}

impl ChatProvider for StoppedProvider {
    fn provider_label(&self) -> &str {
        "scripted"
    }

    fn send(&self, _request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        Box::new(
            [
                ChatDelta::TextDelta(self.response.clone()),
                ChatDelta::Done {
                    stop_reason: self.stop_reason,
                },
            ]
            .into_iter(),
        )
    }
}

fn run_modify(state: &Mutex<WebCanvasState>, provider: &dyn ChatProvider) -> String {
    let mut out = Vec::new();
    stream_modify_route(
        &mut out,
        modify_plan(),
        provider,
        None,
        state,
        &SseHub::default(),
        None,
    )
    .expect("the modify turn answers");
    String::from_utf8(out).expect("utf8 sse")
}

#[test]
fn the_fixture_reply_really_is_a_parsable_but_incomplete_statement() {
    // The test below is only about truncation if the cut reply would otherwise
    // have been applied: the parser has to recover nodes out of it, and the
    // statement has to be unterminated.
    assert!(
        !crate::chat_intent::parse_modify_nodes(CUT_SHORT_MODIFY_REPLY).is_empty(),
        "the cut fixture must still parse into nodes, or the refusal below would \
         be the parser's doing and not the truncation rule's"
    );
    assert_eq!(
        truncation_of(CUT_SHORT_MODIFY_REPLY, Some(StopReason::EndTurn)),
        Some(Truncation::UnterminatedStatement)
    );
}

#[test]
fn a_reply_that_never_closes_its_statement_is_refused_and_says_so() {
    // No stop reason worth trusting — the case the built-in paths that hard-code
    // `EndTurn` leave behind. The reply's own shape is the symptom, and the
    // applier must not turn half a statement into a screen.
    let state = Mutex::new(modify_target_state());
    let before_version = document_version(&state);
    let streamed = run_modify(
        &state,
        &ScriptedProvider {
            response: CUT_SHORT_MODIFY_REPLY.to_string(),
        },
    );

    assert!(
        streamed.contains("never closes"),
        "the sentence has to name the symptom: {streamed}"
    );
    assert!(
        streamed.contains("Nothing was applied"),
        "and has to say that it applied nothing: {streamed}"
    );
    assert!(
        !streamed.contains("<!-- APPLIED -->"),
        "a half statement is not a screen: {streamed}"
    );
    let root = live_root(&state);
    assert_eq!(root["name"], Value::from("Card"), "the root is untouched");
    assert!(
        child_names(&root).is_empty(),
        "the fragment never landed: {:?}",
        child_names(&root)
    );
    assert_eq!(
        document_version(&state),
        before_version,
        "nothing advanced it"
    );
}

#[test]
fn a_reply_that_stopped_at_the_output_budget_is_not_applied_even_when_it_parses() {
    // The provider says why it stopped, and that is the one signal no reading of
    // the text can recover: this reply is balanced and parses cleanly, and it is
    // still only what fitted. Without the stop reason the route would have
    // applied it — which the control below shows.
    let state = Mutex::new(modify_target_state());
    let before_version = document_version(&state);
    let streamed = run_modify(
        &state,
        &StoppedProvider {
            response: WHOLE_MODIFY_REPLY.to_string(),
            stop_reason: StopReason::MaxTokens,
        },
    );

    assert!(
        streamed.contains("ran out of output budget"),
        "the reply has to say why nothing was applied: {streamed}"
    );
    assert!(
        !streamed.contains("<!-- APPLIED -->"),
        "a reply the provider cut may not be reported as applied: {streamed}"
    );
    let root = live_root(&state);
    assert_eq!(root["name"], Value::from("Card"), "the root is untouched");
    assert!(
        child_names(&root).is_empty(),
        "and so is everything under it: {:?}",
        child_names(&root)
    );
    assert_eq!(
        document_version(&state),
        before_version,
        "nothing advanced it"
    );
}

#[test]
fn the_control_the_same_reply_with_a_whole_stop_reason_is_applied() {
    let state = Mutex::new(modify_target_state());
    let before_version = document_version(&state);
    let streamed = run_modify(
        &state,
        &StoppedProvider {
            response: WHOLE_MODIFY_REPLY.to_string(),
            stop_reason: StopReason::EndTurn,
        },
    );

    assert!(
        streamed.contains("<!-- APPLIED -->"),
        "a whole reply is applied exactly as before: {streamed}"
    );
    assert!(
        !streamed.contains("cut off"),
        "and is not annotated as cut off: {streamed}"
    );
    let root = live_root(&state);
    assert!(
        child_names(&root).contains(&"Renamed".to_string()),
        "the reply's content is on the canvas: {:?}",
        child_names(&root)
    );
    assert!(
        document_version(&state) > before_version,
        "and the document version moved, as every applied turn's does"
    );
}

#[test]
fn a_chat_reply_cut_at_the_output_budget_says_so() {
    // The chat route applies nothing, so a cut reply costs no nodes — but it
    // still ends a `done` turn with an answer that stops mid-statement, which is
    // what the three chat-route replies of the corpus were (16 384 deltas, the
    // request's whole budget).
    let provider = StoppedProvider {
        response: "Here is the screen you asked for: I(null, {id:\"n1\", children:[".to_string(),
        stop_reason: StopReason::MaxTokens,
    };
    let mut out = Vec::new();

    stream_chat_route(
        &mut out,
        &design_turn("what is a frame?", Vec::new()),
        &EditorState::starter(),
        &provider,
        None,
    )
    .expect("the chat route answers");

    let streamed = String::from_utf8(out).expect("utf8 sse");
    assert!(
        streamed.contains("cut off at the model's output budget"),
        "a cut answer may not pass as the whole answer: {streamed}"
    );
    assert!(
        streamed.contains(r#""done":true"#),
        "the turn still ends normally: {streamed}"
    );
}

#[test]
fn the_control_a_whole_chat_reply_is_not_reported_as_cut_off() {
    let provider = StoppedProvider {
        response: "A frame is a container node that holds other nodes.".to_string(),
        stop_reason: StopReason::EndTurn,
    };
    let mut out = Vec::new();

    stream_chat_route(
        &mut out,
        &design_turn("what is a frame?", Vec::new()),
        &EditorState::starter(),
        &provider,
        None,
    )
    .expect("the chat route answers");

    let streamed = String::from_utf8(out).expect("utf8 sse");
    assert!(
        !streamed.contains("cut off"),
        "a whole answer must not be annotated: {streamed}"
    );
}

#[test]
fn the_truncation_rule_reads_braces_outside_strings() {
    // A `{` in a label is text, not a statement, and an escaped quote does not
    // end the literal.
    assert_eq!(
        truncation_of(
            r#"I(null, {id:"n1", content:"{not a brace}"});"#,
            Some(StopReason::EndTurn)
        ),
        None
    );
    assert_eq!(
        truncation_of(
            r#"I(null, {id:"n1", content:"a \" brace"});"#,
            Some(StopReason::EndTurn)
        ),
        None
    );
    // A statement that never closes is the symptom when the provider says
    // nothing useful.
    assert_eq!(
        truncation_of(r#"I(null, {id:"n1""#, Some(StopReason::EndTurn)),
        Some(Truncation::UnterminatedStatement)
    );
    // The provider's own verdict wins over any reading of the text.
    assert_eq!(
        truncation_of("all done.", Some(StopReason::MaxTokens)),
        Some(Truncation::OutputBudget)
    );
    assert_eq!(truncation_of("all done.", Some(StopReason::EndTurn)), None);
}
