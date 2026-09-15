//! One place that answers "which pixel is that rail row".
//!
//! Three test files had grown their own answer, and each rebuilt the rail rect
//! from `TOP_BAR_HEIGHT` — which stops being the rail the moment anything is
//! inserted above the rows. Two sections did exactly that (the
//! Layers/Slides/Assets tab row, and the Recipes section), and fourteen tests
//! spent a while pressing the recipes band while asserting about layers
//! (issue #138). They ask the host now.
//!
//! Test-only: the shipping host has no use for a pixel scan.

#![cfg(test)]

use super::WidgetHostNative;
use op_editor_core::NodeId;
use op_editor_ui::widgets::LayerPanelHit;
use op_editor_ui::Point2D;

/// A point inside the row `hit` names, found by scanning the rail the HOST
/// uses — never a rect the test derived for itself.
///
/// `None` when that row is not on screen: a cramped rail hides its palettes on
/// purpose (issue #66), and a test that assumes otherwise should be told so
/// rather than scanning a region that is not there.
pub(super) fn rail_hit_point_for_test(
    host: &WidgetHostNative,
    hit: LayerPanelHit,
    viewport_w: f32,
    viewport_h: f32,
) -> Option<Point2D> {
    let panel = host.layer_panel();
    let rect = host.layers_content_rect(viewport_w, viewport_h);
    let mut y = rect.origin.y;
    while y < rect.origin.y + rect.size.y {
        let point = Point2D::new(rect.origin.x + 48.0, y);
        if panel.hit_test(rect, point) == Some(hit.clone()) {
            return Some(point);
        }
        y += 1.0;
    }
    None
}

/// The layer row that a press selects `node`, or a failure naming the node.
pub(super) fn layer_row_point_for_test(
    host: &WidgetHostNative,
    node: &NodeId,
    viewport_w: f32,
    viewport_h: f32,
) -> Point2D {
    rail_hit_point_for_test(
        host,
        LayerPanelHit::Layer(node.clone()),
        viewport_w,
        viewport_h,
    )
    .unwrap_or_else(|| panic!("no visible layer row for {node:?}"))
}

/// The page row at `index`, or a failure naming the index.
pub(super) fn page_row_point_for_test(
    host: &WidgetHostNative,
    index: usize,
    viewport_w: f32,
    viewport_h: f32,
) -> Point2D {
    rail_hit_point_for_test(host, LayerPanelHit::Page(index), viewport_w, viewport_h)
        .unwrap_or_else(|| panic!("no visible page row for index {index}"))
}
