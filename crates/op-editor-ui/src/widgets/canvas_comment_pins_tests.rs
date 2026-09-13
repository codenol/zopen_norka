//! The comment markers as the canvas widget exposes them.
//!
//! `op_editor_ui::widgets::comment_pins` is tested on its own geometry; this
//! file holds the seam a host actually calls — `CanvasViewport::comment_pins`
//! and `CanvasViewport::hit_test_comment_pin` — because a marker a host can
//! paint but cannot hit is a marker that does nothing.

mod canvas_pins_tests {
    use crate::layout_scene::{LayoutScene, NodeKind, SceneNode, ScenePage};
    use crate::widgets::canvas_viewport::CanvasViewport;
    use crate::{Point2D, Rect};
    use op_editor_core::editor_ui_state::{Comment, CommentAuthor, CommentThread};
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

    fn thread(id: i64, node: &str) -> CommentThread {
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
                    id: Some("u1".to_string()),
                    name: "Kay".to_string(),
                    role: Some("ux_ui".to_string()),
                },
                body: "look at this".to_string(),
                created_at: 1_700_000_000,
            }],
        }
    }

    fn state_with(threads: Vec<CommentThread>) -> EditorState {
        let mut state = EditorState::new();
        state.editor_ui.comments.install_threads(threads);
        state
    }

    #[test]
    fn a_thread_on_a_present_element_becomes_a_marker_a_click_can_find() {
        let state = state_with(vec![thread(1, "n1")]);
        let scene = scene_with(&["n1"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 1);
        // The element's top-right corner, mapped into the canvas region.
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
    fn a_thread_whose_element_is_gone_has_no_marker_to_hit() {
        let state = state_with(vec![thread(1, "n1"), thread(2, "deleted-elsewhere")]);
        let scene = scene_with(&["n1"]);
        let viewport = CanvasViewport::from_editor(&state, &scene);
        // Only the thread with an element gets a marker; the other one is still
        // in the state, which is what the list panel reads.
        let pins = viewport.comment_pins(CANVAS);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].thread_id, 1);
        assert_eq!(state.editor_ui.comments.threads.len(), 2);
    }

    #[test]
    fn a_press_between_two_markers_hits_neither() {
        let state = state_with(vec![thread(1, "n1"), thread(2, "n2")]);
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
    fn zoom_and_pan_move_the_marker_with_its_element() {
        let state = state_with(vec![thread(1, "n1")]);
        let scene = scene_with(&["n1"]);
        let plain = CanvasViewport::from_editor(&state, &scene).comment_pins(CANVAS);

        let mut panned = EditorState::new();
        panned
            .editor_ui
            .comments
            .install_threads(vec![thread(1, "n1")]);
        panned.viewport.pan_x = 40.0;
        panned.viewport.pan_y = 25.0;
        let moved = CanvasViewport::from_editor(&panned, &scene).comment_pins(CANVAS);

        assert_eq!(moved[0].rect.origin.x, plain[0].rect.origin.x + 40.0);
        assert_eq!(moved[0].rect.origin.y, plain[0].rect.origin.y + 25.0);

        let mut zoomed = EditorState::new();
        zoomed
            .editor_ui
            .comments
            .install_threads(vec![thread(1, "n1")]);
        zoomed.viewport.zoom = 2.0;
        let scaled = CanvasViewport::from_editor(&zoomed, &scene).comment_pins(CANVAS);
        assert!(scaled[0].rect.origin.x > plain[0].rect.origin.x);
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
        let state = state_with(vec![thread(1, "n1")]);
        let empty = LayoutScene::default();
        let viewport = CanvasViewport::from_editor(&state, &empty);
        assert!(viewport.comment_pins(CANVAS).is_empty());
    }
}
