//! Comment pins — the markers a page carries where something was said.
//!
//! ## Why the geometry is a pure function and not "wherever paint put it"
//!
//! A pin is a small target on top of a large, zoomable canvas: the slightest
//! disagreement between where it is painted and where a click is tested turns
//! into "the pin does nothing" at one zoom level and "the pin next to it opens"
//! at another. So the anchor, the marker's rect and its hit rect are all pure
//! functions of the document point and the canvas region ([`pin_anchor`],
//! [`pin_rect`], [`pin_hit_rect`]), and paint and hit-test call the same ones.
//! The file browser's cards are computed the same way, for the same reason.
//!
//! ## Why the document-space point is the only thing stored
//!
//! A thread's anchor is a point on a page (see
//! `op_editor_core::editor_ui_state::CommentAnchor`); everything on screen is
//! derived from it per frame through the canvas' own pan/zoom. That is the one
//! place the two spaces meet: `anchor` is document space, `rect` is screen
//! space, and the conversion happens here — so a pan or a zoom moves every
//! marker with the design instead of leaving the markers behind. A pin that
//! cached a screen position would be right for exactly one frame.
//!
//! ## Why pins stack instead of overlapping
//!
//! Two comments can be placed at one point — a reviewer sends one and clicks the
//! same spot again — and two markers at the same point make the lower one
//! unreachable. Each pin after the first at the same document point steps
//! sideways by [`PIN_STACK_STEP`], so every pin stays clickable — and the step
//! is applied before clamping, so the whole fan stays inside the canvas.
//!
//! ## Why other pages' threads are not placed
//!
//! A pin belongs to the page its anchor names, and the canvas shows one page.
//! Placing another page's thread would need a coordinate this page does not
//! have; the rail counts those threads instead, so nothing is silently lost.
//!
//! ## Why a thread with no anchor is not a source at all
//!
//! [`threads_for_page`] filters to the threads that have a place on this page.
//! A migrated thread has no page and no coordinates, and inventing a default
//! would put a marker somewhere the reviewer never clicked.

use op_editor_core::editor_ui_state::CommentsUiState;
use op_editor_core::Viewport;

use crate::theme::Theme;
use crate::widgets::canvas_doc_mapping::doc_point_to_screen;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect};

use super::comment_identity::role_colour;
use super::comment_paint::{label_on, text};

/// Diameter of the marker circle.
pub const PIN_DIAMETER: f32 = 22.0;
/// How far each further pin at one point steps to the side.
pub const PIN_STACK_STEP: f32 = 16.0;
/// How far the marker may overhang the canvas edge before it is pulled back in.
///
/// Zero overhang: a pin half outside the canvas region is half unreachable, and
/// the canvas clips paint to its own rect, so the hidden half is not even
/// visible to aim at.
const PIN_INSET: f32 = PIN_DIAMETER / 2.0 + 2.0;
/// Extra hit area around a marker — a finger is not a cursor.
const PIN_HIT_PAD: f32 = 3.0;
const PIN_HIT_PAD_TOUCH: f32 = 10.0;
/// Document points closer than this are "the same point" for the stack fan.
///
/// Sub-pixel in document space and far below the marker's own size on screen,
/// so two pins this close are visually on top of each other whatever the zoom.
const STACK_EPSILON: f64 = 0.5;

/// One thread that has a marker to draw, on the page being shown.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPinThread {
    pub thread_id: i64,
    /// 1-based position among this page's markers — the number both the marker
    /// and the rail row show, because both walk the same sequence.
    pub ordinal: usize,
    /// The point on this page, in document space.
    pub anchor: op_editor_core::editor_ui_state::CommentAnchor,
    /// The opener's role colour, or `None` for an unknown/absent role.
    pub color: Option<crate::Color>,
    pub resolved: bool,
    /// Comments after the opening one, shown as a count badge.
    pub replies: usize,
}

/// This page's markers' sources, in the order the rail lists them.
///
/// One filter, asked once: a thread has a marker here when it has a drawable
/// anchor that names this page (see `CommentsUiState::pinned_on_page`). A thread
/// with no anchor — one the daemon migrated from the element-keyed format — has
/// nothing to draw and is skipped; the rail still lists it, marked as having no
/// pin.
///
/// The page is passed in rather than read from the scene because the caller
/// already knows which page it is building for
/// (`EditorState::active_page_identity`), and one answer is what keeps a marker's
/// number equal to its row's.
pub fn threads_for_page(ui: &CommentsUiState, page_id: &str) -> Vec<CommentPinThread> {
    ui.pinned_on_page(page_id)
        .into_iter()
        .enumerate()
        .map(|(index, thread)| CommentPinThread {
            thread_id: thread.id,
            ordinal: index + 1,
            // Present by construction: `pinned_on_page` filters on it.
            anchor: thread.anchor.clone().unwrap_or_default(),
            color: role_colour(thread.opener().and_then(|author| author.role.as_deref())),
            resolved: thread.resolved,
            replies: thread.reply_count(),
        })
        .collect()
}

/// A placed marker: a screen-space rect, plus what it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPin {
    pub thread_id: i64,
    /// 1-based position among this page's pins — the same number the rail row
    /// shows, because both count the page's threads in the list's order.
    pub ordinal: usize,
    /// The point it was placed from, in document space. Kept beside the rect so
    /// a caller that needs to frame the pin (the rail's "jump to it") does not
    /// have to invert the screen mapping to recover it.
    pub anchor: op_editor_core::editor_ui_state::CommentAnchor,
    /// The circle's bounding box, in canvas-screen space.
    pub rect: Rect,
    pub color: Option<crate::Color>,
    pub resolved: bool,
    pub replies: usize,
}

impl CommentPin {
    /// The glyph painted inside the marker.
    ///
    /// Two digits, then `99+`: a three-digit number would need a wider circle
    /// than a marker should be, and a page with a hundred threads has stopped
    /// being a review anyway. The rail still numbers every row.
    pub fn label(&self) -> String {
        if self.ordinal > 99 {
            "99+".to_string()
        } else {
            self.ordinal.to_string()
        }
    }

    /// The point a press may land on and still open this thread.
    pub fn hit_rect(&self, touch: bool) -> Rect {
        pin_hit_rect(self.rect, touch)
    }
}

/// The marker's centre for a document point, in canvas-screen space.
///
/// `stack` is how many markers already sit at this point; each steps down and
/// sideways so the fan stays clickable. The clamp keeps the whole marker inside
/// the canvas whatever the element does at the edge of the viewport.
pub fn pin_anchor(anchor: Point2D, canvas: Rect, stack: usize) -> Point2D {
    let step = stack as f32 * PIN_STACK_STEP;
    Point2D::new(
        (anchor.x + step).clamp(
            canvas.origin.x + PIN_INSET,
            (canvas.origin.x + canvas.size.x - PIN_INSET).max(canvas.origin.x + PIN_INSET),
        ),
        (anchor.y + step).clamp(
            canvas.origin.y + PIN_INSET,
            (canvas.origin.y + canvas.size.y - PIN_INSET).max(canvas.origin.y + PIN_INSET),
        ),
    )
}

/// The marker's bounding box for a centre.
pub fn pin_rect(anchor: Point2D) -> Rect {
    Rect::xywh(
        anchor.x - PIN_DIAMETER / 2.0,
        anchor.y - PIN_DIAMETER / 2.0,
        PIN_DIAMETER,
        PIN_DIAMETER,
    )
}

/// Where a click may land and still count as this marker.
///
/// Derived from the painted rect rather than recomputed from the anchor: the
/// two can then only ever differ by the pad, never by a rounding step that
/// leaves a one-pixel ring the marker visibly covers but does not answer for.
pub fn pin_hit_rect(rect: Rect, touch: bool) -> Rect {
    let pad = if touch {
        PIN_HIT_PAD_TOUCH
    } else {
        PIN_HIT_PAD
    };
    Rect::xywh(
        rect.origin.x - pad,
        rect.origin.y - pad,
        rect.size.x + pad * 2.0,
        rect.size.y + pad * 2.0,
    )
}

/// Place the sources on the canvas.
///
/// Threads are placed in list order, so the fan at one point grows in the same
/// order the rail numbers them, and the number a marker shows is the number its
/// source already carries.
pub fn place_pins(
    threads: &[CommentPinThread],
    canvas: Rect,
    viewport: &Viewport,
) -> Vec<CommentPin> {
    // The fan's cursor: a document point, and how many markers are already
    // there. Compared with an epsilon because a wire round trip is free to
    // return `12.000000001` for a click that landed on `12`.
    let mut stacks: Vec<(Point2D, usize)> = Vec::new();
    let mut placed: Vec<CommentPin> = Vec::with_capacity(threads.len());
    for thread in threads {
        let doc = Point2D::new(thread.anchor.x as f32, thread.anchor.y as f32);
        let stack = match stacks.iter_mut().find(|(point, _)| same_point(*point, doc)) {
            Some((_, count)) => {
                *count += 1;
                *count - 1
            }
            None => {
                stacks.push((doc, 1));
                0
            }
        };
        let screen = doc_point_to_screen(doc, canvas, viewport);
        placed.push(CommentPin {
            thread_id: thread.thread_id,
            ordinal: thread.ordinal,
            anchor: thread.anchor.clone(),
            rect: pin_rect(pin_anchor(screen, canvas, stack)),
            color: thread.color,
            resolved: thread.resolved,
            replies: thread.replies,
        });
    }
    placed
}

fn same_point(a: Point2D, b: Point2D) -> bool {
    (a.x - b.x).abs() < STACK_EPSILON as f32 && (a.y - b.y).abs() < STACK_EPSILON as f32
}

/// The topmost marker under `point`, if any.
///
/// Searched back to front because that is the order they are painted in, so a
/// marker a reviewer can see on top is the one their click opens.
pub fn hit_test(pins: &[CommentPin], point: Point2D, touch: bool) -> Option<i64> {
    pins.iter()
        .rev()
        .find(|pin| contains(pin_hit_rect(pin.rect, touch), point))
        .map(|pin| pin.thread_id)
}

fn contains(rect: Rect, point: Point2D) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.x
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.y
}

/// Paint every marker.
///
/// Above the node tree and below the selection chrome: a pin is a comment ON the
/// design, so it must stay visible over the page it is about, and a handle the
/// user is about to drag must stay grabbable.
pub fn paint(cx: &mut PaintCx<'_>, theme: &Theme, pins: &[CommentPin], hovered: Option<i64>) {
    for pin in pins {
        paint_pin(cx, theme, pin, hovered == Some(pin.thread_id));
    }
}

fn paint_pin(cx: &mut PaintCx<'_>, theme: &Theme, pin: &CommentPin, hovered: bool) {
    let accent = pin.color.unwrap_or(theme.muted_foreground);
    let rect = if hovered {
        inflate(pin.rect, 1.0)
    } else {
        pin.rect
    };
    if pin.resolved {
        // A closed thread is still shown, but as an outline: the conversation is
        // worth finding again, and a solid marker would claim it is still open.
        cx.backend.fill_oval(rect, theme.card.with_alpha(0.92));
        cx.backend.stroke_oval(rect, accent.with_alpha(0.75), 1.5);
    } else {
        // A ring in the panel colour under the marker separates it from a page
        // painted in the same colour.
        cx.backend
            .fill_oval(inflate(rect, 1.5), theme.card.with_alpha(0.9));
        cx.backend.fill_oval(rect, accent);
    }
    let label = pin.label();
    let color = if pin.resolved {
        accent
    } else {
        label_on(accent)
    };
    let width = text_metrics::measure_chrome_weighted(cx.backend, &label, 11.0, 700);
    text(
        cx,
        &label,
        11.0,
        color,
        Point2D::new(
            rect.origin.x + (rect.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(rect, 11.0),
        ),
        700,
    );
    if !pin.resolved && pin.replies > 0 {
        // A ring rather than a second badge: the marker is 22 px, and the count
        // is there to answer "is there more here" at a glance, not to be read.
        cx.backend
            .stroke_oval(inflate(rect, 3.0), accent.with_alpha(0.55), 1.5);
    }
}

fn inflate(rect: Rect, by: f32) -> Rect {
    Rect::xywh(
        rect.origin.x - by,
        rect.origin.y - by,
        rect.size.x + by * 2.0,
        rect.size.y + by * 2.0,
    )
}

#[cfg(test)]
#[path = "comment_pins_tests.rs"]
mod tests;
