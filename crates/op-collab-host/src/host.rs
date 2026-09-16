//! The host surface the collaboration runtime drives, plus a headless
//! implementation for daemons and tests.
//!
//! The runtime reaches the editor through
//! [`CollaborationEditorHost`](op_editor_host_core::collab::CollaborationEditorHost)
//! plus two host-owned concerns: the paint-snapshot dirty flag and the
//! collaboration id-allocation policy. Keeping them behind a trait lets the
//! runtime stay free of any concrete host type — the GUI host implements it
//! over its paint caches, [`HeadlessCollabHost`] over a bare `EditorState`.

use std::sync::Arc;

use op_editor_core::{
    next_sequential_counter, DocumentIdAllocator, DocumentInstallError, DocumentInstallReport,
    EditOrigin, EditorState, IdAllocError, PeerNamespace, PenDocument,
};
use op_editor_host_core::collab::CollaborationEditorHost;

/// First id a standalone (non-collaborating) host hands out, matching the GUI
/// host's `next_node_id` seed.
const FIRST_STANDALONE_NODE_ID: u64 = 100;

/// Wakes whichever event loop drives the runtime, from any thread.
///
/// The runtime never learns what that loop is; the embedding host installs a
/// notifier through [`CollabRuntime::set_wake_notifier`](crate::CollabRuntime::set_wake_notifier).
pub type CollabWakeNotifier = Arc<dyn Fn() + Send + Sync>;

pub trait CollabHost: CollaborationEditorHost {
    /// Mark the paint snapshot stale after a mutation through
    /// `editor_state_mut()`.
    fn mark_editor_state_dirty(&mut self);

    /// Switch creation paths to the owner-assigned namespace.
    fn enable_collaboration_ids(&mut self, namespace: PeerNamespace) -> Result<(), IdAllocError>;

    /// Return to the standalone sequential allocation policy.
    fn disable_collaboration_ids(&mut self);

    /// Whether a sign-in this runtime asks for can actually be answered here.
    ///
    /// [`CollabAvailability::SignInRequired`](op_editor_core::CollabAvailability::SignInRequired)
    /// is a promise that pressing "sign in" does something. The GUI hosts can
    /// keep it — desktop and mobile answer it with the device-login flow — so
    /// the default is `true`.
    ///
    /// A `--serve-web` daemon cannot: it has no account store and serves no
    /// login route, so there the same signal names an action nobody can take.
    /// Such a host answers `false` and the runtime reports the deployment as
    /// unavailable instead. The distinction is per DEPLOYMENT, not per build:
    /// both hosts link the same collaboration-ticket ABI, which is why
    /// `collab_ticket_available()` alone could not tell them apart (#148).
    fn answers_collab_sign_in(&self) -> bool {
        true
    }
}

/// Editor host with no rendering, input, or platform surface.
///
/// Everything the runtime needs and nothing else: the canonical editor state,
/// the collaboration id-allocation slot, and a dirty bit standing in for the
/// GUI host's paint-cache invalidation.
pub struct HeadlessCollabHost {
    editor_state: EditorState,
    collab_id_allocator: Option<DocumentIdAllocator>,
    next_node_id: u64,
    dirty: bool,
}

impl HeadlessCollabHost {
    /// Open on the same single-empty-Frame starter document a fresh GUI host
    /// shows, so document-shaped assertions match across hosts.
    pub fn new() -> Self {
        Self {
            editor_state: EditorState::starter(),
            collab_id_allocator: None,
            next_node_id: FIRST_STANDALONE_NODE_ID,
            dirty: false,
        }
    }

    /// Adopt an already-built editor state (daemon documents, test fixtures).
    pub fn with_editor_state(editor_state: EditorState) -> Self {
        Self {
            editor_state,
            collab_id_allocator: None,
            next_node_id: FIRST_STANDALONE_NODE_ID,
            dirty: false,
        }
    }

    pub fn editor_state(&self) -> &EditorState {
        &self.editor_state
    }

    pub fn editor_state_mut(&mut self) -> &mut EditorState {
        &mut self.editor_state
    }

    /// Whether a runtime mutation has landed since the flag was last taken.
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Read and clear the dirty flag — the headless analogue of a repaint.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Next id the namespaced allocator would hand out, when collaborating.
    pub fn collaboration_id_next_counter(&self) -> Option<u64> {
        self.collab_id_allocator
            .as_ref()
            .map(DocumentIdAllocator::next_counter)
    }

    /// Next id the standalone sequential policy would hand out.
    pub const fn next_node_id(&self) -> u64 {
        self.next_node_id
    }
}

impl Default for HeadlessCollabHost {
    fn default() -> Self {
        Self::new()
    }
}

impl CollaborationEditorHost for HeadlessCollabHost {
    fn editor_state(&self) -> &EditorState {
        &self.editor_state
    }

    fn editor_state_mut(&mut self) -> &mut EditorState {
        &mut self.editor_state
    }

    fn install_collaboration_document(
        &mut self,
        document: PenDocument,
        origin: EditOrigin,
    ) -> Result<DocumentInstallReport, DocumentInstallError> {
        CollaborationEditorHost::install_collaboration_document(
            &mut self.editor_state,
            document,
            origin,
        )
    }
}

impl CollabHost for HeadlessCollabHost {
    fn mark_editor_state_dirty(&mut self) {
        self.dirty = true;
    }

    fn enable_collaboration_ids(&mut self, namespace: PeerNamespace) -> Result<(), IdAllocError> {
        self.collab_id_allocator = Some(DocumentIdAllocator::namespaced_for_document(
            &self.editor_state.doc,
            namespace,
        )?);
        Ok(())
    }

    fn disable_collaboration_ids(&mut self) {
        self.collab_id_allocator = None;
        if let Ok(next) = next_sequential_counter(&self.editor_state.doc) {
            self.next_node_id = self.next_node_id.max(next);
        }
    }
}
