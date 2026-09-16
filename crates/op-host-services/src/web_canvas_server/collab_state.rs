//! Collaboration runtime attached to the daemon's single editor.
//!
//! The daemon is a native process, so unlike the browser it can hold the
//! device credentials a relay session needs. It runs the same
//! `op_collab_host::CollabRuntime` the desktop GUI runs; the browser only
//! paints the projection and posts actions.
//!
//! ## The split borrow
//!
//! The runtime wants `&mut impl CollabHost`, and a host is an editor plus an
//! id-allocation slot plus a dirty flag. `HeadlessCollabHost` *owns* its
//! editor, which is right for a daemon that starts empty but wrong here: this
//! daemon's editor already lives on [`WebCanvasState`] next to the version
//! counter, the backing path, and the credential policy. So the host is a
//! borrowing view assembled per call — [`DaemonCollabHost`] — with exactly the
//! semantics of `HeadlessCollabHost`.

use op_collab_host::{CollabHost, CollabRuntime};
use op_editor_core::{
    next_sequential_counter, CollabEditSource, CollabGateAction, CollabGatePolicy,
    CollabGateReason, DocumentIdAllocator, DocumentInstallError, DocumentInstallReport, EditOrigin,
    EditorState, IdAllocError, PeerNamespace, PenDocument, PreparedDocument,
};
use op_editor_host_core::collab::CollaborationEditorHost;

use super::online_policy::ServeMode;
use super::WebCanvasState;

/// First id a standalone daemon hands out, matching the GUI host's seed.
const FIRST_STANDALONE_NODE_ID: u64 = 100;

/// Everything the daemon keeps for collaboration, beside the editor itself.
pub(crate) struct WebCollabState {
    /// The owner/guest actors and their network workers.
    pub(crate) runtime: CollabRuntime,
    /// Owner-assigned id namespace while a session is live.
    id_allocator: Option<DocumentIdAllocator>,
    /// Next id under the standalone sequential policy.
    next_node_id: u64,
    /// Raised by any runtime write through the host seam.
    dirty: bool,
    /// Bumps whenever the *projection* changes — a peer joining, a cursor
    /// moving, a notice appearing. Deliberately separate from the document
    /// version so presence traffic never makes a browser refetch the document.
    seq: u64,
    /// Last cursor the browser published, replayed to peers by the driver.
    presence_override: Option<(f64, f64)>,
    /// Pokes the driver thread awake. `None` until the driver starts, and in
    /// the unit tests, which tick the runtime directly.
    waker: Option<std::sync::Arc<super::collab_driver::CollabWaker>>,
}

impl Default for WebCollabState {
    fn default() -> Self {
        Self {
            runtime: CollabRuntime::new(),
            id_allocator: None,
            next_node_id: FIRST_STANDALONE_NODE_ID,
            dirty: false,
            seq: 0,
            presence_override: None,
            waker: None,
        }
    }
}

impl WebCollabState {
    /// Current projection sequence number.
    pub(crate) const fn seq(&self) -> u64 {
        self.seq
    }

    /// Advance the projection sequence number.
    pub(crate) fn bump_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// Read and clear the host dirty flag.
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Record the browser's latest cursor for the next presence flush.
    pub(crate) fn set_presence_override(&mut self, cursor: Option<(f64, f64)>) {
        self.presence_override = cursor;
    }

    pub(crate) const fn presence_override(&self) -> Option<(f64, f64)> {
        self.presence_override
    }

    /// Register the driver's wake path.
    pub(crate) fn set_waker(&mut self, waker: std::sync::Arc<super::collab_driver::CollabWaker>) {
        self.waker = Some(waker);
    }

    /// Ask the driver to tick now instead of at its next timeout.
    pub(crate) fn wake_driver(&self) {
        if let Some(waker) = self.waker.as_ref() {
            waker.wake();
        }
    }
}

/// A borrowing [`CollabHost`] over the daemon's editor.
///
/// Assembled per call so the editor can stay where the rest of the daemon
/// already reads it. Every method matches `HeadlessCollabHost`.
pub(crate) struct DaemonCollabHost<'a> {
    editor: &'a mut EditorState,
    id_allocator: &'a mut Option<DocumentIdAllocator>,
    next_node_id: &'a mut u64,
    dirty: &'a mut bool,
    /// How this daemon is deployed, copied out of the state so the host can
    /// answer [`CollabHost::answers_collab_sign_in`]. See that method for why a
    /// daemon has to answer it at all.
    mode: ServeMode,
}

impl CollaborationEditorHost for DaemonCollabHost<'_> {
    fn editor_state(&self) -> &EditorState {
        self.editor
    }

    fn editor_state_mut(&mut self) -> &mut EditorState {
        self.editor
    }

    fn install_collaboration_document(
        &mut self,
        document: PenDocument,
        origin: EditOrigin,
    ) -> Result<DocumentInstallReport, DocumentInstallError> {
        CollaborationEditorHost::install_collaboration_document(self.editor, document, origin)
    }
}

impl CollabHost for DaemonCollabHost<'_> {
    fn mark_editor_state_dirty(&mut self) {
        *self.dirty = true;
    }

    fn enable_collaboration_ids(&mut self, namespace: PeerNamespace) -> Result<(), IdAllocError> {
        *self.id_allocator = Some(DocumentIdAllocator::namespaced_for_document(
            &self.editor.doc,
            namespace,
        )?);
        Ok(())
    }

    fn disable_collaboration_ids(&mut self) {
        *self.id_allocator = None;
        if let Ok(next) = next_sequential_counter(&self.editor.doc) {
            *self.next_node_id = (*self.next_node_id).max(next);
        }
    }

    /// Only the online deployment has an account store.
    ///
    /// The local and managed daemons answer `/api/auth/status` with
    /// `available: false` and 404 every login route (see the account comment in
    /// `handle_web_canvas_request`), so a sign-in they asked for could not be
    /// performed by anyone (#148). Online is the one mode where an account
    /// exists to sign into — and it runs no collaboration driver, so this is a
    /// statement about the deployment rather than a path taken at runtime.
    fn answers_collab_sign_in(&self) -> bool {
        self.mode.is_online()
    }
}

/// What a session did with a whole-document push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    /// Installed and sequenced. The only outcome that publishes a version.
    Committed,
    /// Accepted, but the document was identical — no transaction, no version.
    NoChange,
    /// No capture could be opened (busy, or this peer is read-only).
    Rejected,
    /// The capture opened but the session would not commit it. The browser's
    /// copy is now definitively behind and must be refetched.
    Failed,
}

impl IngestOutcome {
    /// Stable machine-readable code, for the outcomes that are errors.
    pub const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Committed | Self::NoChange => None,
            // Both share the existing conflict code so the browser's
            // established refetch path handles them without a new client
            // branch — `parse_push_conflict` only recognises this one.
            Self::Rejected | Self::Failed => Some("version-conflict"),
        }
    }
}

/// RAII close for a session's local-edit capture.
///
/// `begin_local_edit` and `finish_local_edit` must pair on every path. Holding
/// the pair in a guard means an early return — or a panic in the install —
/// cannot leave the capture open and the session wedged.
struct LocalEditCapture<'a> {
    state: Option<&'a mut WebCanvasState>,
}

impl LocalEditCapture<'_> {
    fn state_mut(&mut self) -> &mut WebCanvasState {
        self.state.as_mut().expect("capture is open")
    }

    /// Close the capture, reporting what the session decided.
    fn finish(mut self) -> op_collab_host::LocalEditOutcome {
        let Some(state) = self.state.take() else {
            // The guard lost its state: nothing was rolled back here.
            return op_collab_host::LocalEditOutcome::Failed {
                document_rolled_back: false,
            };
        };
        let (runtime, mut host) = state.collab_runtime_and_host();
        runtime.finish_local_edit(&mut host)
    }
}

impl Drop for LocalEditCapture<'_> {
    fn drop(&mut self) {
        // Only reached when `finish` was not called — an unwind. Close the
        // capture anyway; its result is unobservable on this path, but a
        // session left mid-edit would refuse every later push.
        if let Some(state) = self.state.take() {
            let (runtime, mut host) = state.collab_runtime_and_host();
            let _ = runtime.finish_local_edit(&mut host);
        }
    }
}

/// Why the daemon refused a mutation.
///
/// The bool acks the older applier paths returned could not say *why* a write
/// was dropped, so an MCP or AI caller saw a silent no-op during an active
/// session. Every gateway rejection is now a typed reason the route can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonMutationRefusal {
    /// The collaboration policy refused this source in this phase.
    Collab(CollabGateReason),
}

impl DaemonMutationRefusal {
    /// Stable machine-readable code for a REST body or JSON-RPC error.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Collab(reason) => match reason {
                CollabGateReason::ReadOnly | CollabGateReason::OwnerOnlySave => "collab-readonly",
                CollabGateReason::PendingEdit | CollabGateReason::SessionTransition => {
                    "collab-busy"
                }
                _ => "collab-active",
            },
        }
    }

    /// HTTP status the code maps to. Every refusal is a conflict with the live
    /// session rather than a malformed request.
    pub const fn http_status(self) -> &'static str {
        "409 Conflict"
    }
}

impl std::fmt::Display for DaemonMutationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Collab(reason) => write!(
                f,
                "refused by the live collaboration session ({})",
                reason.i18n_key()
            ),
        }
    }
}

impl std::error::Error for DaemonMutationRefusal {}

impl WebCanvasState {
    /// Borrow the runtime and a host over the same editor.
    ///
    /// One method rather than two accessors because the borrow checker has to
    /// see both borrows split out of `self` at once.
    pub(crate) fn collab_runtime_and_host(&mut self) -> (&mut CollabRuntime, DaemonCollabHost<'_>) {
        // Read the mode before the split: it is `Copy`, and the host needs it
        // while `self` is mutably borrowed into its fields.
        let mode = self.mode;
        let collab = &mut self.collab;
        (
            &mut collab.runtime,
            DaemonCollabHost {
                editor: &mut self.editor,
                id_allocator: &mut collab.id_allocator,
                next_node_id: &mut collab.next_node_id,
                dirty: &mut collab.dirty,
                mode,
            },
        )
    }

    /// The collaboration policy derived from the live projection.
    pub(crate) fn collab_policy(&self) -> CollabGatePolicy {
        CollabGatePolicy::from(&self.editor.editor_ui.collab)
    }

    /// The single gate every non-user daemon mutation passes through.
    ///
    /// MCP writes, AI writes, and bulk imports all reach the same editor from
    /// different routes. Desktop refuses those while a session is active — the
    /// M1 protocol has no way to sequence a write the local user did not make —
    /// and this keeps the daemon's answer identical instead of letting a route
    /// that predates collaboration fork the document.
    ///
    /// Idle and Discovering phases pass through untouched, so a daemon with no
    /// session behaves exactly as it did before collaboration existed.
    pub(crate) fn gate_daemon_mutation(
        &self,
        action: CollabGateAction,
        source: CollabEditSource,
    ) -> Result<(), DaemonMutationRefusal> {
        self.collab_policy()
            .check(action, source)
            .map_err(DaemonMutationRefusal::Collab)
    }

    /// Apply one editor command through the gateway.
    ///
    /// The bool is the editor's own "did this command do anything" ack, kept
    /// so the applier contract is unchanged; a refusal is the `Err`, which is
    /// what makes a rejected write distinguishable from a no-op write.
    pub(crate) fn apply_gated(
        &mut self,
        command: op_editor_core::EditorCommand,
        source: CollabEditSource,
    ) -> Result<bool, DaemonMutationRefusal> {
        self.collab_policy()
            .check_command(&command, source)
            .map_err(DaemonMutationRefusal::Collab)?;
        Ok(self.editor.apply(command))
    }

    /// Whether a live session currently sequences this document.
    pub(crate) fn collab_session_is_active(&self) -> bool {
        self.editor.editor_ui.collab.phase == op_editor_core::CollabConnectionPhase::Active
    }

    /// Install a validated document while a session is live.
    ///
    /// The whole point is that the swap happens *inside* the runtime's
    /// local-edit capture: the session core diffs the document before and after
    /// and emits a precise node transaction, so an unchanged push produces no
    /// transaction at all. [`EditorState::install_prepared_document`] keeps the
    /// document generation, which is what lets the matching finish succeed.
    ///
    /// Reports what the session did with the push — see [`IngestOutcome`].
    /// The previous `bool` could not distinguish "the session accepted this"
    /// from "the session threw it away", so a rejected push was answered 200
    /// with a version bump and the browser never learned to resync.
    pub(crate) fn ingest_document_in_session(
        &mut self,
        prepared: PreparedDocument,
    ) -> IngestOutcome {
        let (runtime, mut host) = self.collab_runtime_and_host();
        if !runtime.begin_local_edit(&mut host) {
            // The document is about to be dropped without ever activating, so
            // release its pending thumbnail seed — otherwise it lingers in the
            // process-global side table until bounded eviction pushes it out.
            jian_ops_schema::image_thumbs::discard_for_document(prepared.document());
            return IngestOutcome::Rejected;
        }
        // Captured before the install activates the incoming document's
        // thumbnails. If the session then REJECTS the edit, rolling the
        // document back does not roll these back: the previous document's
        // pending seed was consumed by its own activation, so re-activating it
        // is a no-op and the refused document's thumbnails keep resolving live
        // ids. Restoring the snapshot on the rejection path is what undoes it.
        let thumbnails_before = jian_ops_schema::image_thumbs::capture_snapshot();
        // The capture is now open and MUST be closed on every path. The guard
        // is what makes that true even if the install below panics: an
        // unclosed capture leaves the session permanently unable to accept
        // another edit, which is worse than the failed push itself.
        let mut capture = LocalEditCapture { state: Some(self) };
        capture
            .state_mut()
            .editor
            .install_prepared_document(prepared, EditOrigin::Local);
        // Straight from the session's own resolution. The previous revision
        // heuristic could not see a REJECTION: the session rolls the edit back,
        // so the revision is unchanged and the push read as `NoChange` — a
        // 200 for a write that had been discarded.
        match capture.finish() {
            op_collab_host::LocalEditOutcome::Committed => IngestOutcome::Committed,
            op_collab_host::LocalEditOutcome::NoChange => IngestOutcome::NoChange,
            // The document was rolled back, so the thumbnails must roll back
            // with it — otherwise the refused document's images stay active.
            op_collab_host::LocalEditOutcome::Rejected => {
                jian_ops_schema::image_thumbs::restore_snapshot(thumbnails_before);
                IngestOutcome::Rejected
            }
            // Only restore when the document actually went back. The
            // standalone fallback KEEPS the edit, so restoring there would
            // leave the thumbnails describing a document that no longer
            // exists — the exact desynchronisation this guard exists for.
            op_collab_host::LocalEditOutcome::Failed {
                document_rolled_back,
            } => {
                if document_rolled_back {
                    jian_ops_schema::image_thumbs::restore_snapshot(thumbnails_before);
                }
                IngestOutcome::Failed
            }
        }
    }
}

#[cfg(test)]
#[path = "collab_state_tests.rs"]
mod tests;
