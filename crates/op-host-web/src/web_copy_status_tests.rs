//! Host-side tests for the copy status (issues #171 / #191).
//!
//! What is testable here without a browser: the observations reach the state
//! the canvas paints, and the versions the strip compares are the ones the sync
//! controller actually holds. The IndexedDB round-trip itself is not — that
//! needs a page, and it was driven live instead.
//!
//! The fixtures install a real `SyncController` rather than poking the status
//! directly, so the test covers the seam that broke the first draft of this
//! feature: the shown version must come from the sync client, which advances it
//! on a pull apply AND on an acknowledged push.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::editor_ui_state::{CopyOrigin, CopyStanding, DocumentCopyStatus};
use wasm_bindgen::JsValue;

use super::*;
use crate::live_sync_glue::{install_for_test, SharedSync, SyncController};
use crate::widget_host::WidgetHost;

struct TestContext {
    host: WidgetHost,
}

impl RepaintContext for TestContext {
    fn host(&self) -> &WidgetHost {
        &self.host
    }

    fn host_mut(&mut self) -> &mut WidgetHost {
        &mut self.host
    }

    fn viewport_size(&self) -> (f32, f32) {
        (1440.0, 900.0)
    }

    fn register_system_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font_from_bytes(&mut self, _bytes: &[u8]) -> Option<String> {
        None
    }

    fn imported_family_list(&self) -> Vec<String> {
        Vec::new()
    }

    fn remove_imported_font(&mut self, _family: &str) {}

    fn repaint(&mut self) -> Result<(), JsValue> {
        Ok(())
    }
}

fn context() -> Rc<RefCell<TestContext>> {
    reset_for_test();
    Rc::new(RefCell::new(TestContext {
        host: WidgetHost::new(),
    }))
}

/// The live document-identity pair, as `sync_facts` reads it.
fn pair(inner: &Rc<RefCell<TestContext>>) -> (u64, u64) {
    let borrowed = inner.borrow();
    let state = borrowed.host.editor_state();
    (state.document_generation(), state.document_revision())
}

/// Install a controller standing in for the live sync stack.
///
/// The controller is registered WEAKLY, exactly as the mount registers the real
/// one, so a test that drops this handle stops being observed — which is the
/// production behaviour too (a tab whose controller is gone has nothing to
/// report, and `sync_facts` answers "nothing known"). Bind the result.
///
/// `applied` is the daemon version this tab's content corresponds to — the same
/// field a pull apply and an acknowledged push both advance. `local_edits`
/// baselines the gate one revision behind the live pair, which is exactly what
/// the gate reads as "this tab holds unpushed content".
fn install(
    inner: &Rc<RefCell<TestContext>>,
    applied: Option<u64>,
    local_edits: bool,
) -> SharedSync {
    let (generation, revision) = pair(inner);
    let sync: SharedSync = Rc::new(RefCell::new(SyncController::new()));
    {
        let mut sync = sync.borrow_mut();
        if let Some(version) = applied {
            sync.client.mark_applied(version);
        }
        sync.gate.note_synced(
            generation,
            if local_edits { revision + 1 } else { revision },
        );
    }
    install_for_test(sync.clone());
    sync
}

fn status(inner: &Rc<RefCell<TestContext>>) -> DocumentCopyStatus {
    inner
        .borrow()
        .host
        .editor_state()
        .editor_ui
        .document_copy
        .clone()
}

#[test]
fn the_shown_version_comes_from_the_sync_clients_own_answer() {
    // A pull apply and an acknowledged push both land here. Deriving the shown
    // version from the pull path alone left it one version behind after every
    // successful push — the strip then reported a divergence that did not
    // exist, which is the one failure mode worse than saying nothing.
    let inner = context();
    let sync = install(&inner, Some(12), false);

    publish(&inner);
    assert_eq!(status(&inner).shown_version, Some(12));
    assert_eq!(status(&inner).standing(), CopyStanding::InStep);

    // The daemon acknowledged a push at 13: same field, and no frame in which
    // the canvas is reported as behind.
    sync.borrow_mut().client.mark_applied(13);
    publish(&inner);

    assert_eq!(status(&inner).shown_version, Some(13));
    assert_eq!(status(&inner).daemon_version, Some(13));
    assert_eq!(status(&inner).standing(), CopyStanding::InStep);
}

#[test]
fn a_probe_answer_becomes_the_daemon_version_and_the_standing() {
    // The whole #191 scenario at this layer: the canvas holds v12, the probe
    // says the daemon is at v14, and the published status says "behind" — which
    // is what the strip paints, and what stops a turn's result from being
    // invisible.
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    publish(&inner);
    assert_eq!(status(&inner).standing(), CopyStanding::InStep);

    note_daemon_version(Some(14));
    publish(&inner);

    assert_eq!(
        status(&inner).standing(),
        CopyStanding::Behind {
            shown: 12,
            daemon: 14
        }
    );
}

#[test]
fn an_unanswered_probe_publishes_silence_not_agreement() {
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    publish(&inner);

    note_daemon_version(None);
    publish(&inner);

    assert_eq!(
        status(&inner).standing(),
        CopyStanding::DaemonSilent { shown: 12 },
        "a probe that said nothing must not become a claim that the canvas is current"
    );
}

#[test]
fn unpushed_local_edits_reach_the_published_status() {
    // The gate's own answer, not a guess from the document: a tab holding
    // unpushed content is painting its own copy, whatever the versions say.
    let inner = context();
    let _sync = install(&inner, Some(12), true);
    note_daemon_version(Some(12));
    publish(&inner);

    assert_eq!(
        status(&inner).standing(),
        CopyStanding::LocalEdits { shown: 12 }
    );
}

#[test]
fn a_latched_conflict_reaches_the_published_status_and_clears_with_it() {
    let inner = context();
    let sync = install(&inner, Some(12), true);
    note_daemon_version(Some(12));
    publish(&inner);
    assert_eq!(
        status(&inner).standing(),
        CopyStanding::LocalEdits { shown: 12 },
        "no conflict latched yet"
    );

    // The latch, which the strip reports in preference to either version: a
    // closed pull gate explains why nothing newer can arrive.
    sync.borrow_mut().gate.note_conflict(14);
    publish(&inner);
    assert_eq!(
        status(&inner).standing(),
        CopyStanding::ConflictLatched {
            shown: 12,
            daemon: 14
        }
    );

    // The latch lifting must be published too: a status that only ever latches
    // on is a status that stays wrong after the situation changes.
    let (generation, revision) = pair(&inner);
    sync.borrow_mut().gate.note_synced(generation, revision);
    note_daemon_version(Some(12));
    publish(&inner);
    assert!(!status(&inner).standing().is_divergence());
}

#[test]
fn the_published_standing_settles_again_when_the_daemon_is_caught_up_with() {
    let inner = context();
    let sync = install(&inner, Some(12), false);
    publish(&inner);
    note_daemon_version(Some(14));
    publish(&inner);
    assert!(!status(&inner).canvas_is_daemons_copy());

    // The pull applied v14.
    sync.borrow_mut().client.mark_applied(14);
    publish(&inner);

    assert_eq!(status(&inner).standing(), CopyStanding::InStep);
    assert!(status(&inner).canvas_is_daemons_copy());
}

#[test]
fn publishing_reports_whether_the_painted_status_changed() {
    // The status is published after the paint, so the caller must be told when
    // the next frame is owed — otherwise the divergence is stated in state and
    // never on screen.
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    assert!(publish(&inner), "the first observation changes the status");

    // The store's answer about which copy this browser keeps arrives a frame
    // later (IndexedDB is asynchronous), and that landing is itself a change
    // the canvas owes a repaint. Drain it, then the status must be stable.
    for _ in 0..4 {
        publish(&inner);
    }
    assert!(
        !publish(&inner),
        "an unchanged set of observations must not ask for a frame every tick"
    );

    note_daemon_version(Some(14));
    assert!(publish(&inner), "a newer daemon version changes it again");
}

#[test]
fn an_observation_that_arrives_while_the_shell_is_borrowed_is_not_lost() {
    // The probe's XHR callback can fire mid-event. Dropping the answer there
    // would leave the status describing a daemon version that has moved on.
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    publish(&inner);

    note_daemon_version(Some(14));
    let held = inner.borrow_mut();
    assert!(!publish(&inner), "a borrowed shell cannot be published to");
    drop(held);

    publish(&inner);
    assert_eq!(
        status(&inner).standing(),
        CopyStanding::Behind {
            shown: 12,
            daemon: 14
        },
        "the parked answer must be published on the next frame"
    );
}

#[test]
fn a_store_that_refuses_a_write_is_not_retried_every_frame() {
    // The host harness has no IndexedDB, so every write here is refused —
    // exactly like a browser in private mode or one over quota. Without the
    // attempt latch, each frame would re-serialize a multi-megabyte document
    // and ask for a repaint: a worse failure than not storing at all.
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    publish(&inner);

    assert_eq!(
        STATE.with(|state| state.borrow().attempted_version),
        Some(12),
        "the daemon's version is written on the frame it moves"
    );
    for _ in 0..5 {
        publish(&inner);
    }

    assert!(
        STATE.with(|state| state.borrow().recorded_version.is_none()),
        "nothing was accepted, so nothing is recorded"
    );
    assert_eq!(
        STATE.with(|state| state.borrow().attempted_version),
        Some(12),
        "and the same version is not attempted again"
    );
}

#[test]
fn a_stored_record_is_published_as_the_copy_this_browser_keeps() {
    let inner = context();
    let _sync = install(&inner, Some(12), false);
    publish(&inner);

    PENDING_STORED.with(|slot| {
        *slot.borrow_mut() = Some(Some(CopyIdentity {
            doc_key: Some("key-1".to_string()),
            daemon_version: Some(11),
            fingerprint: 42,
            saved_at_ms: 5_000,
            origin: CopyOrigin::Daemon,
        }))
    });
    publish(&inner);

    assert_eq!(
        status(&inner).stored_daemon_version(),
        Some(11),
        "the store's own answer about which copy it holds must reach the state"
    );
}
