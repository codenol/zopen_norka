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
//! - **only for a stored document** — one with a server key. A document with
//!   no key has nowhere to go, and inventing a file for it is a product
//!   decision, not a background task's;
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
}

/// Ask for a frame once the quiet period is over.
fn arm_wake(delay_ms: u64) {
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        WAKE.with(|wake| *wake.borrow_mut() = None);
        crate::repaint_coalescer::request();
    }) as Box<dyn FnMut()>);
    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        delay_ms.min(i32::MAX as u64) as i32,
    );
    WAKE.with(|wake| *wake.borrow_mut() = Some(callback));
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
        state.in_flight == false && settled && spaced
    });
    if !due || !dirty || !editing {
        return;
    }
    STATE.with(|state| state.borrow_mut().attempted_at_ms = now_ms);
    match key {
        // A stored document goes to its own file.
        Some(key) => write(inner, &key),
        // A document with no key has no file, and inventing one would be a
        // file the user never asked for. It goes to the daemon's draft slot
        // instead — the decision recorded in issue #16: drafts live on the
        // server, and are offered back rather than restored silently.
        None => write_draft(inner),
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
        &format!("{base}/api/recovery"),
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
        &format!("{base}/api/files/{key}/autosave"),
        &body,
        on_response_with_status,
    );
    if !started {
        STATE.with(|state| state.borrow_mut().in_flight = false);
    }
}
