//! Coalesces repaint requests to a single paint per animation frame.
//!
//! The CanvasKit mount paints the entire editor chrome each frame. Firing that
//! synchronously from every DOM event (mousemove / wheel / keydown) repaints the
//! whole shell once *per input event* — two moves in one display frame paint
//! twice, both blocking the JS main thread. This routes every repaint through a
//! single `requestAnimationFrame`: many `request()` calls within a frame
//! collapse to one paint, capping repaints at the display refresh rate. The
//! desktop host already coalesces the same way (its `redraw_pending` flag +
//! `prepare_redraw`); this brings the web host to parity.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

thread_local! {
    /// Per-thread (wasm is single-threaded) coalescer for the active mount.
    static COALESCER: RefCell<Option<Coalescer>> = const { RefCell::new(None) };
}

struct Coalescer {
    /// Paints the active mount. Borrows the shell internally; it reschedules
    /// itself if the shell is momentarily borrowed when the frame fires.
    paint: Rc<dyn Fn()>,
    /// A frame is already requested — further `request()` calls are no-ops until
    /// it fires.
    scheduled: bool,
    /// Keeps the in-flight rAF closure alive until it fires (`requestAnimationFrame`
    /// does not take ownership). Replaced on the next schedule, by which point
    /// the previous closure has already run.
    frame: Option<Closure<dyn FnMut()>>,
}

/// Install the paint callback for the active mount. Call once per mount, after
/// the shell `Rc<RefCell<_>>` is built. Replaces any previous installation.
pub(crate) fn install(paint: Rc<dyn Fn()>) {
    COALESCER.with(|c| {
        *c.borrow_mut() = Some(Coalescer {
            paint,
            scheduled: false,
            frame: None,
        });
    });
}

/// Request a repaint on the next animation frame. Borrow-free with respect to
/// the shell (it only touches this module's thread-local flag), so it is safe to
/// call from inside an existing `borrow_mut` scope — the actual paint runs later,
/// once the handler's borrow has been released. No-op when no mount is installed
/// (e.g. the non-browser test path, where `web_sys::window()` is `None`).
pub(crate) fn request() {
    let schedule = COALESCER.with(|c| {
        let mut slot = c.borrow_mut();
        match slot.as_mut() {
            Some(co) if !co.scheduled => {
                co.scheduled = true;
                true
            }
            _ => false,
        }
    });
    if schedule {
        schedule_frame();
    }
}

fn schedule_frame() {
    let Some(window) = web_sys::window() else {
        // No browser scheduler (test / worker path). Undo the flag so a later
        // browser-context request can still arm; paint is a no-op without a
        // window anyway.
        COALESCER.with(|c| {
            if let Some(co) = c.borrow_mut().as_mut() {
                co.scheduled = false;
            }
        });
        return;
    };

    let cb = Closure::wrap(Box::new(move || {
        // Clear the flag and clone out the paint fn WITHOUT holding the borrow
        // across the paint: the paint fn borrows the shell and, on a momentary
        // borrow conflict, calls `request()` again (which re-borrows this slot).
        let paint = COALESCER.with(|c| {
            let mut slot = c.borrow_mut();
            slot.as_mut().map(|co| {
                co.scheduled = false;
                co.paint.clone()
            })
        });
        if let Some(paint) = paint {
            paint();
        }
        // Mark the frame delivered so the fallback tick knows rAF ran.
        COALESCER.with(|c| {
            if let Some(co) = c.borrow_mut().as_mut() {
                co.frame = None;
            }
        });
    }) as Box<dyn FnMut()>);

    // Fallback tick. A background or occluded tab throttles — and can stop —
    // `requestAnimationFrame`, so a state change that arrived from the daemon
    // (an AI turn finishing, a peer's edit) would sit unpainted until the user
    // reloads the page. That is exactly the "empty canvas until refresh"
    // report. `setTimeout` keeps running at ~1 Hz when throttled, so it paints
    // the frame rAF owes us.
    let fallback = Closure::wrap(Box::new(move || {
        // `frame` is still armed exactly when rAF never fired — the throttled
        // case this tick exists for. When rAF did paint, it cleared `frame`
        // and this is a no-op instead of a second whole-chrome frame.
        let paint = COALESCER.with(|c| {
            let mut slot = c.borrow_mut();
            slot.as_mut().and_then(|co| {
                if co.frame.is_none() {
                    return None;
                }
                co.frame = None;
                co.scheduled = false;
                Some(co.paint.clone())
            })
        });
        if let Some(paint) = paint {
            paint();
        }
    }) as Box<dyn FnMut()>);
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        fallback.as_ref().unchecked_ref(),
        250,
    );

    match window.request_animation_frame(cb.as_ref().unchecked_ref()) {
        Ok(_) => {
            // Retain the closure until it fires; this drops the prior frame's
            // closure, which has already executed.
            COALESCER.with(|c| {
                if let Some(co) = c.borrow_mut().as_mut() {
                    co.frame = Some(cb);
                }
            });
        }
        Err(_) => {
            // The browser rejected the rAF request. Reset `scheduled` so a later
            // request can re-arm — otherwise the flag stays stuck `true` and the
            // coalescer deadlocks, never painting again. `cb` drops here (it was
            // never registered, so there is no dangling browser callback).
            COALESCER.with(|c| {
                if let Some(co) = c.borrow_mut().as_mut() {
                    co.scheduled = false;
                }
            });
        }
    }
}
