//! The reference card — the picture a design turn was asked to match, shown
//! beside the canvas (issue #63).
//!
//! Why it exists: after "make me a screen like this picture", the app showed
//! the result on the canvas and the picture only as a thumbnail inside the
//! chat transcript, where it scrolled away. Judging fidelity therefore meant
//! keeping the picture open somewhere else and looking back and forth, and the
//! honest answer was usually "roughly". The card puts the two side by side
//! without leaving the editor: the one thing the feature is for becomes
//! something a person can point at.
//!
//! What it deliberately is NOT: a tracing layer over the generated frame. A
//! translucent overlay would answer the same question more directly, but it
//! needs the picture aligned to a node's box in document space, and at the
//! moment the answer is evaluated (right after generation) neither the correct
//! scale nor the intent to align has been established — an overlay would have
//! to invent both, and a misaligned tracing layer is a worse lie than no
//! tracing layer. Side by side compares what is really there.
//!
//! The card owns no picture bytes. It reads them from the chat transcript
//! through [`ReferenceViewState::image_to_show`], so there is exactly one
//! owner of the reference and the card cannot outlive it.

use crate::widgets::canvas_viewport_image::{
    cached_bytes_for, note_pending_decode, required_raster_edge, store_remote_image_bytes,
};
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::text_metrics;
use crate::widgets::{host_canvas_geometry, PaintCx};
use crate::{Point2D, Rect, TextLayout, Theme};
use op_editor_core::{ChatImage, EditorState};

/// Card width. Wide enough that a desktop-scale reference is recognisable,
/// narrow enough to leave the generated frame the majority of the canvas.
pub const REFERENCE_CARD_W: f32 = 300.0;
/// Card height: header + picture area.
pub const REFERENCE_CARD_H: f32 = 260.0;
/// Gap between the card and the canvas edge it hangs off.
const INSET: f32 = 16.0;
const HEADER_H: f32 = 32.0;
const RADIUS: f32 = 12.0;
const CLOSE_BUTTON: f32 = 28.0;
const TITLE_FONT: f32 = 12.0;
const NAME_FONT: f32 = 10.0;
const PAD_X: f32 = 10.0;

/// What a press on the card resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceViewHit {
    /// The `×` in the header — the caller hides the card.
    Close,
    /// The card body. Consumed but otherwise inert: the card covers part of
    /// the canvas, and a press that fell through to the canvas would start a
    /// marquee under a picture the user was only looking at.
    Inside,
}

/// The reference card, resolved from live state.
pub struct ReferenceView<'a> {
    pub theme: Theme,
    /// Picture to show — borrowed from the chat transcript.
    pub image: &'a ChatImage,
    /// Header label, already translated by the caller's locale.
    pub title: &'static str,
}

impl<'a> ReferenceView<'a> {
    /// Resolve the card from `state`: `None` when it is closed, when it points
    /// at a picture the transcript no longer holds, or when the picture bytes
    /// are empty.
    pub fn from_state(state: &'a EditorState) -> Option<Self> {
        let image = state.editor_ui.reference_view.image_to_show(&state.chat)?;
        if image.data.is_empty() {
            return None;
        }
        Some(Self {
            theme: theme_for(&state.editor_ui),
            image,
            title: op_i18n::translate(state.editor_ui.locale, "ai.referenceView.title"),
        })
    }

    /// Where the card sits, or `None` when it is showing nothing.
    ///
    /// One predicate for paint and hit-testing, on purpose: a card that is not
    /// painted must not swallow a press. Getting that wrong is not cosmetic —
    /// the card covers part of the canvas, so an invisible card ate a click
    /// meant for a node under it (and a click meant to drill one level deeper).
    pub fn card_rect(state: &EditorState, viewport_w: f32, viewport_h: f32) -> Option<Rect> {
        state.editor_ui.reference_view.image_to_show(&state.chat)?;
        Some(Self::rect(state, viewport_w, viewport_h))
    }

    /// The card's geometry: the top-right corner of the canvas region, below
    /// the TopBar and left of the property rail.
    ///
    /// The canvas region — not the viewport — is the anchor, so the card never
    /// lands on the layer panel or the right rail, and it holds that promise
    /// when either rail is resized or the sidebar is closed (the region is the
    /// one rect every input path already agrees on). It moves with the region:
    /// selecting a node opens the right rail, which narrows the canvas and
    /// slides the card left rather than letting it cover the panel.
    pub fn rect(state: &EditorState, viewport_w: f32, viewport_h: f32) -> Rect {
        let (left, top, width, height) =
            host_canvas_geometry::canvas_region(state, viewport_w, viewport_h);
        let w = REFERENCE_CARD_W.min((width - INSET * 2.0).max(120.0));
        let h = REFERENCE_CARD_H.min((height - INSET * 2.0).max(100.0));
        Rect {
            origin: Point2D::new(left + (width - w - INSET).max(INSET), top + INSET),
            size: Point2D::new(w, h),
        }
    }

    /// The `×` target inside `rect`.
    pub fn close_rect(rect: Rect) -> Rect {
        Rect {
            origin: Point2D::new(
                rect.origin.x + rect.size.x - PAD_X - CLOSE_BUTTON + 6.0,
                rect.origin.y + (HEADER_H - CLOSE_BUTTON) / 2.0,
            ),
            size: Point2D::new(CLOSE_BUTTON, CLOSE_BUTTON),
        }
    }

    /// Resolve a press inside `rect`. `None` means the press missed the card
    /// and belongs to whatever is underneath.
    pub fn hit_test(rect: Rect, point: Point2D) -> Option<ReferenceViewHit> {
        if !rect.contains(point) {
            return None;
        }
        if Self::close_rect(rect).contains(point) {
            return Some(ReferenceViewHit::Close);
        }
        Some(ReferenceViewHit::Inside)
    }

    /// The picture area: the card below the header, minus the caption strip.
    fn image_rect(&self, rect: Rect) -> Rect {
        let top = rect.origin.y + HEADER_H;
        let bottom_pad = if self.image.name.is_empty() {
            PAD_X
        } else {
            PAD_X + NAME_FONT + 6.0
        };
        Rect {
            origin: Point2D::new(rect.origin.x + PAD_X, top + 4.0),
            size: Point2D::new(
                (rect.size.x - PAD_X * 2.0).max(0.0),
                (rect.size.y - HEADER_H - 4.0 - bottom_pad).max(0.0),
            ),
        }
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let theme = self.theme;
        // Raised surface + hairline border: the card floats above the canvas
        // like every other overlay, so it reads as chrome, not as a document
        // node the user could select.
        cx.backend.fill_round_rect(rect, RADIUS, theme.popover);
        cx.backend
            .stroke_round_rect(rect, RADIUS, theme.border, 1.0);

        // Header: the label that says which of the two pictures this is.
        let title_layout = TextLayout::single_run(
            self.title,
            text_metrics::CHROME_FONT_FAMILY,
            TITLE_FONT,
            theme.foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &title_layout,
            Point2D::new(
                rect.origin.x + PAD_X,
                jian_widgets::centered_text_baseline_y(
                    Rect {
                        origin: Point2D::new(rect.origin.x, rect.origin.y),
                        size: Point2D::new(rect.size.x, HEADER_H),
                    },
                    TITLE_FONT,
                ),
            ),
        );

        let close = Self::close_rect(rect);
        draw_icon(
            cx.backend,
            Icon::Close,
            Point2D::new(
                close.origin.x + (CLOSE_BUTTON - 14.0) / 2.0,
                close.origin.y + (CLOSE_BUTTON - 14.0) / 2.0,
            ),
            14.0,
            theme.muted_foreground,
            1.5,
        );

        self.paint_picture(cx, self.image_rect(rect));
    }

    /// Draw the picture with the canvas's own decode handshake
    /// (bytes → decode → draw), so a heavy reference does not block the frame
    /// that opened the card: the first frames show the placeholder, the
    /// picture lands when the backend has its raster.
    fn paint_picture(&self, cx: &mut PaintCx<'_>, area: Rect) {
        let theme = self.theme;
        cx.backend.fill_round_rect(area, 8.0, theme.muted);

        let id = self.image.id;
        // Web hosts decode by id through a bridge that reads the shared byte
        // cache, so the bytes have to be registered before the decode is
        // queued. Registering once (the cache answers on every later frame)
        // keeps a multi-megabyte reference from being copied per frame.
        if cached_bytes_for(id).is_none() {
            store_remote_image_bytes(id, self.image.data.clone());
        }
        let max_edge_px = required_raster_edge(area, cx.backend.dpi_scale());
        let sharp = cx
            .backend
            .image_decoded(id, self.image.data.as_slice(), max_edge_px);
        if !sharp {
            note_pending_decode(id, max_edge_px);
        }
        if sharp || cx.backend.image_resident(id) {
            cx.backend.save();
            cx.backend.clip_rect(area);
            // `draw_image` is aspect-fit + centred: the whole picture is
            // visible, and the same picture is never stretched into the box —
            // a distorted reference would make a faithful result look wrong.
            cx.backend.draw_image(area, id, self.image.data.as_slice());
            cx.backend.restore();
        } else {
            draw_icon(
                cx.backend,
                Icon::ImagePlus,
                Point2D::new(
                    area.origin.x + area.size.x / 2.0 - 11.0,
                    area.origin.y + area.size.y / 2.0 - 11.0,
                ),
                22.0,
                theme.muted_foreground,
                1.5,
            );
        }
        cx.backend.stroke_round_rect(area, 8.0, theme.border, 1.0);

        // The file name, so "which picture is this" is answerable from the
        // card alone once a transcript holds several.
        if !self.image.name.is_empty() {
            let available = (area.size.x).max(0.0);
            let name = text_metrics::fit_chrome(cx.backend, &self.image.name, available, NAME_FONT);
            let layout = TextLayout::single_run(
                &name,
                text_metrics::CHROME_FONT_FAMILY,
                NAME_FONT,
                theme.muted_foreground.to_jian(),
                Point2D::new(0.0, 0.0),
            );
            cx.backend.draw_text(
                &layout,
                Point2D::new(area.origin.x, area.origin.y + area.size.y + NAME_FONT + 2.0),
            );
        }
    }
}

#[cfg(test)]
#[path = "reference_view_tests.rs"]
mod tests;
