//! The address bar as editor state.
//!
//! Which document is open, which page, and which node is selected all belong
//! in the URL — that is what makes a screen shareable, and it is what Figma
//! does (`/file/<key>/<slug>?node-id=…`). The rule this module implements:
//!
//! - selecting something, or switching pages, **replaces** the current entry —
//!   the address stays truthful without filling the back button with every
//!   click the user made on the way;
//! - opening a link, or opening another document, **pushes** an entry, so Back
//!   returns to where the link was followed from;
//! - Back/Forward (`popstate`) re-applies whatever the address now says.
//!
//! The vocabulary lives in `op_editor_core::route`; this file only reads and
//! writes the browser around it.
//!
//! ## How this file is split
//!
//! The parsing half — reading the address, naming what the editor state says,
//! reading a file list, finding the page a node is on — lives in
//! `route_sync_parse.rs`; the file browser's legs (list, open, create, rename,
//! delete, name) in `route_sync_files.rs`. Both are `#[path]` siblings, and the
//! spine re-exports what moved, so every import path into `route_sync` stays
//! where it was.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::route::{self, DocumentRoute, RouteTarget};
use wasm_bindgen::JsCast;

use crate::repaint_ctx::RepaintContext;
use crate::widget_host::WidgetHost;

#[path = "route_sync_parse.rs"]
mod parse;
use parse::{current_location, page_of, route_file_of, set_title, state_route};

#[path = "route_sync_files.rs"]
mod files;
use files::open_named_document;
pub(crate) use files::{request_file_list, tick_files};

thread_local! {
    /// A file list that arrived while the shell was busy.
    ///
    /// The XHR callback can fire mid-event, when the shell is borrowed; the
    /// frame drains this instead of dropping the answer on the floor.
    static PENDING_LIST: RefCell<Option<Result<Vec<op_editor_core::ServerFile>, String>>> =
        const { RefCell::new(None) };

    /// Whether the router has read the address yet. Until it has, the address
    /// is not ours to write: the mount paints one frame before installing the
    /// router, and that frame used to replace a pasted link with `/`.
    static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// A route read from the address that has not been applied yet.
    ///
    /// The page boots before the document arrives from the daemon, so a link
    /// naming a node has nothing to select on the first frames. Holding the
    /// request here — and refusing to write the address meanwhile — is what
    /// stops the first frame from overwriting the link with an empty state,
    /// which is exactly how the first version of this lost `?node=`.
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };

    /// The address this tab last wrote. Compared against the address the state
    /// implies so a per-frame check costs a string compare instead of a history
    /// write, and so an address that arrived from `popstate` is not written
    /// straight back.
    static LAST_WRITTEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A route waiting for the document it names.
struct Pending {
    route: DocumentRoute,
    /// Frames spent waiting; the link is given up on after a few seconds so a
    /// dead id cannot pin the address for the whole session.
    frames: u32,
    /// Frames since the link applied. The document arrives from the daemon
    /// *after* the first frame and replaces the whole document, which drops
    /// the selection the link just made — so the route is re-asserted for a
    /// moment instead of being declared done on the first success.
    settled: u32,
}

/// Roughly five seconds at 60 fps.
const PENDING_GIVE_UP_FRAMES: u32 = 300;
/// Roughly two seconds: long enough to outlast the daemon's first document.
const PENDING_SETTLE_FRAMES: u32 = 120;

/// Write the state's route into the address bar when it differs from what is
/// there. Called once per frame.
pub(crate) fn tick_pending(host: &mut WidgetHost, viewport: (f32, f32)) -> bool {
    if !INSTALLED.with(std::cell::Cell::get) {
        return false;
    }
    drive_pending(host, viewport)
}

pub(crate) fn tick(host: &WidgetHost) {
    if !INSTALLED.with(std::cell::Cell::get) {
        return;
    }
    // An invitation address belongs to the invitation, not to the editor: the
    // token in it is the credential the form is about to send, so rewriting the
    // address to `/` before it has been accepted would throw the link away (and
    // a refresh would lose it entirely). Once the acceptance succeeds the token
    // is cleared, and the next tick writes the editor's own address as usual.
    if crate::web_auth_sync::invitation_address_active(host) {
        return;
    }
    if PENDING.with(|pending| pending.borrow().is_some()) {
        // The address is the source of truth until it has been applied.
        return;
    }
    let target = state_route(host.editor_state());
    let path = route::to_path(&target);
    let already = LAST_WRITTEN.with(|last| last.borrow().as_deref() == Some(path.as_str()));
    if already {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let history = window.history().ok();
    // Selection and page changes replace; only a genuinely different document
    // earns a history entry (see the module docs).
    let document_changed = LAST_WRITTEN.with(|last| {
        let previous = last.borrow().clone();
        match (previous, &target) {
            (Some(previous), RouteTarget::Document(_)) => {
                route_file_of(&previous) != route_file_of(&path)
            }
            _ => true,
        }
    });
    let pushed = match (document_changed, &history) {
        (true, Some(history)) => history
            .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path))
            .is_ok(),
        (false, Some(history)) => history
            .replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path))
            .is_ok(),
        // No History API (an unusual embed): the address simply stays put.
        (_, None) => true,
    };
    if !pushed {
        return;
    }
    LAST_WRITTEN.with(|last| *last.borrow_mut() = Some(path));
    set_title(host, &target);
}

/// Apply whatever the address says: switch page, select and reveal a node.
///
/// Returns whether anything changed, so the caller can repaint.
pub(crate) fn apply_current(host: &mut WidgetHost, viewport: (f32, f32)) -> bool {
    let Some(target) = current_location() else {
        return false;
    };
    let RouteTarget::Document(wanted) = target else {
        // `/files`: show the file browser. The document stays open behind it,
        // so leaving the screen returns to exactly the document (and node) the
        // address still names.
        LAST_WRITTEN.with(|last| {
            *last.borrow_mut() = Some(route::to_path(&RouteTarget::Files));
        });
        return host.set_screen(op_editor_core::AppScreen::Files);
    };
    let mut changed = host.set_screen(op_editor_core::AppScreen::Editor);
    let (viewport_w, viewport_h) = viewport;
    if let Some(node) = wanted.node.as_ref() {
        let page = page_of(host.editor_state(), node);
        if let Some(page) = page {
            if page != host.editor_state().ui.active_page_index {
                let _ = host.editor_state_mut().set_active_page(page);
            }
        }
        host.editor_state_mut().set_single_selection(node.clone());
        changed = true;
        // Reveal it: a link to a node should land on that node, not on
        // whatever corner the camera happened to be pointing at.
        host.reveal_node(node.as_str(), viewport_w, viewport_h);
    } else if let Some(page) = wanted.page {
        if host.editor_state_mut().set_active_page(page) {
            changed = true;
        }
    }
    if changed {
        LAST_WRITTEN.with(|last| {
            *last.borrow_mut() = Some(route::to_path(&RouteTarget::Document(wanted)));
        });
    }
    changed
}

/// Retry a route that named a node the document did not have yet.
///
/// Returns true once it has been applied (or given up on), so the caller knows
/// the address may be written again.
fn drive_pending(host: &mut WidgetHost, viewport: (f32, f32)) -> bool {
    let waiting = PENDING.with(|pending| pending.borrow().is_some());
    if !waiting {
        return false;
    }
    let wanted = PENDING.with(|pending| {
        pending
            .borrow()
            .as_ref()
            .map(|pending_route| pending_route.route.clone())
    });
    let Some(wanted) = wanted else {
        return false;
    };
    let node_here = match wanted.node.as_ref() {
        Some(node) => page_of(host.editor_state(), node).is_some(),
        None => true,
    };
    if node_here {
        // Re-assert while the document settles; the apply is idempotent, and a
        // selection the user made in the meantime keeps the link from
        // re-firing on every frame.
        let selection_matches = match wanted.node.as_ref() {
            Some(node) => host
                .editor_state()
                .selection
                .set
                .first()
                .is_some_and(|current| current == node),
            None => true,
        };
        let mut applied = false;
        if !selection_matches {
            applied = apply_current(host, viewport);
        }
        let done = PENDING.with(|pending| {
            let mut borrow = pending.borrow_mut();
            let Some(pending_route) = borrow.as_mut() else {
                return true;
            };
            pending_route.settled += 1;
            if pending_route.settled > PENDING_SETTLE_FRAMES {
                *borrow = None;
                return true;
            }
            false
        });
        return applied || done;
    }
    let expired = PENDING.with(|pending| {
        let mut borrow = pending.borrow_mut();
        let Some(pending_route) = borrow.as_mut() else {
            return false;
        };
        pending_route.frames += 1;
        if pending_route.frames > PENDING_GIVE_UP_FRAMES {
            *borrow = None;
            return true;
        }
        false
    });
    if expired {
        // The link pointed at a node this document does not have: keep the
        // page it named and let the address fall back to the real state.
        return apply_current(host, viewport);
    }
    false
}

/// Listen for Back/Forward and apply the address on the first frame.
pub(crate) fn install<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    host: &mut WidgetHost,
    viewport: (f32, f32),
) {
    // The caller already holds the shell borrow (the mount builds the router
    // between the state setup and the first paint), so this takes the host
    // rather than borrowing it back — borrowing here panicked with
    // "RefCell already mutably borrowed" and left the link unapplied.
    if let Some(RouteTarget::Document(route)) = current_location() {
        if route.node.is_some() || route.page.is_some() {
            PENDING.with(|pending| {
                *pending.borrow_mut() = Some(Pending {
                    route,
                    frames: 0,
                    settled: 0,
                });
            });
        }
    }
    INSTALLED.with(|installed| installed.set(true));
    // A key in the address names a document the server holds: open it first,
    // and hold the route (PENDING) until that document is the live one.
    open_named_document(inner, host);
    apply_current(host, viewport);
    if host.editor_state().editor_ui.screen == op_editor_core::AppScreen::Files {
        request_file_list(inner);
    }

    let Some(window) = web_sys::window() else {
        return;
    };
    let inner_for_pop = inner.clone();
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        let viewport = inner_for_pop.borrow().viewport_size();
        if let Ok(mut borrowed) = inner_for_pop.try_borrow_mut() {
            if apply_current(borrowed.host_mut(), viewport) {
                let _ = borrowed.repaint();
            }
        }
    }) as Box<dyn FnMut()>);
    let _ = window.add_event_listener_with_callback("popstate", callback.as_ref().unchecked_ref());
    callback.forget();
}

/// Called by the mount before it tears the shell down.
pub(crate) fn forget_last_written() {
    LAST_WRITTEN.with(|last| *last.borrow_mut() = None);
}

#[cfg(test)]
#[path = "route_sync_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "route_sync_rights_tests.rs"]
mod document_rights_tests;
