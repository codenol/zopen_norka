//! Comment flow shared by the widget hosts.
//!
//! Mirrors `recovery_banner_flow`: placement, paint and press are an
//! `EditorState` mutation plus a widget-layer hit-test, so a host's arm is only
//! "resolve against this canvas, run the platform tail" and both hosts behave
//! identically by construction. Three surfaces are here because they are one
//! interaction: the rail lists the document's conversations, a row or a pin
//! opens the popover, and the popover writes back into the state.
//!
//! ## Why the pins are passed in rather than looked up
//!
//! A pin's screen position follows from the point on its page and the current
//! viewport, and both belong to the caller's `CanvasViewport` snapshot. Passing
//! the placed pins means the popover hangs under the marker the paint pass
//! actually drew — the alternative, recomputing the anchor here, is a second
//! implementation of the viewport transform that could disagree with the first
//! by exactly the pan the user just made.
//!
//! ## Why a composer being written places itself
//!
//! A thread being written has no pin yet — it is the click that will make one —
//! so the popover is anchored to the point the reviewer clicked, converted
//! through the same document→screen mapping the pin will use when the write
//! comes back. The box therefore appears *at the place the comment is about*,
//! and the marker that arrives after the send lands under it. The previous
//! behaviour (the canvas' top-left corner) put the field in a corner the
//! reviewer was not looking at, and the pin then appeared where they never
//! clicked.
//!
//! ## What a press asks the host to do
//!
//! Opening a thread is the widget layer's to do; framing the canvas on a pin
//! needs the viewport, which is the host's. So [`press_panel`] returns a
//! [`CommentsPanelAction`] naming the point to frame, and the host runs its own
//! camera move against its own viewport. Nothing here writes a viewport it
//! cannot see.

use op_editor_core::editor_ui_state::{CommentAnchor, CommentComposer};
use op_editor_core::EditorState;

use crate::widgets::canvas_doc_mapping::doc_point_to_screen;
use crate::widgets::comment_pins::{self, CommentPin};
use crate::widgets::comment_thread_popover::{
    CommentPopoverHit, CommentPopoverModel, CommentThreadPopover,
};
use crate::widgets::comments_panel::{self, CommentsPanel, CommentsPanelHit};
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};

/// The canvas the comment surfaces are placed against.
///
/// The rect and the page are one value because every caller that needs one of
/// them needs the other: a document point can only become a screen position if
/// both are known, and a caller that had the rect but not the page could place
/// a marker from another page's coordinates without noticing.
#[derive(Debug, Clone, Copy)]
pub struct CommentCanvas<'a> {
    pub rect: Rect,
    /// The page the canvas is showing — never empty, because a document with no
    /// authored pages still names one (`EditorState::active_page_identity`).
    pub page_id: &'a str,
}

impl<'a> CommentCanvas<'a> {
    pub fn new(rect: Rect, page_id: &'a str) -> Self {
        Self { rect, page_id }
    }
}

/// What a press on the thread list asked the host to do beyond the widget
/// layer's reach.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentsPanelAction {
    /// Handled inside the widget layer — the state already changed.
    Handled,
    /// Frame this point on the canvas: the row the reviewer clicked may be
    /// about a comment that is off screen right now.
    RevealAnchor(CommentAnchor),
}

/// The rect the popover should hang from.
///
/// An existing thread hangs under its own marker. A thread being written hangs
/// under the point the click landed on — the place its pin will occupy — which
/// is why the pending anchor is converted through the same mapping rather than
/// being given a corner of its own.
pub fn popover_anchor(state: &EditorState, canvas: CommentCanvas<'_>, pins: &[CommentPin]) -> Rect {
    let ui = &state.editor_ui.comments;
    match ui.composer() {
        Some(CommentComposer::Thread(id)) => pins
            .iter()
            .find(|pin| pin.thread_id == id)
            .map(|pin| pin.rect)
            .unwrap_or_else(|| unpinned_anchor(canvas.rect)),
        Some(CommentComposer::NewThread(anchor)) => pending_pin_rect(&anchor, canvas, state),
        None => unpinned_anchor(canvas.rect),
    }
}

/// Where a composer for an unwritten comment hangs from: its future pin.
///
/// `None`-page and off-page anchors fall back to the rail's own corner: the
/// point exists, but not on the page this canvas is showing, so there is no
/// place on the design to point at. The write still carries the anchor, so the
/// comment is not lost by that.
fn pending_pin_rect(
    anchor: &CommentAnchor,
    canvas: CommentCanvas<'_>,
    state: &EditorState,
) -> Rect {
    if !anchor.is_placeable() || canvas.page_id != anchor.page_id {
        return unpinned_anchor(canvas.rect);
    }
    let screen = doc_point_to_screen(
        Point2D::new(anchor.x as f32, anchor.y as f32),
        canvas.rect,
        &state.viewport,
    );
    comment_pins::pin_rect(comment_pins::pin_anchor(screen, canvas.rect, 0))
}

/// Where the popover hangs when its thread has no pin on this canvas.
///
/// A thread whose anchor belongs to another page still opens — from the rail,
/// which is where such a thread is listed — and it needs somewhere to be. The
/// canvas' own top-left corner is the honest place: nothing on this page is
/// being pointed at.
fn unpinned_anchor(canvas: Rect) -> Rect {
    Rect::xywh(canvas.origin.x + 8.0, canvas.origin.y + 8.0, 0.0, 0.0)
}

/// Build the popover this frame, if a composer is open.
///
/// The clock is read from the editor state rather than passed in: an age is the
/// one thing on this surface that depends on time, and two hosts passing two
/// different clocks (frame-relative on one, wall-clock on the other) is how a
/// comment written a minute ago reads as fifty years old. The host already
/// refreshes `now_unix_ms` once per frame.
pub fn popover_for(state: &EditorState) -> Option<CommentThreadPopover> {
    let ui = &state.editor_ui.comments;
    let model = CommentPopoverModel::for_comments(
        ui,
        state.editor_ui.effective_locale(),
        // This build carries no account-id projection in the editor state, so
        // no comment can be recognised as the viewer's own; a local-operator
        // comment still reads as one, and everyone else by name.
        ui.viewer_id.as_deref(),
        ui.composer_focused,
    )?;
    Some(CommentThreadPopover::new(
        model,
        theme_for(&state.editor_ui),
        state.editor_ui.now_unix_ms,
    ))
}

/// Paint the popover, if one is open. Returns the rect it painted, for the
/// hosts' hit-test parity.
pub fn paint_popover(
    cx: &mut PaintCx<'_>,
    state: &EditorState,
    canvas: CommentCanvas<'_>,
    pins: &[CommentPin],
) -> Option<Rect> {
    let popover = popover_for(state)?;
    let rect = popover.rect_at(popover_anchor(state, canvas, pins), canvas.rect);
    popover.paint(cx, rect);
    Some(rect)
}

/// Route a press against the popover.
///
/// Takes the rect the paint pass used rather than re-resolving it: the popover
/// is placed against the pin's own screen rect, and the anchor is not derivable
/// here without the pins the caller already has.
///
/// `true` when the press was consumed — including the press OUTSIDE the popover
/// that closes it. Consuming that one is deliberate: a click that dismisses a
/// floating panel is a click at the panel, not at the canvas underneath it, and
/// letting it through would drop a second pin where the reviewer was aiming at
/// the first.
///
/// An answer that writes (send, resolve) becomes a request in the state rather
/// than a call here: the widget layer owns no transport. The host's frame tick
/// drains it.
pub fn press_popover(state: &mut EditorState, rect: Option<Rect>, point: Point2D) -> bool {
    let Some(rect) = rect else {
        return false;
    };
    // Re-resolved, not trusted from the cached rect: the composer may have been
    // answered, or closed by a keyboard rung, between the paint and this press.
    let hit = match popover_for(state) {
        Some(popover) => popover.hit_test(rect, point),
        None => return false,
    };
    match hit {
        CommentPopoverHit::Close => {
            state.editor_ui.comments.close();
            true
        }
        CommentPopoverHit::FocusInput => {
            state.editor_ui.comments.focus_composer();
            true
        }
        CommentPopoverHit::Send => {
            state.editor_ui.comments.send();
            true
        }
        CommentPopoverHit::Resolution => {
            if let Some(id) = state.editor_ui.comments.open_thread {
                if state
                    .editor_ui
                    .comments
                    .thread(id)
                    .is_some_and(|thread| thread.resolved)
                {
                    state.editor_ui.comments.reopen(id);
                } else {
                    state.editor_ui.comments.resolve(id);
                }
            }
            true
        }
        CommentPopoverHit::Inside => true,
        CommentPopoverHit::Outside => {
            state.editor_ui.comments.close();
            true
        }
    }
}

/// Build the rail's thread list this frame, if the comment tool is active.
pub fn panel_for(state: &EditorState, page_id: &str) -> Option<CommentsPanel> {
    let ui = &state.editor_ui.comments;
    if !ui.rail_visible() {
        return None;
    }
    let page = page_id;
    Some(CommentsPanel::new(
        theme_for(&state.editor_ui),
        state.editor_ui.effective_locale(),
        comments_panel::rows(
            ui,
            state.editor_ui.effective_locale(),
            ui.viewer_id.as_deref(),
            page,
        ),
        ui.loading,
        ui.error.clone(),
        state.editor_ui.now_unix_ms,
        ui.open_count_elsewhere(page),
    ))
}

/// Paint the thread list in the right rail. Returns the rect it painted.
///
/// The rail's rect is passed in rather than derived: the rail is the
/// inspector's own slot, so whoever decides which occupant it has this frame is
/// also the one that knows where it is.
pub fn paint_panel(
    cx: &mut PaintCx<'_>,
    state: &EditorState,
    rect: Rect,
    page_id: &str,
) -> Option<Rect> {
    let panel = panel_for(state, page_id)?;
    panel.paint(cx, rect);
    Some(rect)
}

/// Route a press against the rail's thread list.
///
/// `None` when the point is outside it, so the tiers below keep the click —
/// a press in the canvas beside the rail is aimed at the design, and in comment
/// mode that means it places a pin.
pub fn press_panel(
    state: &mut EditorState,
    rect: Option<Rect>,
    point: Point2D,
    page_id: &str,
) -> Option<CommentsPanelAction> {
    let rect = rect?;
    let panel = panel_for(state, page_id)?;
    match panel.hit_test(rect, point) {
        CommentsPanelHit::Close => {
            state.editor_ui.comments.end_mode();
            Some(CommentsPanelAction::Handled)
        }
        CommentsPanelHit::Row(thread_id) => {
            state.editor_ui.comments.open(thread_id);
            // The camera move is the host's: centring on a document point needs
            // the viewport, which this layer cannot see. Selecting a node would
            // be a different gesture — the comment is about a place, not about
            // whatever element happens to be under it now.
            //
            // A thread with no pin has nowhere to jump to, so opening it is the
            // whole answer: the row says the thread has no marker, and a camera
            // move invented for it would be a jump to a place nobody named.
            match state.editor_ui.comments.thread(thread_id)?.anchor.clone() {
                Some(anchor) => Some(CommentsPanelAction::RevealAnchor(anchor)),
                None => Some(CommentsPanelAction::Handled),
            }
        }
        CommentsPanelHit::Inside => Some(CommentsPanelAction::Handled),
        CommentsPanelHit::Outside => None,
    }
}

#[cfg(test)]
#[path = "comments_flow_tests.rs"]
mod tests;
