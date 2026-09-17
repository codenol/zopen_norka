//! The browser half of the copy status: what the canvas is showing, and the
//! copy this browser keeps (issues #171 / #191).
//!
//! [`op_editor_core::editor_ui_state::copy_status`] holds the pure model — the
//! two versions, the standing, the stored record. This module is the only place
//! that *observes* them: it takes the sync controller's own facts, the version
//! probe's answers and the applied version, publishes them into
//! `EditorUiState::document_copy` so the canvas can paint them, and mirrors the
//! document into [`crate::document_store_idb`].
//!
//! ## Why the facts are borrowed and not recomputed
//!
//! Every value here is something the sync stack already knows — the applied
//! version from `WebSyncClient`, `needs_push` and the conflict latch from
//! `SyncGate`. Nothing is inferred, and in particular the daemon's current
//! version comes from the version probe the pull tick already sends; a second
//! probe would be a second source of truth for the one number this whole
//! feature is about.
//!
//! ## Why the write is bounded
//!
//! A stored copy is the whole document (a kit-backed screen is ~3.4 MiB), so
//! the write cadence follows `web_autosave`'s own rules: a change is written
//! after a floor interval, never per frame, one write at a time, and never
//! twice for the same fingerprint. A daemon copy is written the moment it is
//! applied, because that is the one write the whole point depends on — the
//! identity of the copy the canvas is showing.
//!
//! ## Why there is no second store for a second account
//!
//! Records are keyed by `(subject, document)` and the subject is re-read every
//! frame. When it changes, the cached database handle is dropped and the
//! previous account's identity is cleared, so account B can never read account
//! A's copy as its own — the obligation `web_settings_storage` and
//! `live_sync_recovery::clear` already carry.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::editor_ui_state::{
    document_fingerprint, document_record_key, CopyIdentity, CopyOrigin, StoredCopy,
};
use op_editor_core::sync_gate::PERIODIC_PUSH_CAP_BYTES;

use crate::live_sync_glue::{sync_facts, SyncFacts};
use crate::repaint_ctx::RepaintContext;

/// Floor between two local-copy writes, however busy the editing is — the same
/// number `web_autosave` uses for its own floor, for the same reason: a long
/// editing session must not become a disk-and-CPU stampede.
const LOCAL_WRITE_FLOOR_MS: u64 = 15_000;

/// Monotonic milliseconds for the write floor.
///
/// Gated on the browser rather than routed through `listener::now_ms_perf`
/// unconditionally: that helper reaches a `web_sys` imported static, which
/// panics off-wasm, and the floor is the one thing here the host's tests must
/// still be able to run past.
fn monotonic_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        crate::listener::now_ms_perf()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

thread_local! {
    /// A version probe's answer that arrived while the frame was busy.
    ///
    /// The XHR callback can fire mid-event; publishing from there would need a
    /// mutable borrow of a shell that is already borrowed. Parked for the next
    /// frame, exactly as `web_recovery` parks its probe answer.
    static PENDING_DAEMON_VERSION: RefCell<Option<Option<u64>>> = const { RefCell::new(None) };

    /// A record the store read back, waiting for a frame to install it.
    static PENDING_STORED: RefCell<Option<Option<CopyIdentity>>> = const { RefCell::new(None) };

    /// The last probe answer this module was told, so an unchanged answer does
    /// not ask for a frame every 400 ms — while a CHANGED one always does.
    static LAST_NOTED_VERSION: RefCell<Option<Option<u64>>> = const { RefCell::new(None) };

    static STATE: RefCell<StoreState> = const { RefCell::new(StoreState::new()) };

    /// How many times the document has been serialized, for the test that pins
    /// "never serialize merely to find out that no write is due".
    #[cfg(test)]
    static SERIALIZATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

struct StoreState {
    /// Fingerprint of the last record handed to the store, so an unchanged
    /// document is never written twice.
    written_fingerprint: Option<u64>,
    /// Monotonic ms of the last local-copy write attempt.
    attempted_at_ms: u64,
    /// A write is outstanding; the next one waits for its answer.
    in_flight: bool,
    /// The record key the store has been read for, so a document whose key
    /// arrives from the pull is read once and not every frame.
    read_key: Option<String>,
    /// A read is outstanding for `read_key`.
    read_in_flight: bool,
    /// The account partition the state above belongs to.
    subject: Option<String>,
    /// The daemon version last ACCEPTED by the store as the daemon's copy, so a
    /// version that has not moved is not serialized and written again.
    recorded_version: Option<u64>,
    /// The daemon version a write was last ATTEMPTED for. Distinct from
    /// `recorded_version` on purpose: a store that refuses (quota, private
    /// browsing) must not be retried on every frame — a failed write would
    /// otherwise serialize a multi-megabyte document and ask for a repaint at
    /// the frame rate, which is a worse failure than not storing at all.
    attempted_version: Option<u64>,
    /// The shown version seen last frame, so the frame on which it advances is
    /// recognisable (that is the frame the daemon demonstrably holds it).
    seen_shown_version: Option<u64>,
}

impl StoreState {
    const fn new() -> Self {
        Self {
            written_fingerprint: None,
            attempted_at_ms: 0,
            in_flight: false,
            read_key: None,
            read_in_flight: false,
            subject: None,
            recorded_version: None,
            attempted_version: None,
            seen_shown_version: None,
        }
    }
}

/// Record the daemon's version, as a version probe answered it.
///
/// `None` means the probe came back without an answer — the daemon is silent,
/// which is a different fact from "the daemon is at an older version" and must
/// not be collapsed into it.
pub(crate) fn note_daemon_version(version: Option<u64>) {
    PENDING_DAEMON_VERSION.with(|slot| *slot.borrow_mut() = Some(version));
    // The probe runs on its own 400 ms interval, and this module only publishes
    // inside a frame. Parking the answer without asking for that frame leaves the
    // canvas showing the previous standing until something else happens to
    // repaint — measured: with no other activity the strip kept saying "this
    // canvas has edits the daemon has not confirmed" while the daemon had moved
    // on, which is the bug this whole surface exists to report.
    if changed_answer(&LAST_NOTED_VERSION, version) {
        crate::repaint_coalescer::request();
    }
}

/// Whether an answer differs from the last one noted, recording it either way.
///
/// Split out so the "wake only on a change" rule is one place: without it, a
/// steady daemon would be repainted 2.5 times a second forever.
fn changed_answer(
    last: &'static std::thread::LocalKey<RefCell<Option<Option<u64>>>>,
    answer: Option<u64>,
) -> bool {
    last.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == Some(answer) {
            return false;
        }
        *slot = Some(answer);
        true
    })
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    PENDING_DAEMON_VERSION.with(|slot| *slot.borrow_mut() = None);
    PENDING_STORED.with(|slot| *slot.borrow_mut() = None);
    STATE.with(|slot| *slot.borrow_mut() = StoreState::new());
    LAST_NOTED_VERSION.with(|slot| *slot.borrow_mut() = None);
    SERIALIZATIONS.with(|count| count.set(0));
}

/// How many documents this tab has serialized since the last reset. Test-only:
/// the count is the only way to see a cost that has no other symptom.
#[cfg(test)]
pub(crate) fn serializations_for_test() -> u64 {
    SERIALIZATIONS.with(std::cell::Cell::get)
}

/// Read back the record this browser keeps for the document on screen.
///
/// Called once per (subject, document key): the mount cannot know the document
/// key (the tab learns it from the pull's `fileKey`, issue #97), so the first
/// read is the draft slot's record and a keyed document is read the frame its
/// key arrives.
fn ensure_read(subject: &str, doc_key: Option<&str>) {
    let key = document_record_key(subject, doc_key);
    let needs_read = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.read_key.as_deref() == Some(key.as_str()) {
            return false;
        }
        state.read_key = Some(key.clone());
        state.read_in_flight = true;
        true
    });
    if !needs_read {
        return;
    }
    let read_key = key.clone();
    crate::document_store_idb::read(
        &key,
        Box::new(move |json| {
            // A read that landed after the document changed is not an answer
            // about the document on screen; dropping it is the same rule the
            // recovery probe follows.
            let current = STATE.with(|state| state.borrow().read_key.as_deref() == Some(&read_key));
            if !current {
                return;
            }
            STATE.with(|state| state.borrow_mut().read_in_flight = false);
            let identity = json
                .as_deref()
                .and_then(StoredCopy::parse)
                .map(|stored| stored.identity());
            PENDING_STORED.with(|slot| *slot.borrow_mut() = Some(identity));
            // IndexedDB is asynchronous and this runs long after the frame that
            // asked for the read: without a wake, the copy the browser keeps
            // would stay unknown until an unrelated repaint.
            crate::repaint_coalescer::request();
        }),
    );
}

/// The serialized document to keep, or `None` when it is too large to mirror.
///
/// The cap is the sync transport's own periodic-push cap: past it the document
/// already exceeds what the sync channel carries, and holding a second copy of
/// something that size in the wasm heap is the memory growth the recovery stash
/// refuses for the same reason.
fn document_for_store<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) -> Option<String> {
    #[cfg(test)]
    SERIALIZATIONS.with(|count| count.set(count.get() + 1));
    let borrowed = inner.try_borrow().ok()?;
    let state = borrowed.host().editor_state();
    let json = serde_json::to_string(&state.doc).ok()?;
    if json.len() > PERIODIC_PUSH_CAP_BYTES {
        return None;
    }
    Some(json)
}

/// Hand a record to the store, keeping at most one write in flight.
fn write(
    subject: &str,
    doc_key: Option<&str>,
    name: &str,
    origin: CopyOrigin,
    daemon_version: Option<u64>,
    document: String,
    now_ms: u64,
) {
    let key = document_record_key(subject, doc_key);
    let stored = StoredCopy::new(
        doc_key.map(str::to_string),
        name.to_string(),
        origin,
        daemon_version,
        document,
        now_ms,
    );
    let identity = stored.identity();
    let fingerprint = stored.fingerprint;
    let Some(json) = stored.to_json() else {
        return;
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.in_flight = true;
        state.attempted_at_ms = now_ms;
        state.attempted_version = daemon_version;
    });
    crate::document_store_idb::put(
        &key,
        json,
        Box::new(move |ok| {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.in_flight = false;
                // Only a copy the store actually accepted is the copy this
                // browser keeps; claiming it on a failed write would make the
                // status describe a record that is not there.
                if ok {
                    state.written_fingerprint = Some(fingerprint);
                    state.recorded_version = daemon_version;
                    PENDING_STORED.with(|slot| *slot.borrow_mut() = Some(Some(identity)));
                    // The stored identity is part of what the strip says, so the
                    // write landing owes a frame for the same reason the read
                    // does.
                    crate::repaint_coalescer::request();
                }
            });
        }),
    );
}

/// Per-frame work: publish what is known, then keep the browser's copy current.
///
/// Returns whether anything the canvas paints changed, so the caller can ask
/// for the frame that shows it.
pub(crate) fn publish<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) -> bool {
    let now_ms = monotonic_ms();
    let subject = crate::identity_epoch::current_subject();
    // An identity change makes everything the store knew unreachable: drop the
    // cached handle and start again under the new partition, so account B
    // cannot read A's copy as its own.
    let subject_changed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.subject.as_deref() == Some(subject.as_str()) {
            return false;
        }
        *state = StoreState {
            subject: Some(subject.clone()),
            ..StoreState::new()
        };
        true
    });
    if subject_changed {
        crate::document_store_idb::forget_handle();
    }

    // `Some(None)` = a probe came back without an answer this frame.
    let daemon_version = PENDING_DAEMON_VERSION.with(|slot| slot.borrow_mut().take());
    let stored = PENDING_STORED.with(|slot| slot.borrow_mut().take());

    // The document key and the name come from the live state each frame: the
    // pull's `fileKey` decides the key (issue #97), so the record read below
    // follows it rather than a value captured at mount.
    let Ok(borrowed) = inner.try_borrow() else {
        park(daemon_version, stored);
        return false;
    };
    let (doc_key, name, wall_now_ms, facts): (Option<String>, String, u64, SyncFacts) = {
        let state = borrowed.host().editor_state();
        (
            state.editor_ui.file_key.clone(),
            state
                .editor_ui
                .file_name_display
                .clone()
                .unwrap_or_default(),
            // The stamp the record carries is a WALL clock (the chrome's own
            // `now_unix_ms`), because it is read back after a reload and has to
            // mean something to a person: "kept 11 minutes ago".
            state.editor_ui.now_unix_ms.max(0.0) as u64,
            sync_facts(&*borrowed),
        )
    };
    drop(borrowed);

    // The frame the shown version moves is the frame the daemon demonstrably
    // holds it — either it handed this tab that version, or it acknowledged a
    // push with it. That is an observation, not an inference, and it is what
    // keeps a fresh apply from reading as "the daemon is silent" until the next
    // probe lands 400 ms later.
    let shown_advanced = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.seen_shown_version == facts.applied_version {
            return None;
        }
        state.seen_shown_version = facts.applied_version;
        facts.applied_version
    });

    let mut changed = {
        let Ok(mut borrowed) = inner.try_borrow_mut() else {
            park(daemon_version, stored);
            return false;
        };
        let status = &mut borrowed
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .document_copy;
        let before = status.clone();
        status.note_shown_version(facts.applied_version);
        match daemon_version {
            // The probe spoke this frame, and it outranks the inference below:
            // it is the only thing that can report a daemon this tab has not
            // heard from at all.
            Some(answer) => status.note_daemon_version(answer),
            None => {
                if let Some(version) = shown_advanced {
                    status.note_daemon_version(Some(version));
                }
            }
        }
        status.note_local_edits(facts.local_edits);
        status.note_conflict(facts.conflict);
        if let Some(identity) = stored {
            status.note_stored_copy(identity);
        }
        *status != before
    };

    // The key is unknown at mount and arrives with the pull, so the read is
    // (re)issued until the record for THIS document has been asked for.
    ensure_read(&subject, doc_key.as_deref());

    // Keep the browser's copy current. A daemon copy is written the moment it is
    // applied — that write is the one the whole feature rests on, and it is also
    // the only one that can carry a version. A local copy waits out the floor,
    // because it is written from the editing path, and is skipped when its bytes
    // are the ones already on disk.
    //
    // Whether a write is due is decided FIRST, from flags alone. The document is
    // megabytes, and this function runs on every frame: serializing it merely to
    // discover that nothing is due is the kind of cost that shows up as a
    // stutter rather than as a bug, and it is invisible in every test that
    // asserts what was written (a first draft did exactly that — `serializations`
    // below is what makes the mistake fail a test instead of a person).
    let due = STATE.with(|state| {
        let state = state.borrow();
        !state.in_flight && now_ms.saturating_sub(state.attempted_at_ms) >= LOCAL_WRITE_FLOOR_MS
    });
    // A daemon version this tab has neither stored nor tried to store: the write
    // happens on the frame the version moves, which is what makes the stored
    // identity track the daemon instead of trailing it. An attempt that already
    // failed waits for the floor like any other retry.
    let daemon_version_moved = facts.applied_version.filter(|version| {
        STATE.with(|state| {
            let state = state.borrow();
            state.recorded_version != Some(*version) && state.attempted_version != Some(*version)
        })
    });
    let local_candidate = facts.local_edits && due;

    if daemon_version_moved.is_some() || local_candidate {
        if let Some(document) = document_for_store(inner) {
            let fingerprint = document_fingerprint(&document);
            let already_stored =
                STATE.with(|state| state.borrow().written_fingerprint == Some(fingerprint));
            // Origin follows the side that last wrote. A version the daemon
            // attested is its copy; a local edit is this tab's own and carries NO
            // version — inventing one would make the tab claim an agreement with
            // the daemon that it does not have.
            let write_now = match daemon_version_moved {
                Some(version) => Some((CopyOrigin::Daemon, Some(version))),
                None if local_candidate && !already_stored => Some((CopyOrigin::Local, None)),
                None => None,
            };
            if let Some((origin, version)) = write_now {
                write(
                    &subject,
                    doc_key.as_deref(),
                    &name,
                    origin,
                    version,
                    document,
                    wall_now_ms,
                );
                changed = true;
            }
        }
    }

    if changed {
        if let Ok(mut borrowed) = inner.try_borrow_mut() {
            borrowed.host_mut().mark_editor_state_dirty();
        }
    }
    changed
}

/// Put observations back for the next frame. A busy shell cannot be published
/// to, and dropping an observation there is how the status would go quietly
/// blind.
fn park(daemon_version: Option<Option<u64>>, stored: Option<Option<CopyIdentity>>) {
    if let Some(version) = daemon_version {
        PENDING_DAEMON_VERSION.with(|slot| *slot.borrow_mut() = Some(version));
    }
    if let Some(identity) = stored {
        PENDING_STORED.with(|slot| *slot.borrow_mut() = Some(identity));
    }
}

#[cfg(test)]
#[path = "web_copy_status_tests.rs"]
mod tests;
