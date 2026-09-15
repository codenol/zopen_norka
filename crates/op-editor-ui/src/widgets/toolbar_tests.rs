//! Toolbar layout, hit-testing and the comment tool's own button.
//!
//! Split from `toolbar.rs` for the repository's per-file ceiling. These tests
//! are about the column's arithmetic and about the one item whose active state
//! and badge come from another subsystem (the comment tool), so they read the
//! same build path the host does.

use super::*;

#[test]
fn default_set_has_base_tools_shape_slot_and_actions_no_widgets() {
    let toolbar = Toolbar::default_set();
    let tool_count = toolbar
        .items
        .iter()
        .filter(|i| matches!(i, ToolbarItem::Tool(..)))
        .count();
    let widget_tool_count = toolbar
        .items
        .iter()
        .filter(|i| matches!(i, ToolbarItem::Tool(t, _) if t.is_widget()))
        .count();
    let action_count = toolbar
        .items
        .iter()
        .filter(|i| matches!(i, ToolbarItem::Action(..)))
        .count();
    let shape_slot_count = toolbar
        .items
        .iter()
        .filter(|i| matches!(i, ToolbarItem::ShapeSlot))
        .count();
    // Select / Text / Frame / Section / Hand are direct tool buttons;
    // Rect / Ellipse / Polygon / Line / Pen live behind the single
    // ShapeSlot dropdown. Form widgets are NOT toolbar tools — they
    // are authored via the component kit / AI+MCP, not primitive
    // drop tools.
    assert_eq!(tool_count, 5);
    assert_eq!(widget_tool_count, 0);
    assert_eq!(shape_slot_count, 1);
    assert_eq!(action_count, 4);
    assert_eq!(toolbar.active, Tool::Select);
}

#[test]
fn intrinsic_height_accommodates_all_items() {
    let toolbar = Toolbar::default_set();
    let h = toolbar.intrinsic_height();
    // 5 direct tools + shape slot + 4 action buttons = 10 button
    // slots; total is at least 10 * BUTTON_SIZE plus padding + gaps.
    let buttons = 9.0;
    assert!(
        h > buttons * BUTTON_SIZE,
        "toolbar shorter than its buttons"
    );
    assert!(h < buttons * BUTTON_SIZE + 200.0, "toolbar bloated: {h}");
}

#[test]
fn for_editor_picks_up_active_tool() {
    let mut state = EditorState::new();
    state.tool = op_editor_core::Tool::Frame;
    let toolbar = Toolbar::for_editor(&state);
    assert_eq!(toolbar.active, Tool::Frame);
}

#[test]
fn hit_test_inside_first_button_returns_select() {
    let toolbar = Toolbar::default_set();
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(TOOLBAR_WIDTH, toolbar.intrinsic_height()),
    };
    // Center of the first button.
    let center = Point2D::new((TOOLBAR_WIDTH) / 2.0, PAD_TOP + BUTTON_SIZE / 2.0);
    assert_eq!(
        toolbar.hit_test(rect, center),
        Some(ToolbarHit::Tool(Tool::Select))
    );
}

#[test]
fn hit_test_outside_returns_none() {
    let toolbar = Toolbar::default_set();
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(TOOLBAR_WIDTH, toolbar.intrinsic_height()),
    };
    assert_eq!(toolbar.hit_test(rect, Point2D::new(-10.0, -10.0)), None);
    assert_eq!(toolbar.hit_test(rect, Point2D::new(1000.0, 1000.0)), None);
}

#[test]
fn for_editor_picks_up_pressed_button() {
    let mut state = EditorState::new();
    state.editor_ui.pressed_button = Some(op_editor_core::ButtonPressTarget::Toolbar(
        op_editor_core::ToolbarHover::Action(op_editor_core::ToolbarAction::Undo),
    ));
    let toolbar = Toolbar::for_editor(&state);
    assert_eq!(
        toolbar.pressed,
        Some(op_editor_core::ToolbarHover::Action(
            op_editor_core::ToolbarAction::Undo
        ))
    );
}

#[test]
fn hit_test_resolves_action_button() {
    let toolbar = Toolbar::default_set();
    let rect = Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(TOOLBAR_WIDTH, toolbar.intrinsic_height()),
    };
    // The Undo button sits below the (now longer) tool sections;
    // rather than re-derive its y by hand, scan every row down the
    // bar centre for the first Undo hit. Decouples the test from
    // the exact item layout.
    let cx = TOOLBAR_WIDTH / 2.0;
    let undo_hit = (0..(toolbar.intrinsic_height() as i32)).find(|y| {
        toolbar.hit_test(rect, Point2D::new(cx, *y as f32))
            == Some(ToolbarHit::Action(ToolbarAction::Undo))
    });
    assert!(
        undo_hit.is_some(),
        "expected an Undo action button somewhere down the bar"
    );
}

#[test]
fn no_widget_tools_in_toolbar() {
    // Widgets are authored via the component kit / AI+MCP, never as
    // toolbar drop-tools — so none appear in the bar.
    let toolbar = Toolbar::default_set();
    assert!(!toolbar
        .items
        .iter()
        .any(|i| matches!(i, ToolbarItem::Tool(t, _) if t.is_widget())));
}

/// A state whose host carries a comment client, with two pages.
///
/// The page pair is what makes the badge's scoping observable: a count that
/// mixed the pages would show the same number here as the document-wide one.
fn comment_ready_state() -> (EditorState, String, String) {
    let mut state = EditorState::new();
    state.editor_ui.comments.transport = true;
    state.add_page().expect("a second page");
    let pages = state.doc.pages.as_ref().expect("pages").clone();
    let first = pages[0].id.clone();
    let second = pages[1].id.clone();
    state.set_active_page(0);
    (state, first, second)
}

fn comment_thread(
    id: i64,
    page: &str,
    resolved: bool,
) -> op_editor_core::editor_ui_state::CommentThread {
    op_editor_core::editor_ui_state::CommentThread {
        id,
        anchor: Some(op_editor_core::editor_ui_state::CommentAnchor::new(
            page,
            id as f64 * 10.0,
            20.0,
        )),
        resolved,
        ..op_editor_core::editor_ui_state::CommentThread::default()
    }
}

fn toolbar_rect_of(toolbar: &Toolbar) -> Rect {
    Rect {
        origin: Point2D::new(0.0, 0.0),
        size: Point2D::new(TOOLBAR_WIDTH, toolbar.intrinsic_height()),
    }
}

#[test]
fn the_comment_tool_is_offered_only_where_a_comment_client_exists() {
    // A host with no client has no list to show and no write to send, so the
    // button that would select that empty mode is not painted at all.
    let plain = Toolbar::for_editor(&EditorState::new());
    assert!(!has_comment_item(&plain));

    let (state, _, _) = comment_ready_state();
    let able = Toolbar::for_editor(&state);
    assert!(has_comment_item(&able));
    // And it sits in the tool group, above the separator that ends it —
    // which is where the other tools are.
    let comment = able
        .items
        .iter()
        .position(|item| matches!(item, ToolbarItem::Action(ToolbarAction::ToggleComments, _)))
        .expect("the comment button is in the column");
    let last_tool = able
        .items
        .iter()
        .rposition(|item| matches!(item, ToolbarItem::Tool(..)))
        .expect("tools exist");
    assert_eq!(comment, last_tool + 1);
}

fn has_comment_item(toolbar: &Toolbar) -> bool {
    toolbar
        .items
        .iter()
        .any(|item| matches!(item, ToolbarItem::Action(ToolbarAction::ToggleComments, _)))
}

#[test]
fn the_comment_button_answers_a_press_with_its_own_action() {
    let (state, _, _) = comment_ready_state();
    let toolbar = Toolbar::for_editor(&state);
    let rect = toolbar_rect_of(&toolbar);
    let cx = TOOLBAR_WIDTH / 2.0;
    let hit = (0..(toolbar.intrinsic_height() as i32)).find_map(|y| {
        match toolbar.hit_test(rect, Point2D::new(cx, y as f32)) {
            Some(ToolbarHit::Action(ToolbarAction::ToggleComments)) => Some(()),
            _ => None,
        }
    });
    assert!(
        hit.is_some(),
        "the comment button has to be reachable down the bar"
    );
}

#[test]
fn the_comment_button_reads_the_tool_it_switches() {
    let (mut state, _, _) = comment_ready_state();
    let off = Toolbar::for_editor(&state);
    assert!(!off.comments_armed, "the tool starts off");
    assert!(!off.action_is_active(ToolbarAction::ToggleComments));

    state.editor_ui.comments.begin_mode();
    let on = Toolbar::for_editor(&state);
    assert!(on.comments_armed);
    assert!(on.action_is_active(ToolbarAction::ToggleComments));
    // The buttons beside it have nothing to be "on" — undo is a command,
    // not a mode — so they never paint as active.
    assert!(!on.action_is_active(ToolbarAction::Undo));
}

#[test]
fn the_badge_counts_the_open_threads_of_the_page_being_edited() {
    let (mut state, first, second) = comment_ready_state();
    state.editor_ui.comments.install_threads(vec![
        comment_thread(1, &first, false),
        comment_thread(2, &first, false),
        // A closed thread is not outstanding work.
        comment_thread(3, &first, true),
        comment_thread(4, &second, false),
    ]);
    let toolbar = Toolbar::for_editor(&state);
    assert_eq!(toolbar.comments_open, 2);

    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    toolbar.paint(&mut cx, toolbar_rect_of(&toolbar));
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert_eq!(painted, vec!["2"], "the badge is the only text on the bar");

    // Switching page re-scopes both answers: the badge and the rail it opens
    // have to agree.
    state.set_active_page(1);
    assert_eq!(Toolbar::for_editor(&state).comments_open, 1);
}

#[test]
fn a_badge_of_nothing_is_not_painted_at_all() {
    let (mut state, first, _) = comment_ready_state();
    state
        .editor_ui
        .comments
        .install_threads(vec![comment_thread(1, &first, true)]);
    let toolbar = Toolbar::for_editor(&state);
    assert_eq!(toolbar.comments_open, 0);
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    toolbar.paint(&mut cx, toolbar_rect_of(&toolbar));
    assert!(
        backend.texts.is_empty(),
        "a zero badge claims there is something to read"
    );
}

#[test]
fn access_node_advertises_toolbar_role() {
    let toolbar = Toolbar::default_set();
    let node = toolbar.access_node();
    assert_eq!(node.role(), accesskit::Role::Toolbar);
    assert_eq!(node.label(), Some("Toolbar"));
}
