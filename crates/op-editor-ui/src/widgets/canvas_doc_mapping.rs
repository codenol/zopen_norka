//! Document-space → canvas-screen mapping for canvas overlays.
//!
//! Three overlays paint above the node tree — remote presence, frame labels and
//! comment pins — and each needs the same handful of arithmetic: a document
//! point or rect placed inside the canvas region under the current pan and zoom.
//! Remote presence and the pins both place *rects* anchored to an element's
//! bounds, and two copies of that formula is how two overlays end up an inch
//! apart at zoom 4. So it lives here once.
//!
//! Note the origin: the canvas region's own origin, not the widget rect's. The
//! host paints the canvas through a pan cache whose offscreen layer is grown by
//! a margin, and `CanvasViewport::offset_paint_origin` cancels that margin by
//! shifting the pan — mapping against the canvas region keeps both paths equal.

use op_editor_core::Viewport;

use crate::{Point2D, Rect};

/// An element's document-space bounds as a screen-space rect.
pub(crate) fn doc_rect_to_screen(rect: Rect, canvas_rect: Rect, viewport: &Viewport) -> Rect {
    Rect::xywh(
        canvas_rect.origin.x + viewport.pan_x + rect.origin.x * viewport.zoom,
        canvas_rect.origin.y + viewport.pan_y + rect.origin.y * viewport.zoom,
        rect.size.x * viewport.zoom,
        rect.size.y * viewport.zoom,
    )
}

/// A document-space point as a screen-space point.
pub(crate) fn doc_point_to_screen(
    point: Point2D,
    canvas_rect: Rect,
    viewport: &Viewport,
) -> Point2D {
    Point2D::new(
        canvas_rect.origin.x + viewport.pan_x + point.x * viewport.zoom,
        canvas_rect.origin.y + viewport.pan_y + point.y * viewport.zoom,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(pan_x: f32, pan_y: f32, zoom: f32) -> Viewport {
        Viewport { pan_x, pan_y, zoom }
    }

    #[test]
    fn a_rect_maps_by_pan_then_zoom() {
        let canvas = Rect::xywh(240.0, 40.0, 800.0, 600.0);
        let mapped = doc_rect_to_screen(
            Rect::xywh(10.0, 20.0, 100.0, 50.0),
            canvas,
            &viewport(5.0, -3.0, 2.0),
        );
        assert_eq!(
            mapped.origin,
            Point2D::new(240.0 + 5.0 + 20.0, 40.0 - 3.0 + 40.0)
        );
        assert_eq!(mapped.size, Point2D::new(200.0, 100.0));
    }

    #[test]
    fn a_point_maps_the_same_way_its_rect_does() {
        let canvas = Rect::xywh(240.0, 40.0, 800.0, 600.0);
        let viewport = viewport(5.0, -3.0, 2.0);
        let rect = doc_rect_to_screen(Rect::xywh(10.0, 20.0, 0.0, 0.0), canvas, &viewport);
        assert_eq!(
            doc_point_to_screen(Point2D::new(10.0, 20.0), canvas, &viewport),
            rect.origin
        );
    }
}
