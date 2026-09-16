// Browser boundary: IndexedDB persistence needs a real browser to exercise the
// open / put / delete round-trip; the pure record codec and the comparison it
// feeds are unit-tested in `op_editor_core::editor_ui_state::copy_status`.
//! IndexedDB persistence for the browser's copy of the document (issue #171).
//!
//! This is the storage half of #171's "a local document store in the browser":
//! the last known document and the identity of the copy it came from, kept
//! where the browser shell already keeps things that outlive a page.
//!
//! ## Why a second database and not a second store
//!
//! `font_store_idb` already owns the database `openpencil` at version 1 with
//! the single store `imported_fonts`. Adding a store to it means opening at
//! version 2 — and then that module's own `open_with_u32("openpencil", 1)`
//! fails with `VersionError`, so **every imported font would silently stop
//! persisting**. Two independent databases cannot break each other, and the
//! cost is one more name. The conventions are otherwise identical: out-of-line
//! keys, an upgrade handler that creates the store, and a defensive posture
//! where any IndexedDB error logs to the console and degrades to "no local
//! copy" rather than blocking the editor.
//!
//! ## What is stored
//!
//! One record per (account, document) under
//! [`op_editor_core::editor_ui_state::copy_status::record_key`] — the document's
//! serialized bytes plus its identity: the daemon's key, the daemon version the
//! copy came from, a content fingerprint, and when it was written. A copy is
//! several megabytes (a kit-backed screen is ~3.4 MiB), which is exactly why
//! this is IndexedDB and not `localStorage`, the shell's preference adapter.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbTransactionMode};

/// Its own database — see the module docs for why sharing `openpencil` with
/// `font_store_idb` would silently break imported fonts.
const DB_NAME: &str = "openpencil_documents";
const STORE: &str = "documents";
const DB_VERSION: u32 = 1;

/// IndexedDB exists only in a browser. On the host (the `cargo test` harness
/// that runs this crate's unit tests) every entry point reports "nothing was
/// stored" and does nothing, instead of reaching a `web_sys` imported static
/// that panics off-wasm. This is the same degrade the module promises for a
/// browser that refuses IndexedDB: no local copy, never an error the editor has
/// to handle.
const BROWSER: bool = cfg!(target_arch = "wasm32");

/// One read's answer, or `None` when there is nothing readable to hand back.
type ReadCallback = Box<dyn FnOnce(Option<String>)>;

fn console_warn(msg: &str) {
    web_sys::console::warn_1(&JsValue::from_str(msg));
}

// The open database, cached for the page's life. Every operation here is a
// document-sized write or a rare read, so re-opening per call would be
// wasteful; the cache is cleared when the open fails so a transient failure
// does not disable persistence for the session.
thread_local! {
    static DB: RefCell<Option<IdbDatabase>> = const { RefCell::new(None) };
}

/// Open (once) and hand the live database to `on_ready`. Any failure logs and
/// drops the callback: no local copy is a degradation, never an error the
/// editor has to handle.
fn with_db(on_ready: Box<dyn FnOnce(IdbDatabase)>) {
    if let Some(db) = DB.with(|slot| slot.borrow().clone()) {
        on_ready(db);
        return;
    }
    let result = (|| -> Result<(), JsValue> {
        let window =
            web_sys::window().ok_or_else(|| JsValue::from_str("doc-store: window unavailable"))?;
        let factory = window
            .indexed_db()?
            .ok_or_else(|| JsValue::from_str("doc-store: IndexedDB unavailable"))?;
        let open_req: IdbOpenDbRequest = factory.open_with_u32(DB_NAME, DB_VERSION)?;

        // onupgradeneeded (first open, or a future version bump) — create the
        // store. The request's `result()` is the upgrading database here.
        {
            let upgrade_req = open_req.clone();
            let upgrade = Closure::<dyn FnMut()>::once(move || {
                if let Ok(db) = upgrade_req.result().and_then(|v| {
                    v.dyn_into::<IdbDatabase>()
                        .map_err(|_| JsValue::from_str("upgrade: not a database"))
                }) {
                    // An "already exists" error is harmless on re-entry.
                    let _ = db.create_object_store(STORE);
                }
            });
            open_req.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
            upgrade.forget();
        }

        {
            let success_req = open_req.clone();
            let mut once = Some(on_ready);
            let success = Closure::<dyn FnMut()>::once(move || {
                let db = success_req
                    .result()
                    .ok()
                    .and_then(|v| v.dyn_into::<IdbDatabase>().ok());
                if let Some(db) = db {
                    DB.with(|slot| *slot.borrow_mut() = Some(db.clone()));
                    if let Some(cb) = once.take() {
                        cb(db);
                    }
                }
            });
            open_req.set_onsuccess(Some(success.as_ref().unchecked_ref()));
            success.forget();
        }

        {
            let error = Closure::<dyn FnMut()>::once(move || {
                stop_using_db("doc-store: IndexedDB open failed; no local copy is kept");
            });
            open_req.set_onerror(Some(error.as_ref().unchecked_ref()));
            error.forget();
        }
        Ok(())
    })();
    if let Err(e) = result {
        web_sys::console::warn_1(&e);
    }
}

/// Forget the cached handle and say why, once.
fn stop_using_db(message: &str) {
    DB.with(|slot| *slot.borrow_mut() = None);
    console_warn(message);
}

/// A read/write transaction on the store, or `None` (after logging).
fn writable_store(db: &IdbDatabase) -> Option<IdbObjectStore> {
    match db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite) {
        Ok(tx) => tx.object_store(STORE).ok(),
        Err(e) => {
            web_sys::console::warn_1(&e);
            None
        }
    }
}

/// Persist one record under `key`; replaces any previous record for it.
///
/// Fire-and-forget in the sense that the caller never waits: the transaction
/// auto-commits and the write is a multi-megabyte structured clone. `on_result`
/// is still called, because whether the store accepted the copy is exactly what
/// the status surface needs — a copy that failed to persist is not the copy
/// this browser keeps, and reporting it as one would describe a record that is
/// not there.
pub(crate) fn put(key: &str, json: String, on_result: Box<dyn FnOnce(bool)>) {
    if key.is_empty() || !BROWSER {
        on_result(false);
        return;
    }
    let key = key.to_string();
    with_db(Box::new(move |db| {
        let Some(store) = writable_store(&db) else {
            on_result(false);
            return;
        };
        // A plain string, so the structured clone IndexedDB performs does not
        // reference wasm linear memory (the rule `font_store_idb` states for
        // its byte arrays).
        match store.put_with_key(&JsValue::from_str(&json), &JsValue::from_str(&key)) {
            Ok(req) => {
                // Exactly one of the two closures below runs, and the callback
                // is `FnOnce`, so it is shared behind a once-guard rather than
                // moved into whichever closure happens to be defined first.
                let once = Rc::new(RefCell::new(Some(on_result)));
                let done = once.clone();
                let success = Closure::<dyn FnMut()>::once(move || {
                    if let Some(cb) = done.borrow_mut().take() {
                        cb(true);
                    }
                });
                req.set_onsuccess(Some(success.as_ref().unchecked_ref()));
                success.forget();
                let done = once.clone();
                let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
                    // A quota failure repeats on every write and the console is
                    // the only place a person can see it. Say it once.
                    stop_using_db("doc-store: could not keep a local copy");
                    if let Some(cb) = done.borrow_mut().take() {
                        cb(false);
                    }
                });
                req.set_onerror(Some(error.as_ref().unchecked_ref()));
                error.forget();
            }
            Err(e) => {
                web_sys::console::warn_1(&e);
                on_result(false);
            }
        }
    }));
}

/// Read the record under `key`, handing `on_done` `None` when there is none.
///
/// `on_done` runs only on a successful read; every error path logs and drops
/// it, so a broken store reads as "no local copy" rather than as a hang.
pub(crate) fn read(key: &str, on_done: ReadCallback) {
    if key.is_empty() || !BROWSER {
        on_done(None);
        return;
    }
    let key = key.to_string();
    with_db(Box::new(move |db| {
        let store = match db.transaction_with_str(STORE) {
            Ok(tx) => match tx.object_store(STORE) {
                Ok(store) => store,
                Err(e) => {
                    web_sys::console::warn_1(&e);
                    return;
                }
            },
            Err(e) => {
                web_sys::console::warn_1(&e);
                return;
            }
        };
        let req = match store.get(&JsValue::from_str(&key)) {
            Ok(req) => req,
            Err(e) => {
                web_sys::console::warn_1(&e);
                return;
            }
        };
        let result_req = req.clone();
        let mut once = Some(on_done);
        let success = Closure::<dyn FnMut()>::once(move || {
            let value = result_req.result().unwrap_or(JsValue::NULL);
            let json = value.as_string().filter(|raw| !raw.is_empty());
            if let Some(cb) = once.take() {
                cb(json);
            }
        });
        req.set_onsuccess(Some(success.as_ref().unchecked_ref()));
        success.forget();
        let error = Closure::<dyn FnMut()>::once(move || {
            console_warn("doc-store: could not read the local copy");
        });
        req.set_onerror(Some(error.as_ref().unchecked_ref()));
        error.forget();
    }));
}

/// Forget the cached handle. Exists for the identity reset: the next write
/// re-opens the database, so a copy belonging to the previous account is never
/// read back into the new one.
pub(crate) fn forget_handle() {
    DB.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store's identity is its database name, its store name and its record
    /// key. Two of those are constants in a browser-only module, so this pins
    /// the third: the database must be its own, because sharing `openpencil`
    /// with `font_store_idb` requires a version bump that module's own
    /// `open(DB_NAME, 1)` cannot survive.
    #[test]
    fn the_documents_database_is_not_the_fonts_database() {
        let fonts = include_str!("font_store_idb.rs");
        let fonts_db = fonts
            .split("const DB_NAME: &str = ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .expect("the font store names its database");
        assert_eq!(fonts_db.trim(), "\"openpencil\"");
        assert_ne!(
            DB_NAME, "openpencil",
            "a second store in the fonts' database would break imported fonts"
        );
        assert_eq!(DB_VERSION, 1, "and it carries its own version");
    }
}
