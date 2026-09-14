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

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::route::{self, DocumentRoute, RouteFile, RoutePath, RouteTarget};
use op_editor_core::NodeId;
use wasm_bindgen::JsCast;

use crate::repaint_ctx::RepaintContext;
use crate::widget_host::WidgetHost;

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

/// Read the tab's current route, if it names one.
pub(crate) fn current_location() -> Option<RouteTarget> {
    let window = web_sys::window()?;
    let location = window.location();
    let path = location.pathname().ok()?;
    let query = location.search().unwrap_or_default();
    match route::parse(&path, &query) {
        RoutePath::Known(target) => Some(target),
        RoutePath::NotARoute => None,
    }
}

/// The route the editor state currently describes, for the address bar.
///
/// Two clauses, on purpose. The document mapping is the shared
/// [`route::state_route`] — the same rule the desktop records for Back/Forward
/// — and the browser adds the one thing that rule cannot know: `/files` is a
/// screen rather than a place in a document. A copy of the document mapping
/// here is exactly the drift the shared rule exists to prevent, so there is
/// none.
pub(crate) fn state_route(state: &op_editor_core::EditorState) -> RouteTarget {
    if state.editor_ui.screen == op_editor_core::AppScreen::Files {
        return RouteTarget::Files;
    }
    route::state_route(state, route::file_from_key(state))
}

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

/// The document part of an address, for comparing two routes.
fn route_file_of(path: &str) -> Option<String> {
    match route::parse(path, "") {
        RoutePath::Known(RouteTarget::Document(route)) => Some(match &route.file {
            RouteFile::Key(key) => key.clone(),
            RouteFile::Untitled => "/".to_string(),
        }),
        _ => None,
    }
}

fn set_title(host: &WidgetHost, target: &RouteTarget) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let name = host.editor_state().editor_ui.file_name_display.clone();
    let base = op_editor_ui::PRODUCT_NAME;
    let title = match (&target, name) {
        (RouteTarget::Files, _) => format!("Files — {base}"),
        (RouteTarget::Document(_), Some(name)) => format!("{name} — {base}"),
        (RouteTarget::Document(_), None) => format!("Untitled — {base}"),
    };
    document.set_title(&title);
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

/// Open the document the address names, when it is not the open one.
///
/// The browser cannot read a file; the daemon can. This is the one call that
/// turns `/f/<key>` into a document, and it deliberately does nothing when the
/// key already matches the open document (a reload of the same file must not
/// throw away unsaved work).
fn open_named_document<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, host: &mut WidgetHost) {
    let Some(RouteTarget::Document(route)) = current_location() else {
        return;
    };
    let Some(key) = route.key().map(str::to_string) else {
        return;
    };
    if host.editor_state().editor_ui.file_key.as_deref() == Some(key.as_str()) {
        return;
    }
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let key_for_response = key.clone();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&response) else {
            return;
        };
        if value.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
            // Unknown or deleted key: keep the address out of the way and let
            // the shell show whatever document is already open.
            PENDING.with(|pending| *pending.borrow_mut() = None);
            return;
        }
        let name = value
            .get("name")
            .and_then(|name| name.as_str())
            .map(str::to_string);
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            let state = borrowed.host_mut().editor_state_mut();
            state.editor_ui.file_key = Some(key_for_response.clone());
            if name.is_some() {
                state.editor_ui.file_name_display = name;
            }
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        // A name that did not travel with the open (an older daemon, or a
        // document whose index row is older than the file) is read from the
        // list rather than left as "Untitled" in the tab title.
        request_name_for_key(&inner_for_response, &key_for_response);
        // The document itself arrives through the normal version pull; the
        // pending route then lands on the page/node it names.
        crate::live_sync_glue::request_document_pull(&inner_for_response);
    });
    if !crate::live_sync::post_json(
        &format!("{base}/api/files/{key}/open"),
        "{}",
        Some(on_response),
    ) {
        PENDING.with(|pending| *pending.borrow_mut() = None);
    }
}

/// Open a stored document and put its address in the bar.
fn open_stored_document<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, key: &str) {
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let key = key.to_string();
    // The closure owns the key for the state write; the request URL needs its
    // own copy.
    let key_for_url = key.clone();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        let ok = serde_json::from_str::<serde_json::Value>(&response)
            .ok()
            .and_then(|value| value.get("ok").and_then(|ok| ok.as_bool()))
            .unwrap_or(false);
        if !ok {
            if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
                borrowed
                    .host_mut()
                    .editor_state_mut()
                    .editor_ui
                    .server_files_error = Some("That file could not be opened".to_string());
                let _ = borrowed.repaint();
            }
            return;
        }
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            let state = borrowed.host_mut().editor_state_mut();
            state.editor_ui.file_key = Some(key.clone());
            state.editor_ui.screen = op_editor_core::AppScreen::Editor;
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        // The document itself arrives on the version pull; the address is
        // written on the next frame from the state the open just set.
        crate::live_sync_glue::request_document_pull(&inner_for_response);
    });
    let _ = crate::live_sync::post_json(
        &format!("{base}/api/files/{key_for_url}/open"),
        "{}",
        Some(on_response),
    );
    LAST_WRITTEN.with(|last| *last.borrow_mut() = None);
}

/// Create a stored document and open it.
fn create_stored_document<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        let key = serde_json::from_str::<serde_json::Value>(&response)
            .ok()
            .and_then(|value| value.get("file").cloned())
            .and_then(|file| {
                file.get("key")
                    .and_then(|key| key.as_str())
                    .map(str::to_string)
            });
        let Some(key) = key else {
            if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
                borrowed
                    .host_mut()
                    .editor_state_mut()
                    .editor_ui
                    .server_files_error = Some("A new file could not be created".to_string());
                let _ = borrowed.repaint();
            }
            return;
        };
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            let state = borrowed.host_mut().editor_state_mut();
            state.editor_ui.file_key = Some(key);
            state.editor_ui.screen = op_editor_core::AppScreen::Editor;
            // The list shown next time must include this file.
            state.editor_ui.server_files.clear();
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        crate::live_sync_glue::request_document_pull(&inner_for_response);
    });
    if crate::live_sync::post_json(&format!("{base}/api/files"), "{}", Some(on_response)) {
        LAST_WRITTEN.with(|last| *last.borrow_mut() = None);
    }
}

/// Keep the file browser's list current.
///
/// Called once per frame with the shell in hand: the screen can be reached by
/// the address (applied in `install`) or by a click, and a click has no
/// route to fetch from — so the frame is the one place both paths go through.
pub(crate) fn tick_files<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    // Apply a list that arrived while the shell was busy.
    let arrived = PENDING_LIST.with(|pending| pending.borrow_mut().take());
    if let Some(arrived) = arrived {
        if let Ok(mut borrowed) = inner.try_borrow_mut() {
            let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
            ui.server_files_loading = false;
            match arrived {
                Ok(files) => {
                    ui.server_files = files;
                    ui.server_files_error = None;
                }
                Err(error) => ui.server_files_error = Some(error),
            }
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        } else {
            PENDING_LIST.with(|pending| *pending.borrow_mut() = Some(arrived));
            return;
        }
    }
    // A click on the file screen, performed here because the widget layer has
    // no transport of its own.
    let base = crate::daemon_base::daemon_base();
    let (open_request, create_request) = {
        let Ok(mut borrowed) = inner.try_borrow_mut() else {
            return;
        };
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        (
            ui.server_files_open_request.take(),
            std::mem::take(&mut ui.server_files_create_request),
        )
    };
    if let Some(key) = open_request {
        open_stored_document(inner, &key);
        return;
    }
    // Rename and delete are asked for by the screen and performed here.
    let (rename_request, delete_request) = {
        let Ok(mut borrowed) = inner.try_borrow_mut() else {
            return;
        };
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        (
            ui.server_files_rename_request.take(),
            ui.server_files_delete_request.take(),
        )
    };
    if let Some((key, name)) = rename_request {
        // Optimistic: the card shows the new name immediately, and the fresh
        // list below is the authority.
        if let Ok(mut borrowed) = inner.try_borrow_mut() {
            let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
            if let Some(file) = ui.server_files.iter_mut().find(|file| file.key == key) {
                file.name = name.clone();
            }
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        let body = serde_json::json!({ "name": name }).to_string();
        let inner_for_response = inner.clone();
        let on_response: Rc<dyn Fn(String)> = Rc::new(move |_response: String| {
            // The optimistic rename above is corrected by a fresh list.
            request_file_list(&inner_for_response);
        });
        let _ = crate::live_sync::post_json(
            &format!("{base}/api/files/{key}/rename"),
            &body,
            Some(on_response),
        );
        return;
    }
    if let Some(key) = delete_request {
        let inner_for_response = inner.clone();
        let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
            let ok = serde_json::from_str::<serde_json::Value>(&response)
                .ok()
                .and_then(|value| value.get("ok").and_then(|ok| ok.as_bool()))
                .unwrap_or(false);
            if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
                let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
                if !ok {
                    ui.server_files_error = Some("That file could not be deleted".to_string());
                }
            }
            request_file_list(&inner_for_response);
        });
        let _ =
            crate::live_sync::delete_json(&format!("{base}/api/files/{key}"), Some(on_response));
        return;
    }
    if create_request {
        create_stored_document(inner);
        return;
    }
    let (is_files, needs_list) = {
        let Ok(borrowed) = inner.try_borrow() else {
            return;
        };
        let ui = &borrowed.host().editor_state().editor_ui;
        let is_files = ui.screen == op_editor_core::AppScreen::Files;
        // An empty list with no error and no request in flight is "never
        // asked", not "nothing there": ask.
        let needs_list = ui.server_files.is_empty()
            && !ui.server_files_loading
            && ui.server_files_error.is_none();
        (is_files, needs_list)
    };
    if is_files && needs_list {
        request_file_list(inner);
    }
}

/// Fetch `/api/files` into the state the file screen paints.
pub(crate) fn request_file_list<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    {
        let Ok(mut borrowed) = inner.try_borrow_mut() else {
            return;
        };
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        if ui.server_files_loading {
            return;
        }
        ui.server_files_loading = true;
        ui.server_files_error = None;
    }
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        let parsed = parse_file_list(&response);
        let Ok(mut borrowed) = inner_for_response.try_borrow_mut() else {
            // Busy right now — hand it to the next frame rather than losing
            // it, and ask for that frame: a page sitting still has no frame
            // coming of its own.
            PENDING_LIST.with(|pending| *pending.borrow_mut() = Some(parsed));
            crate::repaint_coalescer::request();
            return;
        };
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        ui.server_files_loading = false;
        match parsed {
            Ok(files) => {
                // A document that is gone takes its preview with it, so a file
                // recreated under the same key cannot show the old picture.
                for stale in ui
                    .server_files
                    .iter()
                    .map(|file| file.key.clone())
                    .filter(|key| !files.iter().any(|file| &file.key == key))
                    .collect::<Vec<_>>()
                {
                    op_editor_ui::files_thumb_runtime::forget_thumb(&stale);
                }
                ui.server_files = files;
                ui.server_files_error = None;
            }
            Err(error) => {
                ui.server_files_error = Some(error);
            }
        }
        borrowed.host_mut().mark_editor_state_dirty();
        let _ = borrowed.repaint();
    });
    if !crate::live_sync::get(&format!("{base}/api/files"), on_response) {
        if let Ok(mut borrowed) = inner.try_borrow_mut() {
            let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
            ui.server_files_loading = false;
            ui.server_files_error = Some("Could not reach the server".to_string());
        }
    }
}

/// Read a `/api/files` response, newest first, capped for the screen.
fn parse_file_list(response: &str) -> Result<Vec<op_editor_core::ServerFile>, String> {
    let value: serde_json::Value = serde_json::from_str(response)
        .map_err(|_| "The server sent an unreadable list".to_string())?;
    if value.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return Err(value
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or("The server refused the list")
            .to_string());
    }
    let files = value
        .get("files")
        .and_then(|files| files.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    Some(op_editor_core::ServerFile {
                        key: file.get("key")?.as_str()?.to_string(),
                        name: file
                            .get("name")
                            .and_then(|name| name.as_str())
                            .unwrap_or("Untitled")
                            .to_string(),
                        updated_at: file
                            .get("updatedAt")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(0),
                        size: file
                            .get("size")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(0),
                        has_thumbnail: file
                            .get("hasThumbnail")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut files = files;
    files.truncate(op_editor_core::SERVER_FILE_CAP);
    Ok(files)
}

/// Fill in the open document's display name from the file list.
fn request_name_for_key<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, key: &str) {
    let base = crate::daemon_base::daemon_base();
    let inner_for_response = inner.clone();
    let key = key.to_string();
    let on_response: Rc<dyn Fn(String)> = Rc::new(move |response: String| {
        let Some(name) = serde_json::from_str::<serde_json::Value>(&response)
            .ok()
            .and_then(|value| {
                value
                    .get("files")
                    .and_then(|files| files.as_array())
                    .and_then(|files| {
                        files.iter().find(|file| {
                            file.get("key").and_then(|value| value.as_str()) == Some(key.as_str())
                        })
                    })
                    .and_then(|file| file.get("name"))
                    .and_then(|name| name.as_str())
                    .map(str::to_string)
            })
        else {
            return;
        };
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            let state = borrowed.host_mut().editor_state_mut();
            // Only when the key is still the open one: a fast second open must
            // not title the new document with the previous one's name.
            if state.editor_ui.file_key.as_deref() == Some(key.as_str()) {
                state.editor_ui.file_name_display = Some(name);
                borrowed.host_mut().mark_editor_state_dirty();
                let _ = borrowed.repaint();
            }
        }
    });
    let _ = crate::live_sync::get(&format!("{base}/api/files"), on_response);
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

/// The page index holding `node`, when the document has it.
fn page_of(state: &op_editor_core::EditorState, node: &NodeId) -> Option<usize> {
    let pages = state.doc.pages.as_ref()?;
    pages.iter().position(|page| {
        page.children.iter().any(|root| {
            op_editor_core::walkers::find_node(std::slice::from_ref(root), node).is_some()
        })
    })
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

/// Keeps `route::slugify` reachable from the host without importing the crate
/// path everywhere; also documents the only place the slug is produced.
pub(crate) fn slug_for(name: &str) -> String {
    route::slugify(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_part_is_extracted_for_history_comparison() {
        assert_eq!(route_file_of("/"), Some("/".to_string()));
        assert_eq!(route_file_of("/f/abc/slug"), Some("abc".to_string()));
        assert_eq!(route_file_of("/files"), None);
    }

    #[test]
    fn slugs_come_from_the_shared_rule() {
        assert_eq!(slug_for("Список токенов"), "spisok-tokenov");
    }
}
