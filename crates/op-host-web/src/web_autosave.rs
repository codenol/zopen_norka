//! Autosave: a change reaches disk without a command.
//!
//! The document lives in the daemon's memory and, until this module, reached
//! disk only when the user found the Save command. Close the tab or restart
//! the daemon and the work was gone — the most expensive failure the product
//! can have, and the one with the least warning.
//!
//! The rule is deliberately conservative, because the document is not small
//! (a kit-backed screen is several megabytes):
//!
//! - **after edits settle**, not on every change — a debounce, so a drag does
//!   not write thirty times;
//! - **never more often than a floor interval**, so a long editing session
//!   cannot turn into a disk-and-CPU stampede;
//! - **to the document's own destination** — a stored document to
//!   `/api/files/<key>/autosave`, a document with no server key to the daemon's
//!   draft slot (`/api/recovery`), which is where the issue #16 decision put it.
//!   A document with no key is never written under a key: the key belongs to
//!   the document, and a stale one is how a local file's contents reached a
//!   stored document (issue #92);
//! - **never on the file-browser screen**, where there is nothing being edited.
//!
//! The acknowledgement is the one the manual save already uses
//! (`save_ack_matches_document` + `mark_saved_revision_at`), so a late reply
//! from a previous document cannot mark the current one saved.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;

use crate::file_actions;
use crate::repaint_ctx::RepaintContext;

/// Quiet period after the last change before a write happens.
const DEBOUNCE_MS: u64 = 3_000;

/// Floor between two autosave attempts, however busy the editing is.
const MIN_INTERVAL_MS: u64 = 15_000;

/// Where one autosave goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutosaveTarget {
    /// The stored document this tab is showing.
    Document(String),
    /// The daemon's one draft slot.
    Draft,
}

/// Decide where a document's autosave goes, from its server key and nothing
/// else.
///
/// The key belongs to the DOCUMENT (see
/// `EditorUiState::set_document_key`), so this reads it off the document being
/// edited: a document with a key writes to that document's file, and a document
/// with no key has no file of its own to write to — it goes to the daemon's
/// draft slot, the decision recorded in issue #16 (a draft lives on the server
/// and is offered back, rather than inventing a file the user never asked for).
///
/// The case this deliberately cannot express is the defect of issue #92: a
/// document with no key being written under some OTHER document's key. Making
/// the destination a function of the key alone means the only way to reach a
/// keyed route is to hold that key, which is what "the key belongs to the
/// document" means in code.
pub(crate) fn autosave_target(key: Option<&str>) -> AutosaveTarget {
    match key {
        Some(key) => AutosaveTarget::Document(key.to_string()),
        None => AutosaveTarget::Draft,
    }
}

/// The route one target writes to.
///
/// Pure, so a test can assert which document an autosave will address without a
/// network — or a browser.
pub(crate) fn autosave_url(base: &str, target: &AutosaveTarget) -> String {
    match target {
        AutosaveTarget::Document(key) => format!("{base}/api/files/{key}/autosave"),
        AutosaveTarget::Draft => format!("{base}/api/recovery"),
    }
}

#[derive(Default)]
struct AutosaveState {
    /// `document_revision` seen last time — a change bumps it.
    last_revision: u64,
    /// Monotonic ms when that revision first appeared.
    changed_at_ms: u64,
    /// Monotonic ms of the last attempt (successful or not).
    attempted_at_ms: u64,
    /// A write is in flight; the next one waits for its answer.
    in_flight: bool,
}

thread_local! {
    static STATE: RefCell<AutosaveState> = RefCell::new(AutosaveState::default());
    /// Latched once the state has been initialised from the document, so the
    /// first frame does not treat "no revision seen yet" as a change.
    static SEEDED: Cell<bool> = const { Cell::new(false) };
    /// A wake-up armed for the moment the debounce expires.
    ///
    /// Without it autosave would only ever run while the user happens to be
    /// moving the mouse: the shell paints on events, so a document left alone
    /// after an edit produces no frame, and no frame means no check. The timer
    /// asks for the frame that does the save.
    static WAKE: RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut()>>> =
        const { RefCell::new(None) };
    /// The pending wake-up's timer handle, so re-arming can CANCEL the timer it
    /// replaces before dropping that timer's closure.
    ///
    /// Without this, every re-arm dropped a closure a live `setTimeout` still
    /// held: the timer fired 250 ms later into a slot that no longer existed,
    /// which is the "closure invoked recursively or after being dropped" panic
    /// the console had been reporting (issue #21). Traced there by wrapping the
    /// generated JS shim and reading the Rust frames it printed —
    /// `arm_wake` ← `web_autosave::tick`.
    static WAKE_TIMER: Cell<Option<i32>> = const { Cell::new(None) };
}

/// Ask for a frame once the quiet period is over.
fn arm_wake(delay_ms: u64) {
    let Some(window) = web_sys::window() else {
        return;
    };
    // Cancel the timer being replaced FIRST, then let its closure drop: a live
    // timer holding a dropped closure is exactly the panic this fixes.
    if let Some(handle) = WAKE_TIMER.with(|timer| timer.take()) {
        window.clear_timeout_with_handle(handle);
    }
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        // This wake-up is firing: release the handle, and let the NEXT arm
        // replace the closure rather than dropping it from inside its own call.
        WAKE_TIMER.with(|timer| timer.set(None));
        crate::repaint_coalescer::request();
    }) as Box<dyn FnMut()>);
    let handle = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            delay_ms.min(i32::MAX as u64) as i32,
        )
        .unwrap_or(0);
    WAKE.with(|wake| *wake.borrow_mut() = Some(callback));
    WAKE_TIMER.with(|timer| timer.set((handle != 0).then_some(handle)));
}

/// Called once per frame with the shell in hand.
pub(crate) fn tick<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let now_ms = crate::listener::now_ms_perf();
    let (revision, key, dirty, editing) = {
        let Ok(borrowed) = inner.try_borrow() else {
            return;
        };
        let state = borrowed.host().editor_state();
        (
            state.document_revision(),
            state.editor_ui.file_key.clone(),
            state.is_dirty(),
            state.editor_ui.screen == op_editor_core::AppScreen::Editor,
        )
    };

    if !SEEDED.with(Cell::get) {
        SEEDED.with(|seeded| seeded.set(true));
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.last_revision = revision;
            state.changed_at_ms = now_ms;
        });
        return;
    }

    // Track when the document last changed: a revision we have not seen starts
    // the quiet period over, and arms the wake-up that will outlive this frame.
    let changed = STATE.with(|state| state.borrow().last_revision != revision);
    if changed {
        arm_wake(DEBOUNCE_MS);
    }
    let due = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.last_revision != revision {
            state.last_revision = revision;
            state.changed_at_ms = now_ms;
        }
        let settled = now_ms.saturating_sub(state.changed_at_ms) >= DEBOUNCE_MS;
        let spaced = now_ms.saturating_sub(state.attempted_at_ms) >= MIN_INTERVAL_MS;
        !state.in_flight && settled && spaced
    });
    if !due || !dirty || !editing {
        return;
    }
    STATE.with(|state| state.borrow_mut().attempted_at_ms = now_ms);
    match autosave_target(key.as_deref()) {
        // A stored document goes to its own file.
        AutosaveTarget::Document(key) => write(inner, &key),
        // A document with no key has no file, and inventing one would be a file
        // the user never asked for. It goes to the daemon's draft slot instead —
        // the decision recorded in issue #16: drafts live on the server, and are
        // offered back rather than restored silently.
        AutosaveTarget::Draft => write_draft(inner),
    }
}

/// Write an unbound document into the server's draft slot.
fn write_draft<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let (body, snap_epoch, snap_gen, snap_rev) = {
        let Ok(borrowed) = inner.try_borrow() else {
            return;
        };
        let host = borrowed.host();
        let state = host.editor_state();
        let body =
            file_actions::serialize_save_payload(state, file_actions::SavePayloadTarget::Daemon);
        (
            body,
            host.document_epoch(),
            state.document_generation(),
            state.document_revision(),
        )
    };
    let Ok(body) = body else {
        return;
    };
    STATE.with(|state| state.borrow_mut().in_flight = true);
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, _body| {
        STATE.with(|state| state.borrow_mut().in_flight = false);
        if status != 200 {
            return;
        }
        let Ok(mut borrowed) = inner_for_response.try_borrow_mut() else {
            return;
        };
        if !file_actions::save_ack_matches_document(
            borrowed.host().document_epoch(),
            borrowed.host().editor_state().document_generation(),
            snap_epoch,
            snap_gen,
        ) {
            return;
        }
        // The draft is on disk; the document is as saved as it can be without
        // a file of its own.
        if borrowed
            .host_mut()
            .editor_state_mut()
            .mark_saved_revision_at(snap_gen, snap_rev)
        {
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
    });
    let started = crate::live_sync::post_json_with_status(
        &autosave_url(&base, &AutosaveTarget::Draft),
        &body,
        on_response,
    );
    if !started {
        STATE.with(|state| state.borrow_mut().in_flight = false);
    }
}

/// Send the document to its file, and mark it saved when the daemon agrees.
fn write<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, key: &str) {
    let (body, snap_epoch, snap_gen, snap_rev) = {
        let Ok(borrowed) = inner.try_borrow() else {
            return;
        };
        let host = borrowed.host();
        let state = host.editor_state();
        let body =
            file_actions::serialize_save_payload(state, file_actions::SavePayloadTarget::Daemon);
        (
            body,
            host.document_epoch(),
            state.document_generation(),
            state.document_revision(),
        )
    };
    let Ok(body) = body else {
        return;
    };

    STATE.with(|state| state.borrow_mut().in_flight = true);
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        STATE.with(|state| state.borrow_mut().in_flight = false);
        let Ok(saved) = file_actions::parse_save_response(&response) else {
            return;
        };
        let Ok(mut borrowed) = inner_for_response.try_borrow_mut() else {
            return;
        };
        // The same identity check the manual save uses: a reply that belongs
        // to a document this tab no longer holds must not mark it saved.
        if !file_actions::save_ack_matches_document(
            borrowed.host().document_epoch(),
            borrowed.host().editor_state().document_generation(),
            snap_epoch,
            snap_gen,
        ) {
            return;
        }
        if borrowed
            .host_mut()
            .editor_state_mut()
            .mark_saved_revision_at(snap_gen, snap_rev)
        {
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        let _ = saved;
    });
    // `post_json_with_status`, not `post_json`: the latter sets no timeout, and
    // a reply that never arrives would leave `in_flight` set for the session.
    // The status variant takes `(status, body)`; this reply is only used for
    // its body, and the status is reported through it.
    let on_response_with_status: Rc<dyn Fn(u16, String)> =
        Rc::new(move |_status, body| on_response(body));
    let started = crate::live_sync::post_json_with_status(
        &autosave_url(&base, &AutosaveTarget::Document(key.to_string())),
        &body,
        on_response_with_status,
    );
    if !started {
        STATE.with(|state| state.borrow_mut().in_flight = false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_host::WidgetHost;

    const BASE: &str = "http://127.0.0.1:9";

    /// The route an autosave for `key` takes, as `tick` decides it.
    fn route_of(key: Option<&str>) -> String {
        autosave_url(BASE, &autosave_target(key))
    }

    #[test]
    fn a_stored_document_autosaves_into_its_own_file() {
        assert_eq!(
            route_of(Some("key1")),
            "http://127.0.0.1:9/api/files/key1/autosave"
        );
    }

    #[test]
    fn a_document_with_no_key_never_addresses_a_stored_document() {
        // A document with no home on the server has no `/api/files/<key>/…`
        // route to use: the daemon's draft slot is the only destination, and
        // nothing in this request names a document at all.
        assert_eq!(route_of(None), "http://127.0.0.1:9/api/recovery");
        assert!(!route_of(None).contains("/api/files/"));
    }

    #[test]
    fn a_local_file_installed_over_a_server_document_moves_autosave_off_it() {
        // Issue #92, at the seam the frame reads its decision from: the tab had
        // a stored document open, then installed one it read from the user's
        // disk. Autosave must follow the DOCUMENT, so it must stop naming the
        // document that was replaced — otherwise the local file's contents are
        // written into somebody else's stored document.
        let mut host = WidgetHost::new();
        host.editor_state_mut().editor_ui.file_key = Some("oldkey".to_string());

        host.install_ingested_state(op_editor_core::EditorState::starter());

        let key = host.editor_state().editor_ui.file_key.clone();
        assert_eq!(key, None, "a local file carries no server key");
        assert_eq!(
            route_of(key.as_deref()),
            "http://127.0.0.1:9/api/recovery",
            "autosave must not address the document this tab used to show"
        );
    }
}
