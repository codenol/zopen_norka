//! The mark a section wears when it no longer matches what it was built from.
//!
//! The property panel answers that question for the section somebody selected.
//! This answers it for the whole canvas: a reader walking a document sees which
//! sections have drifted without opening anything, which is the operator's
//! point — a screen that can say which analytics it came from is only half the
//! promise until it can also say that the answer has gone stale.
//!
//! ## Why the mark is drawn here and not by the node painter
//!
//! `paint_node` knows the scene and nothing else: it has no editor state, so it
//! cannot know which sections have drifted, and giving it one would put a store
//! lookup in the hottest loop of the renderer. This pass walks the same tree
//! afterwards, with the marks in hand, and paints only where there is something
//! to say — which is nothing at all for a document whose sections are in sync.
//!
//! ## Why nothing is painted for "not known"
//!
//! `SectionMarks::of` answers `None` when nobody has read a section yet, and a
//! section that has never been read is not a section that has drifted. Marking
//! it would accuse every section in every document that has not been opened,
//! and a warning that appears before anything is known is a warning people learn
//! to ignore.

use op_editor_core::editor_ui_state::section_panel::SectionMarks;
use op_editor_core::section::LinkState;
use op_editor_core::Viewport;

use crate::layout_scene::SceneNode;
use crate::theme::Theme;
use crate::widgets::canvas_doc_mapping::doc_rect_to_screen;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

/// Size of the warning glyph in screen pixels.
const MARK_SIZE: f32 = 14.0;
/// Inset from the section's top-right corner.
const MARK_INSET: f32 = 6.0;

/// Paint a mark on every section the reader has found out of step.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    cx: &mut PaintCx<'_>,
    nodes: &[SceneNode],
    marks: &SectionMarks,
    theme: &Theme,
    canvas_rect: Rect,
    viewport: &Viewport,
) {
    if marks.is_empty() {
        return;
    }
    for node in nodes {
        paint_node(cx, node, marks, theme, canvas_rect, viewport);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_node(
    cx: &mut PaintCx<'_>,
    node: &SceneNode,
    marks: &SectionMarks,
    theme: &Theme,
    canvas_rect: Rect,
    viewport: &Viewport,
) {
    let id = op_editor_core::NodeId::new(node.id.clone());
    if let Some(state) = marks.of(&id) {
        if !state.is_in_sync() {
            paint_mark(cx, node, state, theme, canvas_rect, viewport);
        }
    }
    for child in &node.children {
        paint_node(cx, child, marks, theme, canvas_rect, viewport);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_mark(
    cx: &mut PaintCx<'_>,
    node: &SceneNode,
    state: LinkState,
    theme: &Theme,
    canvas_rect: Rect,
    viewport: &Viewport,
) {
    let rect = doc_rect_to_screen(node.bounds, canvas_rect, viewport);
    if !overlaps(canvas_rect, rect) {
        return;
    }
    let colour = warning_color(theme);
    let centre = Point2D::new(
        rect.origin.x + rect.size.x - MARK_INSET - MARK_SIZE / 2.0,
        rect.origin.y + MARK_INSET + MARK_SIZE / 2.0,
    );
    // A disc behind the glyph: the canvas under a section is whatever somebody
    // put there, and a thin outline over a busy screen is a mark nobody sees.
    cx.backend.fill_oval(
        Rect::xywh(
            centre.x - MARK_SIZE / 2.0 - 2.0,
            centre.y - MARK_SIZE / 2.0 - 2.0,
            MARK_SIZE + 4.0,
            MARK_SIZE + 4.0,
        ),
        theme.popover,
    );
    let icon = match state {
        // A section whose screens moved is not the same fact as one whose
        // analytics moved, and a reader looking at the canvas can only be told
        // which by the glyph: a triangle for "something changed", an octagon
        // for "what it was built from is gone".
        LinkState::AssetMissing => Icon::AlertOctagon,
        // A padlock rather than a warning, because there is nothing to fix and
        // nothing went wrong: the asset this section names exists, and this
        // reader may not open it (issue #110). Drawing it as a warning would
        // send somebody hunting for a fault in a document they are simply not
        // allowed to fetch.
        LinkState::NotReadable => Icon::Lock,
        _ => Icon::AlertTriangle,
    };
    draw_icon(
        cx.backend,
        icon,
        Point2D::new(centre.x - MARK_SIZE / 2.0, centre.y - MARK_SIZE / 2.0),
        MARK_SIZE,
        colour,
        1.6,
    );
}

/// The colour a drift is drawn in.
fn warning_color(theme: &Theme) -> Color {
    theme.destructive
}

/// Whether two rects share any area. Written out rather than reached for: the
/// geometry type deliberately carries no `intersects`, and this pass has to ask
/// the question once per marked section, not once per node.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.origin.x < b.origin.x + b.size.x
        && b.origin.x < a.origin.x + a.size.x
        && a.origin.y < b.origin.y + b.size.y
        && b.origin.y < a.origin.y + a.size.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::NodeId;

    #[test]
    fn an_offscreen_mark_is_not_painted() {
        let canvas = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        assert!(overlaps(canvas, Rect::xywh(50.0, 50.0, 10.0, 10.0)));
        assert!(!overlaps(canvas, Rect::xywh(200.0, 0.0, 10.0, 10.0)));
    }

    #[test]
    fn a_document_whose_sections_are_in_sync_paints_nothing() {
        let marks = SectionMarks::default();
        assert!(marks.is_empty());
        // The early return is the whole cost of this pass for such a document.
    }

    #[test]
    fn the_mark_knows_which_sections_have_drifted() {
        let mut marks = SectionMarks::default();
        let broken = NodeId::new("s1");
        let fine = NodeId::new("s2");
        marks.replace(vec![
            (
                broken.clone(),
                LinkState::Broken {
                    side: op_editor_core::section::MovedSide::Both,
                },
            ),
            (fine.clone(), LinkState::InSync),
        ]);

        assert_eq!(
            marks.of(&broken).map(|state| state.is_in_sync()),
            Some(false)
        );
        assert_eq!(marks.of(&fine).map(|state| state.is_in_sync()), Some(true));
        assert_eq!(marks.of(&NodeId::new("unknown")), None);
    }
}
