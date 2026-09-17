//! Keep the build stamp blinking while it is stale.
//!
//! The web shell paints on events, so a stamp that blinks on its own schedule
//! needs a waker. It is a `setTimeout` loop rather than a rAF loop on purpose:
//! the flip interval is half a second or more, `setTimeout` keeps running
//! (throttled to ~1 Hz) in a background tab, and a rAF-driven loop would both
//! spin at the refresh rate and stop entirely when the tab is hidden.
//!
//! When the build is fresh the loop parks on a slow re-check instead of
//! stopping, so a page left open starts blinking on its own the moment the
//! build crosses the three-minute mark.
//!
//! It lives in this module rather than beside `lib.rs` because it reads a
//! widget's own blink period: the `op_editor_ui::widgets` facade is reachable
//! only from `widget_host` (spec §1.4, `tools/check-widget-boundary.sh`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_ui::widgets::build_stamp;
use wasm_bindgen::JsCast;

use crate::repaint_ctx::RepaintContext;

thread_local! {
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    /// Keeps the pending timeout closure alive until it fires. Replaced on the
    /// next schedule, by which point the previous one has run — `Closure::once`
    /// panicked here with "invoked recursively or after being dropped" because
    /// a forgotten once-closure is already consumed when the timer fires it.
    static PENDING: RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut()>>> =
        const { RefCell::new(None) };
}

/// How long to wait before looking again when nothing is blinking.
const IDLE_RECHECK_MS: i32 = 15_000;

/// How long until the next frame is worth painting.
///
/// While the stamp is blinking this is half a blink — the paint itself decides
/// whether this frame is the visible half, so the pump only has to keep
/// offering frames. A fresh build is checked occasionally instead, so a page
/// left open starts blinking on its own when the build ages past three
/// minutes.
fn frame_interval_ms(now_unix_ms: f64) -> i32 {
    let age = build_stamp::build_age_secs(now_unix_ms);
    if build_stamp::blink_period_ms(build_stamp::freshness(age)).is_some() {
        // Half of the shortest period (1 s) keeps both blink rates honest.
        500
    } else {
        IDLE_RECHECK_MS
    }
}

/// Start the loop unless one is already running.
pub(crate) fn ensure<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if RUNNING.with(Cell::get) {
        return;
    }
    RUNNING.with(|running| running.set(true));
    schedule(inner.clone());
}

fn schedule<C: RepaintContext + 'static>(inner: Rc<RefCell<C>>) {
    // The timeout closure hands its clone on when it fires, which keeps it
    // `FnMut`-compatible for `Closure::wrap`.
    let mut slot = Some(inner);
    let delay = match slot.as_ref().expect("inner").try_borrow_mut() {
        Ok(mut borrowed) => {
            let now = crate::listener::now_ms_perf();
            let unix = crate::listener::now_unix_secs();
            borrowed.host_mut().set_clocks(now, unix);
            let wall_ms = crate::listener::now_unix_ms();
            let interval = frame_interval_ms(wall_ms);
            if interval < IDLE_RECHECK_MS {
                // Blinking: paint this instant, then offer the next frame.
                let _ = borrowed.repaint();
            }
            interval
        }
        // Mid-event with the host borrowed: try again shortly.
        Err(_) => IDLE_RECHECK_MS / 10,
    };

    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if let Some(inner) = slot.take() {
            schedule(inner);
        }
    }) as Box<dyn FnMut()>);
    match web_sys::window() {
        Some(window) => {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                delay,
            );
            // Retain until it fires; the next schedule replaces it.
            PENDING.with(|slot| *slot.borrow_mut() = Some(callback));
        }
        None => {
            RUNNING.with(|running| running.set(false));
        }
    }
}
