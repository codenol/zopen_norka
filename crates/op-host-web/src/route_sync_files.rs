//! The file browser's legs: listing, opening, creating and naming documents.
//!
//! Everything here turns a click on the file screen — or a key in the address —
//! into a request the daemon serves, and installs what comes back into the state
//! the screen paints. The route reading it stands on is `route_sync_parse`'s;
//! the pending-route and address bookkeeping it writes is the spine's.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::route::RouteTarget;

use super::parse::{can_write_from_open, current_location, parse_file_list};
use super::{LAST_WRITTEN, PENDING, PENDING_LIST};
use crate::repaint_ctx::RepaintContext;
use crate::widget_host::WidgetHost;

/// Open the document the address names, when it is not the open one.
///
/// The browser cannot read a file; the daemon can. This is the one call that
/// turns `/f/<key>` into a document, and it deliberately does nothing when the
/// key already matches the open document (a reload of the same file must not
/// throw away unsaved work).
pub(super) fn open_named_document<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    host: &mut WidgetHost,
) {
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
        let can_write = can_write_from_open(&value);
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            let state = borrowed.host_mut().editor_state_mut();
            state
                .editor_ui
                .set_document_key(Some(key_for_response.clone()));
            if let Some(can_write) = can_write {
                state.editor_ui.document_read_only = !can_write;
            }
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
            state.editor_ui.set_document_key(Some(key.clone()));
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
            state.editor_ui.set_document_key(Some(key));
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
