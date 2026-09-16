//! Who is allowed to be told `SignInRequired`.
//!
//! The runtime published `signInRequired` whenever the build could mint a
//! collaboration ticket and the account was anonymous, so a `--serve-web`
//! daemon — which has no account store and no login route — asked its caller to
//! sign in (#148). `CollabHost::answers_collab_sign_in` is the host's half of
//! that decision; these tests pin both answers so neither side can drift back.

use crate::host::HeadlessCollabHost;
use crate::CollabHost;
use op_editor_core::{
    AccountState, CollabAvailability, DocumentInstallError, DocumentInstallReport, EditOrigin,
    EditorState, IdAllocError, PeerNamespace, PenDocument,
};
use op_editor_host_core::collab::CollaborationEditorHost;

use super::CollabRuntime;

/// A host that says it cannot answer a sign-in, on top of the headless one.
///
/// Delegation rather than a second implementation: the subject here is the
/// availability decision, so every other method must behave exactly like the
/// host the rest of the suite drives. This is the shape `DaemonCollabHost`
/// takes in `op-host-services`, reduced to the one method under test.
struct NoSignInHost(HeadlessCollabHost);

impl NoSignInHost {
    fn new() -> Self {
        Self(HeadlessCollabHost::new())
    }
}

impl CollaborationEditorHost for NoSignInHost {
    fn editor_state(&self) -> &EditorState {
        self.0.editor_state()
    }

    fn editor_state_mut(&mut self) -> &mut EditorState {
        self.0.editor_state_mut()
    }

    fn install_collaboration_document(
        &mut self,
        document: PenDocument,
        origin: EditOrigin,
    ) -> Result<DocumentInstallReport, DocumentInstallError> {
        CollaborationEditorHost::install_collaboration_document(&mut self.0, document, origin)
    }
}

impl CollabHost for NoSignInHost {
    fn mark_editor_state_dirty(&mut self) {
        self.0.mark_editor_state_dirty();
    }

    fn enable_collaboration_ids(&mut self, namespace: PeerNamespace) -> Result<(), IdAllocError> {
        self.0.enable_collaboration_ids(namespace)
    }

    fn disable_collaboration_ids(&mut self) {
        self.0.disable_collaboration_ids();
    }

    fn answers_collab_sign_in(&self) -> bool {
        false
    }
}

/// Whether the collaboration-ticket ABI is linked into this build.
///
/// Both outcomes below are reachable only when it is: without it the runtime
/// answers `Unavailable` for a reason of its own and neither test would say
/// anything about the host's answer. Guarding keeps the suite green on a build
/// that ships without the proprietary artifact, instead of asserting on a
/// capability that build does not have.
fn tickets_are_linked() -> bool {
    op_auth_bridge::collab_ticket_available()
}

#[test]
fn a_host_that_cannot_answer_a_sign_in_is_never_asked_for_one() {
    if !tickets_are_linked() {
        return;
    }
    let mut runtime = CollabRuntime::new();
    let mut host = NoSignInHost::new();

    assert!(!runtime.refresh_availability(&mut host));
    assert_eq!(
        host.editor_state().editor_ui.collab.availability,
        CollabAvailability::Unavailable,
        "this deployment has no account store, so a sign-in it asked for would \
         name an action nobody can take — the defect #148 reported"
    );
}

#[test]
fn a_host_that_can_answer_a_sign_in_still_asks_for_one() {
    if !tickets_are_linked() {
        return;
    }
    // The default is the GUI behaviour: desktop and mobile answer this signal
    // with the device-login flow, so the signal must keep its meaning there.
    let mut runtime = CollabRuntime::new();
    let mut host = HeadlessCollabHost::new();
    assert!(host.answers_collab_sign_in());

    assert!(runtime.refresh_availability(&mut host));
    assert_eq!(
        host.editor_state().editor_ui.collab.availability,
        CollabAvailability::SignInRequired
    );
}

#[test]
fn a_signed_in_account_is_ready_whatever_the_host_answers() {
    if !tickets_are_linked() {
        return;
    }
    // The account is consulted before the host, so a deployment that already
    // holds a session never depends on a sign-in route it does not need. This
    // keeps the new branch from swallowing the `Ready` case.
    let mut runtime = CollabRuntime::new();
    let mut host = NoSignInHost::new();
    host.editor_state_mut().editor_ui.account =
        AccountState::signed_in_profile("Ada".to_string(), Some("ada".to_string()));

    runtime.refresh_availability(&mut host);
    assert_eq!(
        host.editor_state().editor_ui.collab.availability,
        CollabAvailability::Ready
    );
}
