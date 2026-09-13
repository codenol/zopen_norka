//! Comment flow shared by the widget hosts.
//!
//! Mirrors `recovery_banner_flow`: placement, paint and press are an
//! `EditorState` mutation plus a widget-layer hit-test, so a host's arm is only
//! "resolve against this canvas, run the platform tail" and both hosts behave
//! identically by construction. Three surfaces are here because they are one
//! interaction: a pin opens the popover, the popover writes to the state, and
//! the panel is the list of the same threads.
//!
//! ## Why the pins are passed in rather than looked up
//!
//! A pin's screen position is a property of the canvas viewport and the render
//! tree, and both belong to the caller's `CanvasViewport` snapshot. Passing the
//! placed pins means the popover hangs under the marker the paint pass actually
//! drew — the alternative, recomputing the anchor here from document bounds,
//! is a second implementation of the viewport transform that could disagree
//! with the first by exactly the pan the user just made.
//!
//! ## What a press asks the host to do
//!
//! Selecting a node and opening a thread are the widget layer's to do; framing
//! the camera on that node needs the render tree, which is the host's. So
//! [`press_panel`] returns a [`CommentsPanelAction`] naming the node to frame,
//! and the host runs its own `zoom_to_fit_node` against its own scene. Nothing
//! here writes a viewport it cannot see.

use op_editor_core::editor_ui_state::CommentComposer;
use op_editor_core::{EditorState, NodeId};

use crate::widgets::comment_pins::CommentPin;
use crate::widgets::comment_thread_popover::{
    CommentPopoverHit, CommentPopoverModel, CommentThreadPopover,
};
use crate::widgets::comments_panel::{self, CommentsPanel, CommentsPanelHit};
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};

/// What a press on the thread list asked the host to do beyond the widget
/// layer's reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentsPanelAction {
    /// Handled inside the widget layer — the state already changed.
    Handled,
    /// Frame this element on the canvas: the row the reviewer clicked is about
    /// an element that may be off screen right now.
    RevealNode(String),
}

/// Where the popover hangs when its thread has no pin.
///
/// A thread whose element was deleted still opens — from the panel, which is
/// where such a thread lives — and it needs somewhere to be. The canvas' own
/// top-left corner is the honest place: no element is being pointed at.
fn unpinned_anchor(canvas: Rect) -> Rect {
    Rect::xywh(canvas.origin.x + 8.0, canvas.origin.y + 8.0, 0.0, 0.0)
}

/// The rect the popover should hang from.
pub fn popover_anchor(state: &EditorState, canvas: Rect, pins: &[CommentPin]) -> Rect {
    let ui = &state.editor_ui.comments;
    match ui.composer() {
        Some(CommentComposer::Thread(id)) => pins
            .iter()
            .find(|pin| pin.thread_id == id)
            .map(|pin| pin.rect)
            .unwrap_or_else(|| unpinned_anchor(canvas)),
        // A thread being written has no pin yet — it is the click that will make
        // one — so it opens beside the panel's own corner rather than following
        // a marker that does not exist.
        Some(CommentComposer::NewThread(_)) => unpinned_anchor(canvas),
        None => unpinned_anchor(canvas),
    }
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
    canvas: Rect,
    pins: &[CommentPin],
) -> Option<Rect> {
    let popover = popover_for(state)?;
    let rect = popover.rect_at(popover_anchor(state, canvas, pins), canvas);
    popover.paint(cx, rect);
    Some(rect)
}

/// Route a press against the popover.
///
/// Takes the rect the paint pass used rather than re-resolving it: the popover
/// is placed against the pin's own screen rect, and the anchor is not derivable
/// here without the pins the caller already has.
///
/// `true` when the press was consumed — including the press OUTSIDE the panel
/// that closes it. Consuming that one is deliberate: a click that dismisses a
/// floating panel is a click at the panel, not at the canvas underneath it, and
/// letting it through would start a marquee or drop the selection the reviewer
/// was about to comment on.
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

/// Paint the comment pill when the panel is closed, and answer nothing when it
/// is open — the two never share the corner.
pub fn paint_toggle(cx: &mut PaintCx<'_>, state: &EditorState, canvas: Rect) -> Option<Rect> {
    let ui = &state.editor_ui.comments;
    if ui.panel_open {
        return None;
    }
    let rect = comments_panel::CommentsToggle::rect_in_canvas(canvas);
    comments_panel::CommentsToggle::paint(
        cx,
        &theme_for(&state.editor_ui),
        rect,
        ui.open_count(),
        ui.loading,
    );
    Some(rect)
}

/// Route a press against the comment pill. `true` when it opened the panel.
///
/// Opening it also asks for the document's conversation: the daemon pushes no
/// signal for comments, so the list a client holds is only as fresh as its last
/// read, and a panel that opened onto yesterday's review would be worse than one
/// that opened onto a spinner.
pub fn press_toggle(state: &mut EditorState, rect: Option<Rect>, point: Point2D) -> bool {
    let Some(rect) = rect else {
        return false;
    };
    if !comments_panel::CommentsToggle::contains(rect, point) {
        return false;
    }
    state.editor_ui.comments.panel_open = true;
    state.editor_ui.comments.request_reload();
    true
}

/// Build the list panel this frame, if it is open.
pub fn panel_for(state: &EditorState, node_exists: &dyn Fn(&str) -> bool) -> Option<CommentsPanel> {
    let ui = &state.editor_ui.comments;
    if !ui.panel_open {
        return None;
    }
    Some(CommentsPanel::new(
        theme_for(&state.editor_ui),
        state.editor_ui.effective_locale(),
        comments_panel::rows(
            ui,
            state.editor_ui.effective_locale(),
            ui.viewer_id.as_deref(),
            node_exists,
        ),
        ui.pin_mode,
        ui.loading,
        ui.error.clone(),
        state.editor_ui.now_unix_ms,
    ))
}

/// Paint the list panel, if it is open. Returns the rect it painted.
pub fn paint_panel(
    cx: &mut PaintCx<'_>,
    state: &EditorState,
    canvas: Rect,
    node_exists: &dyn Fn(&str) -> bool,
) -> Option<Rect> {
    let panel = panel_for(state, node_exists)?;
    let rect = panel.rect_in_canvas(canvas);
    panel.paint(cx, rect);
    Some(rect)
}

/// Route a press against the list panel.
///
/// `None` when the point is outside it, so the tiers below keep the click —
/// unlike the popover, the panel sits on the edge of the canvas and a click
/// beside it is aimed at the design.
pub fn press_panel(
    state: &mut EditorState,
    rect: Option<Rect>,
    point: Point2D,
    node_exists: &dyn Fn(&str) -> bool,
) -> Option<CommentsPanelAction> {
    let rect = rect?;
    let panel = panel_for(state, node_exists)?;
    match panel.hit_test(rect, point) {
        CommentsPanelHit::Close => {
            state.editor_ui.comments.panel_open = false;
            Some(CommentsPanelAction::Handled)
        }
        CommentsPanelHit::ArmPin => {
            state.editor_ui.comments.toggle_pin_mode();
            Some(CommentsPanelAction::Handled)
        }
        CommentsPanelHit::Row(thread_id) => {
            state.editor_ui.comments.open(thread_id);
            // Select the element the thread is about, exactly as a layer click
            // does, so the property panel and the canvas agree about what the
            // reviewer is looking at.
            let node = state
                .editor_ui
                .comments
                .thread(thread_id)
                .map(|thread| thread.node_id.clone())?;
            if node_exists(&node) {
                state.set_single_selection(NodeId::new(node.clone()));
                Some(CommentsPanelAction::RevealNode(node))
            } else {
                // Nowhere to jump: the element is gone, and the row says so.
                Some(CommentsPanelAction::Handled)
            }
        }
        CommentsPanelHit::Inside => Some(CommentsPanelAction::Handled),
        CommentsPanelHit::Outside => None,
    }
}

#[cfg(test)]
#[path = "comments_flow_tests.rs"]
mod tests;
