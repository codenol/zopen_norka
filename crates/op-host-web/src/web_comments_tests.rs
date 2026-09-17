//! Reading the daemon's comment answers, and turning its refusals into
//! something the chrome can show.
//!
//! Most of it is pure: a status and a body in, a typed result out. The XHR
//! plumbing around it (`fetch_threads` / `post_thread`) is deliberately not
//! exercised — a browser test would be testing XmlHttpRequest, and the part
//! that has ever been wrong is the decoding: a null `authorRole`, an empty
//! `authorName`, a thread with no comments, a pin whose `pageId`/`x`/`y` are
//! `null` because the daemon migrated it from the old element-keyed format, and
//! a 403 that is an answer rather than a malfunction.
//!
//! The frame itself is exercised, though: `tick` is where a document open turns
//! into a read (issue #70), and the count of those reads — one, not one per
//! frame — is the property that matters. `hold_wire` lets the real frame path
//! run on the host target, where `web_sys` cannot be called at all.
//!
//! ## How this file is split
//!
//! Four `include!` fragments — one per scenario group — rather than `mod`
//! siblings, because `cargo test -- --list` names a test after the module path
//! it is defined in, and a `mod` split of a file that IS the `tests` module
//! would therefore rename every test in it (issue #220). An `include!` fragment
//! is spliced in at its include site, so the module path, the declaration order
//! and the whole test list stay byte-identical. The groups:
//! `web_comments_tests/open_reads.rs` is what a document open turns into,
//! `decoding.rs` what an answer decodes to, `install.rs` what an answer does to
//! the panel, and `wire_shapes.rs` the shapes the wire carries.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsValue;

use crate::repaint_ctx::RepaintContext;
use crate::widget_host::WidgetHost;

use super::*;

/// The smallest shell `tick` runs against: a host, and a repaint tally.
struct Frame {
    host: WidgetHost,
    repaints: usize,
}

impl RepaintContext for Frame {
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
        self.repaints += 1;
        Ok(())
    }
}

/// A tab with the daemon's comment client, on `key`.
///
/// The wire is held before the caller's first frame: every `web_sys` call is a
/// wasm import that panics on the host target, so a test that let a request
/// through would take the process down rather than fail an assertion.
fn open_document(key: Option<&str>) -> Rc<RefCell<Frame>> {
    hold_wire::hold();
    let mut host = WidgetHost::new();
    {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.file_key = key.map(str::to_string);
        ui.comments.transport = true;
    }
    // Each test drives one tab, and the identity epoch is a thread-local the
    // tests below move deliberately.
    crate::identity_epoch::reset_for_test();
    Rc::new(RefCell::new(Frame { host, repaints: 0 }))
}

fn frame(inner: &Rc<RefCell<Frame>>) -> usize {
    tick(inner);
    hold_wire::reads().len()
}

/// The document the tab is showing, as the router would set it on an open.
fn set_open_key(inner: &Rc<RefCell<Frame>>, key: &str) {
    let mut borrowed = inner.borrow_mut();
    borrowed.host_mut().editor_state_mut().editor_ui.file_key = Some(key.to_string());
}

fn a_thread(id: i64, page: &str) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, 10.0, 20.0)),
        comments: vec![Comment::default()],
        ..CommentThread::default()
    }
}

// The file is exactly these four fragments, in the order they were written,
// and each fragment is a whole test item: the blank line between two includes
// is the blank line that separated them in the one file this used to be, which
// is what makes the fragments reassemble it byte for byte. A fragment is
// formatted (rustfmt over it is a no-op); it is just not part of the module
// tree `cargo fmt` walks.
include!("web_comments_tests/open_reads.rs");

include!("web_comments_tests/decoding.rs");

include!("web_comments_tests/install.rs");

include!("web_comments_tests/wire_shapes.rs");
