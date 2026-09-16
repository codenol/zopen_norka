//! The per-editor sync controller and the identity pairs every gating
//! decision is keyed on.
//!
//! Split out of `live_sync_glue.rs` at the 800-line cap — pure code motion.
//!
//! It also owns [`sync_facts`], the read-only projection the copy-status
//! surface uses (issues #171 / #191): a fact read here cannot disagree with the
//! gate that acts on it.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use op_editor_core::sync_gate::SyncGate;
use op_editor_core::web_sync::WebSyncClient;

use crate::repaint_ctx::RepaintContext;

use super::ACTIVE_SYNC;

/// Shared sync state for one mounted editor.
///
/// The gate and the client must see the same document identity, so they live
/// in one struct rather than as two independently-owned pieces.
pub(crate) struct SyncController {
    pub gate: SyncGate,
    pub client: WebSyncClient,
    pub push_busy: bool,
    /// A document identity already measured above the periodic push limit.
    /// WASM linear memory does not shrink after a giant temporary JSON string,
    /// so do not rebuild the same oversized snapshot every two seconds.
    pub(super) oversize_identity: Option<(u64, u64, u64)>,
}

impl SyncController {
    pub(crate) fn new() -> Self {
        Self {
            gate: SyncGate::default(),
            client: WebSyncClient::new(),
            push_busy: false,
            oversize_identity: None,
        }
    }
}

pub(crate) type SharedSync = Rc<RefCell<SyncController>>;

/// Commit a successful daemon Save as a sync acknowledgement.
///
/// The daemon has already installed exactly the saved snapshot. Recording its
/// version prevents the next probe from downloading and replacing the same
/// potentially huge document, while the snapshot pair reopens the pull gate.
/// A later local edit still differs from this pair and remains eligible for a
/// normal push (or another explicit Save for oversized documents).
pub(crate) fn acknowledge_daemon_save(
    version: u64,
    generation: u64,
    revision: u64,
    active_page_index: usize,
    preserve_authored_geometry: bool,
) {
    ACTIVE_SYNC.with(|slot| {
        let Some(sync) = slot.borrow().as_ref().and_then(Weak::upgrade) else {
            return;
        };
        let mut sync = sync.borrow_mut();
        sync.client.mark_applied(version);
        sync.client
            .note_applied_snapshot_without_hash(active_page_index, preserve_authored_geometry);
        sync.gate.note_synced(generation, revision);
    });
}

/// Record the live generation/revision as synced without advancing the
/// daemon version baseline. File → New installs a local starter and must
/// not push it (the daemon `/api/file/new` is the kit authority); the next
/// version probe still pulls the library-rich document.
pub(crate) fn acknowledge_current_pair(generation: u64, revision: u64) {
    ACTIVE_SYNC.with(|slot| {
        let Some(sync) = slot.borrow().as_ref().and_then(Weak::upgrade) else {
            return;
        };
        sync.borrow_mut().gate.note_synced(generation, revision);
    });
}

/// Install a controller as the live one, for tests that assert what a reader of
/// [`sync_facts`] sees without starting the sync loops.
#[cfg(test)]
pub(crate) fn install_for_test(sync: SharedSync) {
    ACTIVE_SYNC.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&sync)));
}

/// The document-identity pair every gating decision is keyed on. Read fresh
/// from the live editor state at each decision point — never cached — so an
/// edit that lands between a tick firing and its async response landing is
/// always observed.
pub(super) fn current_pair<C: RepaintContext>(b: &C) -> (u64, u64) {
    let s = b.host().editor_state();
    (s.document_generation(), s.document_revision())
}

pub(super) fn current_oversize_identity<C: RepaintContext>(b: &C) -> (u64, u64, u64) {
    let host = b.host();
    let state = host.editor_state();
    let doc = state;
    (
        host.document_epoch(),
        doc.document_generation(),
        doc.document_revision(),
    )
}

/// What the copy-status surface needs from the shared controller (issues
/// #171 / #191).
///
/// Borrowed, never recomputed: the gate already answers both questions, and a
/// second answer derived from the document itself would be a second source of
/// truth for the one thing this feature exists to state. Default is "nothing
/// known", which is what a tab with no controller yet really has.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SyncFacts {
    /// The daemon version this tab last applied and repainted (`None` = never,
    /// which is the difference between "behind" and "nothing pulled yet").
    pub(crate) applied_version: Option<u64>,
    /// The gate says content local to this tab is not in the daemon's document.
    pub(crate) local_edits: bool,
    /// A push conflict is latched, carrying the daemon's version at that time.
    pub(crate) conflict: Option<u64>,
}

/// Read [`SyncFacts`] from the controller the daemon bootstrap installed.
pub(crate) fn sync_facts<C: RepaintContext>(b: &C) -> SyncFacts {
    let pair = current_pair(b);
    ACTIVE_SYNC.with(|slot| {
        let Some(sync) = slot.borrow().as_ref().and_then(Weak::upgrade) else {
            return SyncFacts::default();
        };
        let Ok(sync) = sync.try_borrow() else {
            // A tick that is mid-mutation must not be observed half-applied:
            // reporting nothing is honest, and the next frame reports again.
            return SyncFacts::default();
        };
        SyncFacts {
            // `initialized` is false until the first apply, and the client's
            // version is 0 before it — reporting 0 would make a tab that has
            // pulled nothing look like a tab showing version 0.
            applied_version: sync
                .client
                .initialized()
                .then(|| sync.client.applied_version()),
            local_edits: sync.gate.needs_push(pair),
            conflict: sync.gate.conflict(),
        }
    })
}
