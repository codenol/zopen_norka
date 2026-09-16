//! Fixtures, paint recorder and numeric assertion helpers behind
//! `layer_panel_tests`.
//!
//! The test cases stay in the parent module, so their names do not move; only
//! the state builders, the recording `RenderBackend` and the small helpers
//! live here.

use crate::widgets::layer_panel::LayerPanel;
use crate::{Point2D, Rect};
use op_editor_core::EditorState;

pub(super) const SECTION_HEADER_HEIGHT: f32 = 28.0;
pub(super) const PAGE_ROW_HEIGHT: f32 = 32.0;
pub(super) const LAYER_ROW_HEIGHT: f32 = 28.0;
// `SECTION_GAP` used to sit here. No test in `layer_panel_tests` ever read it,
// and a second copy of the gap is exactly the kind of number that drifts away
// from the one the panel uses: a test that needs it should read
// `LayerPanelMetrics::section_gap`, which is what the paint path reads.

/// Build an `EditorState` from a canonical `.op` JSON string.
pub(super) fn state_from(src: &str) -> EditorState {
    let doc = jian_ops_schema::load_str(src)
        .expect("layer-panel fixture parses")
        .value;
    EditorState::from_document(doc)
}

/// Four sibling rectangles `n1..n4` in a single-page document.
pub(super) fn four_rects() -> EditorState {
    state_from(
        r##"{ "version": "1.0.0", "children": [
              {"type":"rectangle","id":"n1","name":"A","width":10,"height":10},
              {"type":"rectangle","id":"n2","name":"B","width":10,"height":10},
              {"type":"rectangle","id":"n3","name":"C","width":10,"height":10},
              {"type":"rectangle","id":"n4","name":"D","width":10,"height":10}
        ]}"##,
    )
}

pub(super) fn first_layer_trailing_points(panel: &LayerPanel, rect: Rect) -> (Point2D, Point2D) {
    let y = panel.regions(rect).layers_rows_top;
    let row = Rect {
        origin: Point2D::new(rect.origin.x + 6.0, y + 2.0),
        size: Point2D::new(rect.size.x - 12.0, LAYER_ROW_HEIGHT - 4.0),
    };
    let trailing_right = row.origin.x + row.size.x - 8.0;
    let lock_x = trailing_right - 14.0;
    let eye_x = lock_x - 22.0;
    let icon_y = row.origin.y + 6.0;
    (
        Point2D::new(eye_x + 6.0, icon_y + 6.0),
        Point2D::new(lock_x + 6.0, icon_y + 6.0),
    )
}

pub(super) fn first_layer_eye_top_left(panel: &LayerPanel, rect: Rect) -> Point2D {
    let (eye_center, _) = first_layer_trailing_points(panel, rect);
    Point2D::new(eye_center.x - 6.0, eye_center.y - 5.0)
}

pub(super) fn first_layer_lock_top_left(panel: &LayerPanel, rect: Rect) -> Point2D {
    let (_, lock_center) = first_layer_trailing_points(panel, rect);
    Point2D::new(lock_center.x - 6.0, lock_center.y - 5.0)
}

#[derive(Default)]
pub(super) struct LayerPaintBackend {
    pub(super) strokes: Vec<(Point2D, f32, crate::Color)>,
}

impl crate::RenderBackend for LayerPaintBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: crate::Color) {}
    fn stroke_rect(&mut self, _: Rect, _: crate::Color, _: f32) {}
    fn draw_text(&mut self, _: &crate::TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: crate::Color, _: f32) {}
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: crate::Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: crate::Color, _: f32) {}
    fn stroke_svg_path(
        &mut self,
        _: &str,
        top_left: Point2D,
        size: f32,
        color: crate::Color,
        _: f32,
    ) {
        self.strokes.push((top_left, size, color));
    }
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

pub(super) fn approx_point(a: Point2D, b: Point2D) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

pub(super) fn is_yellow_400(color: crate::Color) -> bool {
    (color.r - 250.0 / 255.0).abs() < 1e-4
        && (color.g - 204.0 / 255.0).abs() < 1e-4
        && (color.b - 21.0 / 255.0).abs() < 1e-4
        && (color.a - 1.0).abs() < 1e-4
}

pub(super) fn nested_frame_doc(depth: usize) -> String {
    let mut src = String::from(r#"{"version":"1.0.0","children":["#);
    for i in 0..depth {
        src.push_str(&format!(
            r##"{{"type":"frame","id":"nest-{i:05}","name":"Nested Layer {i:05}","x":8,"y":6,"width":400,"height":220,"fill":[{{"type":"solid","color":"#ffffff20"}}],"stroke":{{"thickness":1,"fill":[{{"type":"solid","color":"#0088ff"}}]}},"children":["##
        ));
    }
    for _ in 0..depth {
        src.push_str("]}");
    }
    src.push_str("]}");
    src
}

pub(super) fn run_deep_layer_fixture(test: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .name("op-layer-panel-deep-fixture".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(test)
        .expect("spawn deep layer fixture test");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}
