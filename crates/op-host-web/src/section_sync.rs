//! Read the selected section's properties (#59).
//!
//! The Section block shows values that are NOT in the document — the analytics a
//! section was built from, its summary, its flows live in the daemon's store,
//! because a `.op` file cannot carry them (see `op_editor_core::section`) — so
//! something has to go and ask. This is that something.
//!
//! Three jobs per tick, in this order:
//!
//! 1. watch the selection and tell the panel which section it is looking at;
//! 2. when that section changes, read `GET /api/files/<key>/sections/<node>`;
//! 3. resolve each attached analytics document against `GET /api/analytics/<key>`
//!    so the block can say whether it still hashes to what the link recorded —
//!    the one claim in the block that can quietly become false.
//!
//! ## Why the mockup fingerprint is taken here
//!
//! A link is one claim about two things: the analytics it was made from, and the
//! screens that stood there when it was made. The second half is in the document
//! this tab is holding, so it is computed locally (`mockup_fingerprint`) at the
//! moment the properties arrive; the first half needs the store and comes back
//! over the wire. Both then go to the shared
//! [`link_state`](op_editor_core::section::link_state), so the browser and any
//! other reader of the same link answer by one rule rather than two.
//!
//! ## Why one request at a time
//!
//! A section may reference several analytics documents. They are resolved one
//! per tick, in the order the section lists them: this is a panel somebody is
//! looking at, not a dashboard, and a burst of requests is a burst nobody asked
//! for.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::section::{SectionDigest, SectionProperties};
use op_editor_core::{section, NodeId};

// How a status code becomes a fact about an asset, for both readers (#145).
#[path = "section_sync_answer.rs"]
mod answer;
#[cfg(test)]
#[path = "section_sync_tests.rs"]
mod tests;

#[path = "section_sync_marks.rs"]
mod marks;
#[path = "section_sync_requests.rs"]
mod requests;

use answer::{MarkAnswer, Resolution};
use marks::refresh_marks;
use requests::{attach_requested, resolve_next, save_pending, start_properties};

use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Tick cadence: fast enough that selecting a section feels answered, slow
/// enough that an idle editor costs nothing.
const TICK_MS: i32 = 250;

/// One answer, on its way to the panel.
struct Pending {
    node: NodeId,
    properties: SectionProperties,
    /// The section's screens as they are now: the second half of every link.
    mockups: SectionDigest,
    resolved: Vec<(String, Resolution)>,
}

thread_local! {
    /// The properties that arrived and are waiting for their links.
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    /// The section a properties request is in flight for.
    static IN_FLIGHT: RefCell<Option<NodeId>> = const { RefCell::new(None) };
    /// A write is in flight. One at a time: two saves racing would land in
    /// whichever order the network chose.
    static SAVE_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    /// The document whose sections the canvas marks are about.
    static MARKS_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Sections whose links are being resolved for the canvas marks.
    static MARKS_PENDING: RefCell<Vec<MarkPending>> = const { RefCell::new(Vec::new()) };
    /// Digests already asked for, keyed by asset — a document may reference the
    /// same analytics from several sections.
    static MARK_DIGESTS: RefCell<Vec<(String, MarkAnswer)>> = const { RefCell::new(Vec::new()) };
    /// Whether a list of sections is on its way.
    static MARKS_LIST_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    /// Ticks since the marks were last refreshed. The canvas is told about a
    /// drift every couple of seconds rather than on every frame: the store is
    /// somewhere else, and a document whose sections are in sync should cost
    /// nothing to look at.
    static MARKS_TICKS: Cell<u32> = const { Cell::new(0) };
}

/// How many ticks between two refreshes of the canvas marks.
const MARKS_REFRESH_TICKS: u32 = 8;

/// One section the canvas is waiting to mark.
#[derive(Clone)]
struct MarkPending {
    node: NodeId,
    /// The mockup digest of THIS section's screens, taken when its properties
    /// arrived.
    mockups: SectionDigest,
    /// The links it carries, in order.
    links: Vec<op_editor_core::section::AnalyticsLink>,
}

/// Wire the section reader onto the mounted shell. Called from mount.
pub(crate) fn start<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    let inner = inner.clone();
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        watch_selection(&inner);
        attach_requested(&inner, &base);
        save_pending(&inner, &base);
        resolve_next(&inner, &base);
        refresh_marks(&inner, &base);
    });
    let _ = live_sync::start_interval(TICK_MS, tick);
}

/// The selected node, its document key, and whether it is a section.
fn selection<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
) -> (Option<NodeId>, Option<String>, bool) {
    let Ok(context) = inner.try_borrow() else {
        return (None, None, false);
    };
    let state = context.host().editor_state();
    let is_section = state.selected_node().is_some_and(section::is_section);
    // `NodeId::NONE` is how "nothing is selected" is spelled in the editor.
    let anchor = state.selection.anchor.clone();
    (
        anchor.is_real().then_some(anchor),
        state.editor_ui.file_key.clone(),
        is_section,
    )
}

/// Tell the panel which section it is looking at, and start a read when it
/// changes.
fn watch_selection<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let (anchor, key, is_section) = selection(inner);
    // Only a section has properties; every other selection is "nothing to show"
    // and must clear the block rather than leave the last section's summary on
    // screen under a different node's name.
    let watched = anchor.filter(|_| is_section);
    let (changed, answered) = {
        let Ok(context) = inner.try_borrow() else {
            return;
        };
        let panel = &context.host().editor_state().editor_ui.section_panel;
        (panel.node != watched, panel.read)
    };
    if changed {
        let Ok(mut context) = inner.try_borrow_mut() else {
            return;
        };
        context
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .section_panel
            .select(watched.clone());
        context.host_mut().mark_editor_state_dirty();
        let _ = context.repaint();
    }
    let (Some(node), Some(key)) = (watched, key) else {
        return;
    };
    // Already answered, still being asked, or waiting on its links.
    if answered || IN_FLIGHT.with(|slot| slot.borrow().is_some()) {
        return;
    }
    start_properties(inner, &key, node);
}
