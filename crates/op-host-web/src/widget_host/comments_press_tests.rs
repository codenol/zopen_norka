//! Press coverage for the web host's comment surfaces.
//!
//! Drives the real composition pass (`paint_editor`) and then the real press
//! ladder, because the two halves are what can drift: the panel and the popover
//! are placed against the canvas region and the conversation's length, so a
//! press must hit-test the rects the paint actually used. Asserting on the
//! shared flow alone would not catch the host caching them.

use super::WidgetHost;
use op_editor_core::editor_ui_state::{Comment, CommentAuthor, CommentRequest, CommentThread};
use op_editor_core::NodeId;
use op_editor_ui::widgets::comments_panel::CommentsToggle;
use op_editor_ui::{Color, Point2D, Rect, RenderBackend, TextLayout};

const W: f32 = 1440.0;
const H: f32 = 900.0;

/// Recording backend — enough of the trait for a full composition pass.
#[derive(Default)]
struct CaptureBackend {
    texts: Vec<String>,
}

impl RenderBackend for CaptureBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _: Point2D) {
        for run in layout.runs() {
            self.texts.push(run.content.clone());
        }
    }
    fn clip_rect(&mut self, _: Rect) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn thread(id: i64, node: &str, name: &str) -> CommentThread {
    CommentThread {
        id,
        node_id: node.to_string(),
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

/// A host with one thread about `n1` and a document key, so requests can travel.
fn host_with_thread() -> WidgetHost {
    let mut host = WidgetHost::new();
    let state = host.editor_state_mut();
    state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_600_000.0;
    state.editor_ui.file_key = Some("key1".to_string());
    state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, "n1", "Kay")]);
    host
}

fn centre(rect: Rect) -> (f32, f32) {
    (
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

/// Paint one frame; the host caches the rects its press arms use.
fn paint(host: &mut WidgetHost) {
    let mut backend = CaptureBackend::default();
    host.paint_editor(&mut backend, W, H);
}

#[test]
fn the_pill_paints_shut_and_the_host_press_opens_the_panel() {
    let mut host = host_with_thread();
    paint(&mut host);
    let rect = host.comments_toggle_rect.expect("the pill painted");
    assert!(host.comments_panel_rect.is_none(), "the panel is shut");

    let (x, y) = centre(rect);
    assert!(host.apply_press(x, y, W, H));
    assert!(host.editor_state().editor_ui.comments.panel_open);
    // Opening the panel asks for the conversation, because the daemon pushes no
    // signal for comments.
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Reload]
    );
}

#[test]
fn with_the_panel_open_the_pill_is_gone_and_the_panel_takes_the_press() {
    let mut host = host_with_thread();
    host.editor_state_mut().editor_ui.comments.panel_open = true;
    paint(&mut host);
    assert!(host.comments_toggle_rect.is_none());
    let panel = host.comments_panel_rect.expect("the panel painted");

    // The arm row, which is where a review starts a pin.
    let arm = op_editor_ui::widgets::comments_panel::CommentsPanel::arm_rect(panel);
    let (x, y) = centre(arm);
    assert!(host.apply_press(x, y, W, H));
    assert!(host.editor_state().editor_ui.comments.pin_mode);
}

#[test]
fn a_row_press_opens_the_thread_and_asks_the_canvas_to_frame_its_element() {
    let mut host = host_with_thread();
    host.editor_state_mut().editor_ui.comments.panel_open = true;
    paint(&mut host);
    let panel = host.comments_panel_rect.expect("the panel painted");
    let rows = op_editor_ui::widgets::comments_panel::CommentsPanel::new(
        op_editor_ui::theme::Theme::dark(),
        op_editor_core::editor_ui_state::Locale::EnUs,
        op_editor_ui::widgets::comments_panel::rows(
            &host.editor_state().editor_ui.comments,
            op_editor_core::editor_ui_state::Locale::EnUs,
            None,
            |_| false,
        ),
        false,
        false,
        None,
        0.0,
    )
    .row_rects(panel);
    let (x, y) = centre(rows[0]);
    host.apply_press(x, y, W, H);
    assert!(host.editor_state().editor_ui.comments.is_open(1));
    // The element is not in this host's (empty) document, so nothing was
    // selected — the row still opens the conversation, which is the point.
    assert!(host.editor_state().selection.is_empty());
}

#[test]
fn escape_dismisses_the_popover_then_the_armed_mode_then_the_selection() {
    let mut host = host_with_thread();
    let state = host.editor_state_mut();
    state.editor_ui.comments.open(1);
    state.editor_ui.comments.set_pin_mode(true);
    state.set_single_selection(NodeId::new("n1"));

    assert!(host.apply_escape());
    assert!(host.editor_state().editor_ui.comments.composer().is_none());
    assert!(host.apply_escape());
    assert!(!host.editor_state().editor_ui.comments.pin_mode);
    assert!(!host.editor_state().selection.is_empty());
    assert!(host.apply_escape());
    assert!(host.editor_state().selection.is_empty());
}

#[test]
fn a_focused_composer_owns_the_keyboard() {
    let mut host = host_with_thread();
    let state = host.editor_state_mut();
    state.editor_ui.comments.open(1);
    state.editor_ui.comments.focus_composer();

    assert!(host.apply_text('h'));
    assert!(host.apply_text('i'));
    assert_eq!(host.editor_state().editor_ui.comments.reply_draft, "hi");
    // The tool must not have changed behind the typed text.
    assert_eq!(host.editor_state().tool, op_editor_core::Tool::Select);

    assert!(host.apply_backspace());
    assert_eq!(host.editor_state().editor_ui.comments.reply_draft, "h");

    assert!(host.apply_send());
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Reply {
            thread_id: 1,
            text: "h".to_string(),
        }]
    );
}

#[test]
fn a_composer_that_is_not_focused_does_not_eat_the_keyboard() {
    let mut host = host_with_thread();
    host.editor_state_mut().editor_ui.comments.open(1);
    host.editor_state_mut().editor_ui.comments.blur_composer();
    host.apply_text('r');
    // Nothing was typed into the composer.
    assert!(host
        .editor_state()
        .editor_ui
        .comments
        .reply_draft
        .is_empty());
}

#[test]
fn a_control_character_never_reaches_the_draft() {
    let mut host = host_with_thread();
    let state = host.editor_state_mut();
    state.editor_ui.comments.open(1);
    state.editor_ui.comments.focus_composer();
    assert!(host.apply_text('\n'));
    assert!(host
        .editor_state()
        .editor_ui
        .comments
        .reply_draft
        .is_empty());
}

#[test]
fn a_click_on_the_canvas_away_from_the_pill_belongs_to_the_canvas() {
    let mut host = host_with_thread();
    paint(&mut host);
    let rect = host.comments_toggle_rect.expect("the pill painted");
    host.apply_press(rect.origin.x - 60.0, rect.origin.y, W, H);
    assert!(!host.editor_state().editor_ui.comments.panel_open);
}

/// A document with one element the reviewer can comment on.
///
/// The element is a CHILD of a frame, not a bare frame: a click resolves the
/// deepest node on the hit path, and an empty container body is skipped rather
/// than walked into, so a bare frame would give pin mode nothing to attach a
/// comment to (see `LayoutScene::node_path_at_doc_point`).
const ONE_FRAME: &str = r#"{"version":"1.0.0","children":[
  {"type":"frame","id":"frame","name":"Frame","x":400,"y":80,"width":300,"height":300,
   "children":[
     {"type":"rectangle","id":"n1","name":"Card","x":20,"y":20,"width":200,"height":200}
   ]}
]}"#;

/// A host with a document element and one thread about it, so both a pin-mode
/// click and a panel row have something real to work with.
fn host_with_frame_and_thread() -> WidgetHost {
    let doc = jian_ops_schema::load_str(ONE_FRAME)
        .expect("the fixture parses")
        .value;
    let mut host = WidgetHost::new();
    host.editor_state = op_editor_core::EditorState::from_document(doc);
    let state = host.editor_state_mut();
    state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_600_000.0;
    state.editor_ui.file_key = Some("key1".to_string());
    state.tool = op_editor_core::Tool::Select;
    state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, "n1", "Kay")]);
    host.editor_state_dirty = true;
    host.last_viewport_w = W;
    host.last_viewport_h = H;
    host
}

/// The screen point of an element in the fixture, as a person's click.
fn element_point(host: &WidgetHost) -> (f32, f32) {
    let (canvas_x, canvas_y, _, _) = host.canvas_region(W, H);
    (canvas_x + 450.0, canvas_y + 130.0)
}

/// Open the panel the way a reviewer does: press the pill, press the arm row,
/// press the element. Returns the rect the popover painted.
///
/// Every step goes through `apply_press` — the tier ladder and the widget
/// hit-test — because that is the path a person takes, and it is the path the
/// composer's own zones were never exercised through (issue #49).
fn open_composer_by_press(host: &mut WidgetHost) -> op_editor_ui::Rect {
    use op_editor_ui::widgets::comments_panel::CommentsPanel;
    paint(host);
    let pill = host.comments_toggle_rect.expect("the pill painted");
    let (px, py) = centre(pill);
    assert!(host.apply_press(px, py, W, H), "the pill press is consumed");
    assert!(host.editor_state().editor_ui.comments.panel_open);

    paint(host);
    let panel = host.comments_panel_rect.expect("the panel painted");
    let (ax, ay) = centre(CommentsPanel::arm_rect(panel));
    assert!(host.apply_press(ax, ay, W, H), "the arm press is consumed");
    assert!(
        host.editor_state().editor_ui.comments.pin_mode,
        "the arm row is what arms pin mode"
    );

    paint(host);
    let (ex, ey) = element_point(host);
    assert!(
        host.apply_press(ex, ey, W, H),
        "the canvas press is consumed"
    );
    host.apply_release_with_viewport(W, H);
    assert_eq!(
        host.editor_state().editor_ui.comments.composer(),
        Some(op_editor_core::editor_ui_state::CommentComposer::NewThread(
            "n1".to_string()
        )),
        "a click on an element opens the composer for it"
    );

    paint(host);
    host.comments_popover_rect.expect("the composer painted")
}

#[test]
fn a_press_on_the_composer_field_focuses_it_and_typing_appears() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    let rect = open_composer_by_press(&mut host);

    // Setup only: the composer opens focused, so the press below has to be
    // given something to do. The state's own defocus is the precondition — the
    // press is still the thing under test, and it is the only thing that can
    // put focus back.
    host.editor_state_mut().editor_ui.comments.blur_composer();
    assert!(!host.editor_state().editor_ui.comments.composer_focused);

    let field = CommentThreadPopover::input_rect(rect);
    let (fx, fy) = centre(field);
    assert!(
        host.apply_press(fx, fy, W, H),
        "a press on the field is the popover's to consume"
    );
    assert!(
        host.editor_state().editor_ui.comments.composer_focused,
        "the field takes focus on the click that lands on it"
    );

    assert!(host.apply_text('h'));
    assert_eq!(host.editor_state().editor_ui.comments.draft(), "h");
    // The canvas must not have been touched by a letter typed into a field.
    assert_eq!(host.editor_state().tool, op_editor_core::Tool::Select);
}

#[test]
fn a_press_on_send_posts_the_comment_the_field_was_typed_into() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    let rect = open_composer_by_press(&mut host);
    // Opening the panel asked for the conversation; that is done with.
    let _ = host.editor_state_mut().editor_ui.comments.take_requests();

    // Click the field, type, then click Send — the whole gesture, no state call
    // in between.
    let (fx, fy) = centre(CommentThreadPopover::input_rect(rect));
    assert!(host.apply_press(fx, fy, W, H));
    for c in "looks off".chars() {
        assert!(host.apply_text(c));
    }
    assert_eq!(host.editor_state().editor_ui.comments.draft(), "looks off");

    paint(&mut host);
    let rect = host.comments_popover_rect.expect("the composer painted");
    let (sx, sy) = centre(CommentThreadPopover::send_rect(rect));
    assert!(host.apply_press(sx, sy, W, H), "the send press is consumed");
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Create {
            node_id: "n1".to_string(),
            text: "looks off".to_string(),
        }],
        "Send posts the draft it was typed into"
    );
}

#[test]
fn a_press_on_the_field_is_what_lets_a_composed_comment_be_sent() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    // The whole story of issue #49, in one test: the browser delivers anything
    // that is not a plain single-character `keydown` — an IME commit, a dead
    // key, `Input.insertText` — as a text payload to whichever field owns the
    // keyboard. With nobody owning it the payload is dropped and Send has
    // nothing to send, which is precisely what the browser check saw.
    let mut host = host_with_frame_and_thread();
    let rect = open_composer_by_press(&mut host);
    // Opening the panel asked for the conversation; that is done with.
    let _ = host.editor_state_mut().editor_ui.comments.take_requests();
    host.editor_state_mut().editor_ui.comments.blur_composer();

    let composed = "Тут нужен отступ";
    assert!(
        !host.apply_paste_text(composed),
        "an unfocused field is not where a composed character lands"
    );
    assert_eq!(host.editor_state().editor_ui.comments.draft(), "");

    let (fx, fy) = centre(CommentThreadPopover::input_rect(rect));
    assert!(host.apply_press(fx, fy, W, H));
    assert!(
        host.apply_paste_text(composed),
        "the press is what gives the field the keyboard"
    );
    assert_eq!(host.editor_state().editor_ui.comments.draft(), composed);

    paint(&mut host);
    let rect = host.comments_popover_rect.expect("the composer painted");
    let (sx, sy) = centre(CommentThreadPopover::send_rect(rect));
    assert!(host.apply_press(sx, sy, W, H));
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Create {
            node_id: "n1".to_string(),
            text: composed.to_string(),
        }],
        "the composed comment is what travels"
    );
}

#[test]
fn a_press_on_the_reply_field_of_an_open_thread_focuses_it() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    // Open the thread from the panel's row, the other way into a popover.
    host.editor_state_mut().editor_ui.comments.panel_open = true;
    paint(&mut host);
    let panel = host.comments_panel_rect.expect("the panel painted");
    let rows = op_editor_ui::widgets::comments_panel::CommentsPanel::new(
        op_editor_ui::theme::Theme::dark(),
        op_editor_core::editor_ui_state::Locale::EnUs,
        op_editor_ui::widgets::comments_panel::rows(
            &host.editor_state().editor_ui.comments,
            op_editor_core::editor_ui_state::Locale::EnUs,
            None,
            |_| true,
        ),
        false,
        false,
        None,
        0.0,
    )
    .row_rects(panel);
    let (rx, ry) = centre(rows[0]);
    assert!(host.apply_press(rx, ry, W, H), "the row press is consumed");

    host.editor_state_mut().editor_ui.comments.blur_composer();
    paint(&mut host);
    let rect = host.comments_popover_rect.expect("the thread painted");

    let (fx, fy) = centre(CommentThreadPopover::input_rect(rect));
    assert!(host.apply_press(fx, fy, W, H));
    assert!(
        host.editor_state().editor_ui.comments.composer_focused,
        "the reply field is a field too: the same press focuses it"
    );
    assert!(host.apply_text('y'));
    assert_eq!(host.editor_state().editor_ui.comments.reply_draft, "y");
}

#[test]
fn a_press_on_the_resolution_button_resolves_and_then_reopens() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    host.editor_state_mut().editor_ui.comments.open(1);
    paint(&mut host);
    let rect = host.comments_popover_rect.expect("the thread painted");

    let (rx, ry) = centre(CommentThreadPopover::resolution_rect(rect));
    assert!(
        host.apply_press(rx, ry, W, H),
        "the resolve press is consumed"
    );
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Resolve { thread_id: 1 }]
    );

    // The answer makes the thread resolved; the same button now reopens it, so
    // the zone has to stay where the paint put it.
    let mut resolved = thread(1, "n1", "Kay");
    resolved.resolved = true;
    host.editor_state_mut()
        .editor_ui
        .comments
        .install_threads(vec![resolved]);
    host.editor_state_mut().editor_ui.comments.open(1);
    paint(&mut host);
    let rect = host.comments_popover_rect.expect("the thread still paints");
    let (rx, ry) = centre(CommentThreadPopover::resolution_rect(rect));
    assert!(
        host.apply_press(rx, ry, W, H),
        "the reopen press is consumed"
    );
    assert_eq!(
        host.editor_state_mut().editor_ui.comments.take_requests(),
        vec![CommentRequest::Reopen { thread_id: 1 }]
    );
}

#[test]
fn a_press_outside_the_composer_closes_it_and_queues_nothing() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    let rect = open_composer_by_press(&mut host);
    let (fx, fy) = centre(CommentThreadPopover::input_rect(rect));
    assert!(host.apply_press(fx, fy, W, H));
    assert!(host.apply_text('h'));
    let _ = host.editor_state_mut().editor_ui.comments.take_requests();

    // Well clear of the panel, the popover and the pill.
    assert!(host.apply_press(W - 700.0, H - 300.0, W, H));
    assert!(
        host.editor_state().editor_ui.comments.composer().is_none(),
        "a press outside dismisses the composer"
    );
    assert!(
        host.editor_state_mut()
            .editor_ui
            .comments
            .take_requests()
            .is_empty(),
        "a dismissal is not a send: nothing is queued"
    );
    // The draft goes with the popover, so reopening it cannot resurrect a
    // sentence the reviewer no longer remembers typing.
    assert_eq!(host.editor_state().editor_ui.comments.draft(), "");
}

#[test]
fn the_pill_rect_is_the_one_the_widget_layer_places() {
    // Paint and press must agree about where the pill is; the host caches the
    // painted rect, so the values have to come from the same function.
    let mut host = host_with_thread();
    paint(&mut host);
    let canvas =
        op_editor_ui::widgets::host_canvas_geometry::canvas_rect(host.editor_state(), W, H);
    assert_eq!(
        host.comments_toggle_rect,
        Some(CommentsToggle::rect_in_canvas(canvas))
    );
}
