//! Press coverage for the web host's comment surfaces.
//!
//! Drives the real composition pass (`paint_editor`) and then the real press
//! ladder, because the two halves are what can drift: the rail and the popover
//! are placed against the rail's rect and the conversation's length, so a press
//! must hit-test the rects the paint actually used. Asserting on the shared flow
//! alone would not catch the host caching them.
//!
//! The composer tests are the ones issue #53 is about: a click in the middle of
//! the page must open the field *there*, and the pin must land where the field
//! stood — not in a corner of the canvas.

use super::WidgetHost;
use op_editor_core::editor_ui_state::{
    Comment, CommentAnchor, CommentAuthor, CommentRequest, CommentThread,
};
use op_editor_core::NodeId;
use op_editor_ui::widgets::comment_pins::{pin_rect, CommentPinThread};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
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

/// A document with one element the reviewer can comment on top of.
///
/// The element matters only as something to look at: a comment is placed at the
/// point that was clicked whatever is under it, which is the whole difference
/// from the element-keyed model this replaced.
const ONE_FRAME: &str = r#"{"version":"1.0.0","children":[
  {"type":"frame","id":"frame","name":"Frame","x":400,"y":80,"width":300,"height":300,
   "children":[
     {"type":"rectangle","id":"n1","name":"Card","x":20,"y":20,"width":200,"height":200}
   ]}
]}"#;

fn thread(id: i64, page: &str, x: f64, y: f64, name: &str) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, x, y)),
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

/// The page this host's document names, as the app itself resolves it.
fn page_of(host: &WidgetHost) -> String {
    host.editor_state().active_page_identity().0
}

fn host_with(threads: Vec<CommentThread>) -> WidgetHost {
    let doc = jian_ops_schema::load_str(ONE_FRAME)
        .expect("the fixture parses")
        .value;
    let mut host = WidgetHost::new();
    // Through the host's own seam, not by assigning the field: that is what
    // carries this host's capabilities (the comment client) onto a fresh state,
    // and a test that bypassed it would be testing a build where the comment
    // button does not exist.
    host.replace_editor_state(op_editor_core::EditorState::from_document(doc));
    let state = host.editor_state_mut();
    state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
    state.editor_ui.now_unix_ms = 1_700_000_600_000.0;
    state.editor_ui.file_key = Some("key1".to_string());
    state.tool = op_editor_core::Tool::Select;
    state.editor_ui.comments.install_threads(threads);
    host.editor_state_dirty = true;
    host.last_viewport_w = W;
    host.last_viewport_h = H;
    host
}

fn host_with_thread() -> WidgetHost {
    let mut host = host_with(vec![]);
    let page = page_of(&host);
    host.editor_state_mut()
        .editor_ui
        .comments
        .install_threads(vec![thread(1, &page, 900.0, 620.0, "Kay")]);
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

/// The screen point a person clicks: inside the fixture's card, on the canvas.
fn click_point(host: &WidgetHost) -> (f32, f32) {
    let (canvas_x, canvas_y, _, _) = host.canvas_region(W, H);
    (canvas_x + 450.0, canvas_y + 130.0)
}

/// The toolbar's comment button, found the way a press finds it.
///
/// Scanned rather than derived: the column's arithmetic is the widget's, and a
/// test that re-derived it would move with every unrelated toolbar change.
fn comment_button(host: &WidgetHost) -> (f32, f32) {
    let rect = canvas_geometry::toolbar_rect_for(host.editor_state());
    let toolbar = op_editor_ui::widgets::Toolbar::for_editor(host.editor_state());
    let x = rect.origin.x + rect.size.x / 2.0;
    for y in (rect.origin.y as i32)..((rect.origin.y + rect.size.y) as i32) {
        if toolbar.hit_test(rect, Point2D::new(x, y as f32))
            == Some(op_editor_ui::widgets::ToolbarHit::Action(
                op_editor_ui::widgets::ToolbarAction::ToggleComments,
            ))
        {
            return (x, y as f32);
        }
    }
    panic!("the comment button is in the toolbar");
}

/// Turn the comment tool on the way a reviewer does: press the toolbar button.
fn arm_by_press(host: &mut WidgetHost) {
    paint(host);
    let (x, y) = comment_button(host);
    assert!(host.apply_press(x, y, W, H), "the button press is consumed");
    assert!(
        host.editor_state().editor_ui.comments.pin_mode,
        "the toolbar button is what selects the comment tool"
    );
}

/// Open a composer the way a reviewer does: select the tool, then click the
/// page. Returns the rect the popover painted.
///
/// Every step goes through `apply_press` — the tier ladder and the widget
/// hit-test — because that is the path a person takes, and it is the path the
/// composer's own zones were never exercised through (issue #49).
fn open_composer_by_press(host: &mut WidgetHost) -> Rect {
    arm_by_press(host);
    paint(host);
    let (ex, ey) = click_point(host);
    assert!(
        host.apply_press(ex, ey, W, H),
        "the canvas press is consumed"
    );
    host.apply_release_with_viewport(W, H);
    assert!(
        host.editor_state().editor_ui.comments.composer().is_some(),
        "a click on the canvas opens the composer for that point"
    );

    paint(host);
    host.comments_popover_rect.expect("the composer painted")
}

#[test]
fn the_tool_selects_the_rail_and_a_second_press_leaves_it() {
    let mut host = host_with_thread();
    paint(&mut host);
    assert!(
        host.comments_panel_rect.is_none(),
        "the inspector owns the rail until the comment tool asks for it"
    );

    arm_by_press(&mut host);
    paint(&mut host);
    let rail = host.comments_panel_rect.expect("the comment rail painted");
    // The rail's own slot: the same rect the inspector paints into, so the
    // canvas keeps the width it had and the list is not a box over the design.
    assert_eq!(
        rail,
        canvas_geometry::property_panel_rect(host.editor_state(), W, H)
    );
    // Selecting the tool asks for the conversation, because the daemon pushes no
    // signal for comments.
    let requests = host.editor_state_mut().editor_ui.comments.take_requests();
    assert!(requests.contains(&CommentRequest::Reload));

    // Pressing it again is how the tool is left, exactly like the other tools.
    let (x, y) = comment_button(&host);
    assert!(host.apply_press(x, y, W, H));
    assert!(!host.editor_state().editor_ui.comments.pin_mode);
    paint(&mut host);
    assert!(host.comments_panel_rect.is_none(), "the rail went back");
}

#[test]
fn another_tool_leaves_the_comment_tool() {
    let mut host = host_with_thread();
    arm_by_press(&mut host);
    // The single-key router and the toolbar both funnel through
    // `set_active_tool`, which is where "the column has one active entry" is
    // enforced.
    assert!(host.apply_tool_shortcut("f"));
    assert_eq!(host.editor_state().tool, op_editor_core::Tool::Frame);
    assert!(
        !host.editor_state().editor_ui.comments.pin_mode,
        "a canvas that kept dropping pins after the frame tool was picked would be a mode nobody asked for"
    );
    paint(&mut host);
    assert!(host.comments_panel_rect.is_none());
}

#[test]
fn the_host_paints_the_same_rail_the_widget_places() {
    let mut host = host_with_thread();
    arm_by_press(&mut host);
    paint(&mut host);
    let rail = host.comments_panel_rect.expect("the rail painted");
    let panel = op_editor_ui::widgets::comments_flow::panel_for(
        host.editor_state(),
        &host.active_page_id(),
    )
    .expect("the rail is showing the conversation");
    assert!(!panel.row_rects(rail).is_empty(), "with a thread to list");
}

#[test]
fn a_row_press_opens_the_thread_and_frames_its_pin() {
    let mut host = host_with_thread();
    host.editor_state_mut().viewport.pan_x = -4_000.0;
    arm_by_press(&mut host);
    paint(&mut host);
    let rail = host.comments_panel_rect.expect("the rail painted");
    let panel = op_editor_ui::widgets::comments_flow::panel_for(
        host.editor_state(),
        &host.active_page_id(),
    )
    .expect("the rail is showing the conversation");
    let row = panel.row_rects(rail)[0];
    let (x, y) = centre(row);
    assert!(host.apply_press(x, y, W, H), "the row press is consumed");
    assert!(host.editor_state().editor_ui.comments.is_open(1));
    // The pin was panned far off screen; the row brought it back to the middle
    // of the canvas.
    let (canvas_x, canvas_y, canvas_w, canvas_h) = host.canvas_region(W, H);
    let zoom = host.editor_state().viewport.zoom;
    let pin_x = canvas_x + host.editor_state().viewport.pan_x + 900.0 * zoom;
    let pin_y = canvas_y + host.editor_state().viewport.pan_y + 620.0 * zoom;
    assert!(
        (pin_x - (canvas_x + canvas_w / 2.0)).abs() < 1.0,
        "centred on x"
    );
    assert!(
        (pin_y - (canvas_y + canvas_h / 2.0)).abs() < 1.0,
        "centred on y"
    );
}

#[test]
fn a_thread_with_no_pin_opens_from_the_rail_without_moving_the_canvas() {
    // The daemon migrated threads from the element-keyed format have no page and
    // no coordinates. They are listed — a conversation nobody can find is worse
    // than one without a marker — and pressing one opens it without a jump to a
    // place nobody named.
    let mut host = host_with(vec![CommentThread {
        id: 5,
        anchor: None,
        comments: vec![Comment::default()],
        ..CommentThread::default()
    }]);
    arm_by_press(&mut host);
    paint(&mut host);
    let rail = host.comments_panel_rect.expect("the rail painted");
    let panel = op_editor_ui::widgets::comments_flow::panel_for(
        host.editor_state(),
        &host.active_page_id(),
    )
    .expect("the rail lists it");
    assert_eq!(panel.row_rects(rail).len(), 1, "the thread is listed");

    let before = host.editor_state().viewport.clone();
    let (x, y) = centre(panel.row_rects(rail)[0]);
    assert!(host.apply_press(x, y, W, H));
    assert!(host.editor_state().editor_ui.comments.is_open(5));
    assert_eq!(
        host.editor_state().viewport.pan_x,
        before.pan_x,
        "nowhere to jump to"
    );
    assert_eq!(host.editor_state().viewport.pan_y, before.pan_y);
}

#[test]
fn a_composer_opens_at_the_point_that_was_clicked() {
    let mut host = host_with_frame_and_thread();
    let (click_x, click_y) = click_point(&host);
    let rect = open_composer_by_press(&mut host);

    // The anchor the click recorded is the click's document point.
    let anchor = match host.editor_state().editor_ui.comments.composer() {
        Some(op_editor_core::editor_ui_state::CommentComposer::NewThread(anchor)) => anchor,
        other => panic!("expected a new-thread composer, got {other:?}"),
    };
    assert_eq!(anchor.page_id, page_of(&host));
    let canvas = canvas_geometry::canvas_rect(host.editor_state(), W, H);
    let doc_point =
        canvas_geometry::canvas_doc_point_unclamped(host.editor_state(), click_x, click_y);
    assert_eq!(anchor.x, doc_point.x as f64);
    assert_eq!(anchor.y, doc_point.y as f64);

    // And the field hangs off the marker that click will become — the same rect
    // the pin is placed at, so the marker lands under the field that wrote it.
    let expected_pin = pin_rect(Point2D::new(click_x, click_y));
    let popover = op_editor_ui::widgets::comments_flow::popover_for(host.editor_state())
        .expect("a composer is open");
    assert_eq!(rect, popover.rect_at(expected_pin, canvas));
    assert_ne!(
        rect.origin,
        Point2D::new(canvas.origin.x + 8.0, canvas.origin.y + 8.0),
        "the canvas corner is the bug this replaced"
    );
    // The pin is placed from the same numbers, so the two agree by construction.
    let placed = op_editor_ui::widgets::comment_pins::place_pins(
        &[CommentPinThread {
            thread_id: 12,
            ordinal: 1,
            anchor: anchor.clone(),
            color: None,
            resolved: false,
            replies: 0,
        }],
        canvas,
        &host.editor_state().viewport,
    );
    assert_eq!(placed[0].rect, expected_pin);
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
    // `input_active()` is what keeps a typed letter out of the tool router, and
    // the comment field is part of that rule (issue #49).
    assert!(host.input_active());
    // The canvas must not have been touched by a letter typed into a field.
    assert_eq!(host.editor_state().tool, op_editor_core::Tool::Select);
}

#[test]
fn a_press_on_send_posts_the_comment_at_the_point_it_was_written_at() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    let rect = open_composer_by_press(&mut host);
    let anchor = match host.editor_state().editor_ui.comments.composer() {
        Some(op_editor_core::editor_ui_state::CommentComposer::NewThread(anchor)) => anchor,
        other => panic!("expected a new-thread composer, got {other:?}"),
    };
    // Selecting the tool asked for the conversation; that is done with.
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
    let requests = host.editor_state_mut().editor_ui.comments.take_requests();
    assert_eq!(
        requests.first(),
        Some(&CommentRequest::Create {
            anchor,
            text: "looks off".to_string(),
        }),
        "Send posts the draft at the point the composer stood"
    );
    // The tool stays selected: a review is several comments in a row.
    assert!(host.editor_state().editor_ui.comments.pin_mode);
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
    let requests = host.editor_state_mut().editor_ui.comments.take_requests();
    assert!(
        matches!(
            requests.first(),
            Some(CommentRequest::Create { text, .. }) if text == composed
        ),
        "the composed comment is what travels: {requests:?}"
    );
}

#[test]
fn a_press_on_the_reply_field_of_an_open_thread_focuses_it() {
    use op_editor_ui::widgets::comment_thread_popover::CommentThreadPopover;
    let mut host = host_with_frame_and_thread();
    // Open the thread from the rail's row, the other way into a popover.
    arm_by_press(&mut host);
    paint(&mut host);
    let rail = host.comments_panel_rect.expect("the rail painted");
    let panel = op_editor_ui::widgets::comments_flow::panel_for(
        host.editor_state(),
        &host.active_page_id(),
    )
    .expect("the rail lists the thread");
    let (rx, ry) = centre(panel.row_rects(rail)[0]);
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
    let mut host = host_with_thread();
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
    let mut resolved = thread(1, &page_of(&host), 900.0, 620.0, "Kay");
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

    // Well clear of the rail, the popover and the toolbar.
    let (canvas_x, canvas_y, canvas_w, canvas_h) = host.canvas_region(W, H);
    assert!(host.apply_press(
        canvas_x + canvas_w - 40.0,
        canvas_y + canvas_h - 300.0,
        W,
        H
    ));
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
fn a_canvas_click_with_the_tool_off_selects_instead_of_pinning() {
    let mut host = host_with_frame_and_thread();
    paint(&mut host);
    let (ex, ey) = click_point(&host);
    host.apply_press(ex, ey, W, H);
    host.apply_release_with_viewport(W, H);
    assert!(
        host.editor_state().editor_ui.comments.composer().is_none(),
        "no tool, no pin"
    );
    assert!(
        !host.editor_state().selection.is_empty(),
        "the click selected the element under it, as a click outside the comment tool does"
    );
}

#[test]
fn escape_dismisses_the_popover_then_the_tool_then_the_selection() {
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

/// A host with the fixture document, so a click has something to select.
fn host_with_frame_and_thread() -> WidgetHost {
    let mut host = host_with(vec![]);
    let page = page_of(&host);
    host.editor_state_mut()
        .editor_ui
        .comments
        .install_threads(vec![thread(1, &page, 900.0, 620.0, "Kay")]);
    host
}
