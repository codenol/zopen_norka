//! Keyboard ownership of the comment composer.
//!
//! Issue #49 was found by driving the built app: the composer was drawn, the
//! click on it was fine, and yet nothing could be typed. The press tests had
//! proved the press path and the keyboard tests had proved the ladder, and both
//! were right — what neither of them asked was whether the host considered the
//! comment field a text input AT ALL.
//!
//! That one answer — `input_active` — is what decides three things a
//! reviewer cannot see: whether the browser's hidden IME capture input takes
//! DOM focus (and so whether a composed character, a dead key or an
//! `Input.insertText` payload has anywhere to land), whether a bare letter is
//! allowed to switch the canvas tool instead of being typed, and whether the
//! clipboard chords belong to the field or to the canvas. A comment field that
//! does not report itself as active loses all three silently.
//!
//! These tests drive that rule directly, without a DOM.

use super::WidgetHost;
use op_editor_core::editor_ui_state::{Comment, CommentAnchor, CommentAuthor, CommentThread};
use op_editor_core::Tool;

fn thread(id: i64, page: &str, name: &str) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, 100.0, 200.0)),
        created_at: 1_700_000_000,
        resolved: false,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![Comment {
            id: id * 10,
            author: CommentAuthor {
                id: Some(format!("u{id}")),
                name: name.to_string(),
                role: Some("ux_ui".to_string()),
            },
            body: "the spacing here looks off".to_string(),
            created_at: 1_700_000_000,
        }],
    }
}

/// A host with one thread ready to open its composer.
///
/// The thread is placed on the page the host's own document names — read from
/// the state rather than spelled, because that id is what the canvas and the
/// rail both key on.
fn host_with_thread() -> WidgetHost {
    let mut host = WidgetHost::new();
    let page = host.editor_state().active_page_identity().0;
    let state = host.editor_state_mut();
    state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_600_000.0;
    state.editor_ui.file_key = Some("key1".to_string());
    state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, &page, "Kay")]);
    host
}

fn open_composer(host: &mut WidgetHost) {
    host.editor_state_mut().editor_ui.comments.open(1);
    assert!(host.editor_state().editor_ui.comments.composer_focused);
}

#[test]
fn a_focused_composer_is_a_text_input_the_host_knows_about() {
    let mut host = host_with_thread();
    open_composer(&mut host);

    // Both answers matter and they are the same rule: `input_active` gates the
    // editor shortcuts, `text_input_focus_active` decides whether the hidden
    // IME input takes DOM focus in the browser. A field that is typed into but
    // not "active" gets neither.
    assert!(
        host.input_active(),
        "the comment field owns the keyboard while it is focused"
    );
    assert!(
        host.text_input_focus_active(),
        "and the browser's IME capture input is focused for it"
    );
    assert!(
        host.non_chat_input_owns_keyboard(),
        "so a paste belongs to the field, not to the canvas"
    );

    host.editor_state_mut().editor_ui.comments.blur_composer();
    assert!(!host.input_active(), "an unfocused composer owns nothing");
    assert!(!host.text_input_focus_active());
}

#[test]
fn a_closed_composer_never_keeps_the_keyboard() {
    // The flag and the composer are separate fields, and only both together
    // mean "this field is on screen and being typed into".
    let mut host = host_with_thread();
    host.editor_state_mut().editor_ui.comments.blur_composer();
    assert!(!host.input_active());
    host.editor_state_mut().editor_ui.comments.open(1);
    host.editor_state_mut().editor_ui.comments.close();
    assert!(!host.input_active(), "closed means closed, flag or not");
}

#[test]
fn a_composed_payload_reaches_the_comment_field() {
    // The browser delivers anything that is not a plain single-character
    // `keydown` through the hidden IME capture input: an IME commit, a dead
    // key, an emoji insertion, `Input.insertText`. All of it arrives as a text
    // payload on the field that owns the keyboard, and the comment draft is
    // that field — losing it is exactly the reported "nothing is typed".
    let mut host = host_with_thread();
    open_composer(&mut host);

    assert!(host.apply_paste_text("Тут нужен отступ"));
    assert_eq!(
        host.editor_state().editor_ui.comments.reply_draft,
        "Тут нужен отступ"
    );
}

#[test]
fn a_letter_that_names_a_tool_is_typed_instead_of_switching_the_tool() {
    // `r`, `t`, `v`, `f`, `p`, `y`, `o`, `l`, `h` are canvas tool shortcuts and
    // are ordinary letters in a sentence: "test" must not turn the reviewer's
    // editor into the Text tool and lose its `t`.
    let mut host = host_with_thread();
    open_composer(&mut host);
    assert_eq!(host.editor_state().tool, Tool::Select);

    for key in ["r", "t", "v", "f", "p", "y", "o", "l", "h"] {
        assert!(
            !host.apply_tool_shortcut(key),
            "{key:?} must not reach the tool router while a comment is being typed"
        );
        assert!(host.apply_text(key.chars().next().unwrap()));
    }
    assert_eq!(
        host.editor_state().tool,
        Tool::Select,
        "the canvas tool did not move"
    );
    assert_eq!(
        host.editor_state().editor_ui.comments.reply_draft,
        "rtvfpyolh"
    );
}

#[test]
fn select_all_while_commenting_does_not_select_the_canvas() {
    // The composer has no range-selection model, so Cmd/Ctrl+A has nothing to
    // select — but it must not act on the canvas behind the popover either.
    // Two top-level nodes, so "the chord changed nothing" can be told apart
    // from "there was only one node to select anyway".
    const TWO_NODES: &str = r#"{"version":"1.0.0","children":[
      {"type":"rectangle","id":"n1","name":"One","x":0,"y":0,"width":100,"height":100},
      {"type":"rectangle","id":"n2","name":"Two","x":300,"y":0,"width":100,"height":100}
    ]}"#;
    let doc = jian_ops_schema::load_str(TWO_NODES)
        .expect("the fixture parses")
        .value;
    let mut host = WidgetHost::new();
    host.editor_state = op_editor_core::EditorState::from_document(doc);
    host.editor_state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
    host.editor_state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, "n1", "Kay")]);
    host.editor_state_mut()
        .set_single_selection(op_editor_core::NodeId::new("n1"));
    open_composer(&mut host);

    assert!(host.apply_select_all(), "the chord is swallowed");
    assert_eq!(
        host.editor_state().selection.len(),
        1,
        "the canvas selection is exactly what it was"
    );
}

#[test]
fn enter_sends_the_comment_the_field_owns() {
    let mut host = host_with_thread();
    open_composer(&mut host);
    assert!(host.apply_text('o'));
    assert!(host.apply_text('k'));
    assert!(host.apply_send());
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![op_editor_core::editor_ui_state::CommentRequest::Reply {
            thread_id: 1,
            text: "ok".to_string(),
        }]
    );
}
