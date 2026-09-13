//! Comment pins — the markers an element carries when something was said about
//! it.
//!
//! ## Why the geometry is a pure function and not "wherever paint put it"
//!
//! A pin is a small target on top of a large, zoomable canvas: the slightest
//! disagreement between where it is painted and where a click is tested turns
//! into "the pin does nothing" at one zoom level and "the pin next to it opens"
//! at another. So the anchor, the marker's rect and its hit rect are all pure
//! functions of the element's screen bounds and the canvas region
//! ([`pin_anchor`], [`pin_rect`], [`pin_hit_rect`]), and paint and hit-test call
//! the same ones. The file browser's cards are computed the same way, for the
//! same reason.
//!
//! ## Why a thread with no element still exists
//!
//! [`place_pins`] drops a thread whose node the document no longer has — there
//! is nowhere to draw a marker, and a pin at the origin would be a lie about
//! where the comment is. It does not drop the *thread*: the list panel shows it
//! with a "no pin" mark, which is what [`place_pins`]' caller asks for by
//! asking twice (once for pins, once per row). A conversation whose subject was
//! renamed away is still a conversation somebody had.
//!
//! ## Why pins stack instead of overlapping
//!
//! Two threads can be about one element, and two markers at the same point make
//! the lower one unreachable. Each pin after the first on the same element steps
//! sideways by [`PIN_STACK_STEP`], so every pin stays clickable — and the step is
//! applied before clamping, so the whole fan stays inside the canvas.

use op_editor_core::editor_ui_state::CommentsUiState;
use op_editor_core::Viewport;

use crate::layout_scene::SceneNode;
use crate::theme::Theme;
use crate::widgets::canvas_doc_mapping::doc_rect_to_screen;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect};

use super::comment_identity::role_colour;
use super::comment_paint::{label_on, text};

/// Diameter of the marker circle.
pub const PIN_DIAMETER: f32 = 22.0;
/// How far each further pin on one element steps to the side.
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

/// One thread waiting for a marker, before its element's position is known.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPinThread {
    pub thread_id: i64,
    /// 1-based position in the document's thread list — the number the marker
    /// shows, and the same number the list panel shows against the row.
    pub ordinal: usize,
    pub node_id: String,
    /// The opener's role colour, or `None` for an unknown/absent role.
    pub color: Option<crate::Color>,
    pub resolved: bool,
    /// Comments after the opening one, shown as a count badge.
    pub replies: usize,
}

/// Every thread of the document, as pin sources.
pub fn threads_for(ui: &CommentsUiState) -> Vec<CommentPinThread> {
    ui.threads
        .iter()
        .enumerate()
        .map(|(index, thread)| CommentPinThread {
            thread_id: thread.id,
            ordinal: index + 1,
            node_id: thread.node_id.clone(),
            color: role_colour(thread.opener().and_then(|author| author.role.as_deref())),
            resolved: thread.resolved,
            replies: thread.reply_count(),
        })
        .collect()
}

/// A placed marker: geometry in canvas-screen space, plus what it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPin {
    pub thread_id: i64,
    pub ordinal: usize,
    pub node_id: String,
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
    /// than a marker should be, and a document with a hundred threads on one
    /// element has stopped being a review anyway. The list panel still numbers
    /// every row.
    pub fn label(&self) -> String {
        if self.ordinal > 99 {
            "99+".to_string()
        } else {
            self.ordinal.to_string()
        }
    }
}

/// The marker's centre for an element at `bounds`, in canvas-screen space.
///
/// The top-right corner of the element, because that is where a marker reads as
/// belonging to the element without covering its content — a pin on the centre
/// hides the very thing being discussed. `stack` is how many markers are already
/// on this element; each steps sideways so the fan stays clickable.
pub fn pin_anchor(bounds: Rect, canvas: Rect, stack: usize) -> Point2D {
    let corner = Point2D::new(
        bounds.origin.x + bounds.size.x,
        bounds.origin.y + stack as f32 * PIN_STACK_STEP,
    );
    Point2D::new(
        corner.x.clamp(
            canvas.origin.x + PIN_INSET,
            canvas.origin.x + canvas.size.x - PIN_INSET,
        ),
        corner.y.clamp(
            canvas.origin.y + PIN_INSET,
            canvas.origin.y + canvas.size.y - PIN_INSET,
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

/// Place the markers whose element is present.
///
/// `bounds_of` answers an element's screen-space bounds, or `None` when the
/// document has no such element. Threads are placed in list order, so the
/// fan on one element grows in the same order the panel numbers them.
pub fn place_pins(
    threads: &[CommentPinThread],
    mut bounds_of: impl FnMut(&str) -> Option<Rect>,
    canvas: Rect,
) -> Vec<CommentPin> {
    let mut placed: Vec<CommentPin> = Vec::with_capacity(threads.len());
    let mut stacks: Vec<(String, usize)> = Vec::new();
    for thread in threads {
        let Some(bounds) = bounds_of(&thread.node_id) else {
            continue;
        };
        let stack = match stacks.iter_mut().find(|(node, _)| *node == thread.node_id) {
            Some((_, count)) => {
                *count += 1;
                *count - 1
            }
            None => {
                stacks.push((thread.node_id.clone(), 1));
                0
            }
        };
        placed.push(CommentPin {
            thread_id: thread.thread_id,
            ordinal: thread.ordinal,
            node_id: thread.node_id.clone(),
            rect: pin_rect(pin_anchor(bounds, canvas, stack)),
            color: thread.color,
            resolved: thread.resolved,
            replies: thread.replies,
        });
    }
    placed
}

/// Place the markers against the render-node tree.
///
/// The tree carries the element bounds the canvas paints from — `aggregate_bounds`
/// is a container's child union — so a pin sits where the element visibly is,
/// including at a zoom that changed since the last frame.
pub fn scene_pins(
    threads: &[CommentPinThread],
    roots: &[SceneNode],
    canvas: Rect,
    viewport: &Viewport,
) -> Vec<CommentPin> {
    place_pins(
        threads,
        |id| {
            let node = find_node(roots, id)?;
            // A hidden element has nothing on screen to pin, and its bounds are
            // whatever the last visible layout left behind.
            if node.hidden {
                return None;
            }
            let bounds = node.aggregate_bounds();
            if bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
                return None;
            }
            Some(doc_rect_to_screen(bounds, canvas, viewport))
        },
        canvas,
    )
}

fn find_node<'a>(roots: &'a [SceneNode], id: &str) -> Option<&'a SceneNode> {
    for root in roots {
        if root.id == id {
            return Some(root);
        }
        if let Some(node) = find_node(&root.children, id) {
            return Some(node);
        }
    }
    None
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
/// design, so it must stay visible over the element it is about, and a handle
/// the user is about to drag must stay grabbable.
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
        // A ring in the panel colour under the marker separates it from an
        // element painted in the same colour.
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
