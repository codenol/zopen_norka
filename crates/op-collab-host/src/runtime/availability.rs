//! Which availability the runtime publishes for the host driving it.
//!
//! Three inputs decide it, and they answer three different questions:
//!
//! * `op_auth_bridge::collab_ticket_available()` — can this BUILD mint an
//!   admission ticket at all? It is a property of the linked auth artifact, so
//!   it is the same in every host built from one checkout.
//! * `account.is_signed_in()` — does this host already hold a session?
//! * [`CollabHost::answers_collab_sign_in`] — can this DEPLOYMENT turn a
//!   sign-in into a session? Desktop and mobile say yes (the device-login
//!   flow), a `--serve-web` daemon says no: it has no account store, its
//!   `/api/auth/status` reports `available: false`, and every login route is a
//!   404.
//!
//! The first two were the whole computation until #148, and the gap between
//! them is what the issue measured: a local daemon — which links the same
//! ticket ABI as the desktop — published `signInRequired` while its own account
//! tier published `no accounts`, so the collaboration panel offered a sign-in
//! nothing on that deployment could perform. The third input is the host's
//! answer to that, and the reason this decision needs a host at all rather than
//! one more build-time capability check.

use crate::CollabHost;
use op_editor_core::CollabAvailability;

/// The availability to publish for `host`.
pub(super) fn next(host: &impl CollabHost) -> CollabAvailability {
    if !op_auth_bridge::collab_ticket_available() {
        CollabAvailability::Unavailable
    } else if host.editor_state().editor_ui.account.is_signed_in() {
        CollabAvailability::Ready
    } else if host.answers_collab_sign_in() {
        CollabAvailability::SignInRequired
    } else {
        // A sign-in cannot be answered here, so asking for one would name an
        // action nobody can take. `Unavailable` is the honest answer: no
        // session can be started on this deployment, whatever the build links.
        CollabAvailability::Unavailable
    }
}
