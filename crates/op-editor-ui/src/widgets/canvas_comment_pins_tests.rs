//! The comment markers as the canvas widget exposes them.
//!
//! `op_editor_ui::widgets::comment_pins` is tested on its own geometry; this
//! file holds the seam a host actually calls — `CanvasViewport::comment_pins`
//! and `CanvasViewport::hit_test_comment_pin` — because a marker a host can
//! paint but cannot hit is a marker that does nothing. It is also where the
//! page filter is exercised: the canvas shows one page, and only that page's
//! threads may be placed on it.

mod canvas_pins_tests {
    use crate::layout_scene::{LayoutScene, NodeKind, SceneNode, ScenePage};
    use crate::widgets::canvas_viewport::CanvasViewport;
    use crate::{Point2D, Rect};
    use op_editor_core::editor_ui_state::{Comment, CommentAnchor, CommentAuthor, CommentThread};
    use op_editor_core::EditorState;

    const CANVAS: Rect = Rect {
        origin: Point2D { x: 240.0, y: 40.0 },
        size: Point2D { x: 800.0, y: 600.0 },
    };

    fn scene_with(ids: &[&str]) -> LayoutScene {
        let children = ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let mut node = SceneNode::leaf(*id, NodeKind::Rect);
                node.bounds = Rect::xywh(100.0 + index as f32 * 200.0, 100.0, 120.0, 60.0);
                node
            })
            .collect();
        LayoutScene {
            pages: vec![ScenePage {
                id: "p1".to_string(),
                name: "Page 1".to_string(),
                children,
            }],
            active_page_index: 0,
        }
    }

    /// A thread at a document point on `page`.
    ///
    /// The document tree is not consulted at all: a comment is about a point,
    /// which is why these tests do not need the element they used to name.
    fn thread(id: i64, page: &str, x: f64, y: f64) -> CommentThread {
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
                    id: Some("u1".to_string()),
                    name: "Kay".to_string(),
                    role: Some("ux_ui".to_string()),
                },
                body: "look at this".to_string(),
                created_at: 1_700_000_000,
            }],
        }
    }

    /// The id the state gives its own page.
    ///
    /// Read, never spelled: which page a host is on is one rule
    /// (`EditorState::active_page_identity`), and a test that hard-coded its own
    /// would pass while the canvas looked for a page nobody had named.
    fn page_of(state: &EditorState) -> String {
        state.active_page_identity().0
    }

    /// A state with the comment tool active — the mode the canvas shows markers
    /// in — and the given threads loaded.
    fn state_with(threads: Vec<CommentThread>) -> EditorState {
        let mut state = EditorState::new();
        state.editor_ui.comments.transport = true;
        state.editor_ui.comments.begin_mode();
        state.editor_ui.comments.install_threads(threads);
        state
    }

    #[test]
    fn a_thread_becomes_a_marker_at_its_document_point() {
        let mut state = state_with(vec![]);
        let page = page_of(&state);
        state
            .editor_ui
            .comments
            .install_threads(vec![thread(1, &page, 220.0, 100.0)]);
        let scene = scene_with(&["n1"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 1);
        // The document point mapped into the canvas region, at zoom 1 / pan 0.
        assert_eq!(
            pins[0].rect.origin,
            Point2D::new(240.0 + 220.0 - 11.0, 40.0 + 100.0 - 11.0)
        );
        let centre = Point2D::new(
            pins[0].rect.origin.x + pins[0].rect.size.x / 2.0,
            pins[0].rect.origin.y + pins[0].rect.size.y / 2.0,
        );
        assert_eq!(
            viewport.hit_test_comment_pin(CANVAS, centre, false),
            Some(1)
        );
    }

    #[test]
    fn a_thread_on_another_page_has_no_marker_on_this_one() {
        let mut state = state_with(vec![]);
        let page = page_of(&state);
        state.editor_ui.comments.install_threads(vec![
            thread(1, &page, 220.0, 100.0),
            thread(2, "another-page", 220.0, 100.0),
        ]);
        let scene = scene_with(&["n1"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].thread_id, 1);
        // The other page's thread is still in the state, which is what the rail
        // counts.
        assert_eq!(state.editor_ui.comments.threads.len(), 2);
    }

    #[test]
    fn a_thread_whose_element_was_deleted_keeps_its_marker() {
        // The case the element-keyed model could not express: the pin is on a
        // point, so deleting what was under it moves nothing.
        let mut state = state_with(vec![]);
        let page = page_of(&state);
        state
            .editor_ui
            .comments
            .install_threads(vec![thread(1, &page, 500.0, 300.0)]);
        let scene = scene_with(&[]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].thread_id, 1);
    }

    #[test]
    fn a_press_between_two_markers_hits_neither() {
        let mut state = state_with(vec![]);
        let page = page_of(&state);
        state.editor_ui.comments.install_threads(vec![
            thread(1, &page, 120.0, 100.0),
            thread(2, &page, 520.0, 100.0),
        ]);
        let scene = scene_with(&["n1", "n2"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 2);
        let between = Point2D::new(
            (pins[0].rect.origin.x + pins[1].rect.origin.x) / 2.0,
            pins[0].rect.origin.y + 4.0,
        );
        assert_eq!(viewport.hit_test_comment_pin(CANVAS, between, false), None);
    }

    #[test]
    fn zoom_and_pan_move_the_marker_with_the_page() {
        let scene = scene_with(&["n1"]);
        let mut seed = state_with(vec![]);
        let page = page_of(&seed);
        let plain =
            CanvasViewport::from_editor(&state_with(vec![thread(1, &page, 300.0, 200.0)]), &scene)
                .comment_pins(CANVAS);

        let mut panned = state_with(vec![thread(1, &page, 300.0, 200.0)]);
        panned.viewport.pan_x = 40.0;
        panned.viewport.pan_y = 25.0;
        let moved = CanvasViewport::from_editor(&panned, &scene).comment_pins(CANVAS);

        assert_eq!(moved[0].rect.origin.x, plain[0].rect.origin.x + 40.0);
        assert_eq!(moved[0].rect.origin.y, plain[0].rect.origin.y + 25.0);

        let mut zoomed = state_with(vec![thread(1, &page, 300.0, 200.0)]);
        zoomed.viewport.zoom = 2.0;
        let scaled = CanvasViewport::from_editor(&zoomed, &scene).comment_pins(CANVAS);
        assert!(scaled[0].rect.origin.x > plain[0].rect.origin.x);
        // Whatever the camera does, the anchor the row press frames on is the
        // document point it was written at.
        assert_eq!(scaled[0].anchor.x, 300.0);
        assert_eq!(scaled[0].anchor.page_id, page);
    }

    #[test]
    fn the_markers_belong_to_the_comment_tool() {
        // Outside the mode the canvas is the design: no pins, and nothing for a
        // press to open. The list and the count are what tell a reviewer there
        // is a conversation to look at.
        let mut state = state_with(vec![]);
        let page = page_of(&state);
        state
            .editor_ui
            .comments
            .install_threads(vec![thread(1, &page, 220.0, 100.0)]);
        let scene = scene_with(&["n1"]);
        assert_eq!(
            CanvasViewport::from_editor(&state, &scene)
                .comment_pins(CANVAS)
                .len(),
            1,
            "the tool is active, so the page's markers are on the canvas"
        );
        state.editor_ui.comments.end_mode();
        let viewport = CanvasViewport::from_editor(&state, &scene);
        assert!(viewport.comment_pins(CANVAS).is_empty());
        assert_eq!(
            viewport.hit_test_comment_pin(CANVAS, Point2D::new(460.0, 150.0), false),
            None
        );
    }

    #[test]
    fn a_canvas_with_no_threads_paints_no_markers() {
        let state = EditorState::new();
        let scene = scene_with(&["n1"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        assert!(viewport.comment_pins(CANVAS).is_empty());
        assert_eq!(
            viewport.hit_test_comment_pin(CANVAS, Point2D::new(460.0, 150.0), false),
            None
        );
    }

    #[test]
    fn a_scene_with_no_page_has_nowhere_to_put_a_marker() {
        let seed = state_with(vec![]);
        let page = page_of(&seed);
        let state = state_with(vec![thread(1, &page, 100.0, 100.0)]);
        let empty = LayoutScene::default();
        let viewport = CanvasViewport::from_editor(&state, &empty);
        assert!(viewport.comment_pins(CANVAS).is_empty());
    }
}
