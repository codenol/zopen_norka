//! The recovery offer's browser half: the launch probe and the two answers.
//!
//! A document with no server key writes itself into the daemon's draft slot
//! (`crate::web_autosave`). This module asks about that slot exactly once per
//! page, and performs whichever answer the banner recorded — the widget layer
//! owns no transport, so the press travels through `EditorUiState` and lands
//! here (the same hand-off `route_sync` uses for the file screen).
//!
//! Why a probe at all, rather than a banner that appears with the document:
//! the draft is *not* a document in the store. It has no key, it is never
//! listed, and the only way to know it exists is to ask. The question is asked
//! once, at mount, because the answer is about the *last* session: a draft
//! written later in this session is this session's own work, already on
//! screen.
//!
//! Nothing here is retried. A daemon that does not answer means no offer —
//! the work is still in its slot, and the next launch asks again. An answer
//! the daemon rejects is treated the same way: the user's answer stands for
//! this page (the banner must not come back), and the work is offered again
//! next time rather than forcing a decision loop on someone who already made
//! it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::editor_ui_state::{RecoveryDraft, RecoveryRequest};

use crate::repaint_ctx::RepaintContext;

thread_local! {
    /// Whether the launch probe has been issued. One question per page, for
    /// the reason in the module docs.
    static PROBED: Cell<bool> = const { Cell::new(false) };

    /// A probe answer that arrived while the shell was borrowed.
    ///
    /// The XHR callback can fire mid-event, when `inner` is already borrowed;
    /// dropping the answer there would silently hide the user's work, so it
    /// waits here for the next frame instead.
    static PENDING_PROBE: RefCell<Option<Option<RecoveryDraft>>> = const { RefCell::new(None) };
}

/// Claim the one-shot launch probe slot. `true` for the first caller only.
pub(crate) fn claim_probe_slot() -> bool {
    PROBED.with(|probed| !probed.replace(true))
}

#[cfg(test)]
pub(crate) fn reset_probe_state_for_test() {
    PROBED.with(|probed| probed.set(false));
    PENDING_PROBE.with(|pending| *pending.borrow_mut() = None);
}

/// Ask the daemon what its draft slot holds.
///
/// Returns whether a request was started. Called once, from the mount path
/// beside the other one-shot daemon reads.
pub(crate) fn probe_at_mount<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) -> bool {
    if !claim_probe_slot() {
        return false;
    }
    let url = format!("{}/api/recovery", crate::daemon_base::daemon_base());
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, body| {
        // A non-200 is not an answer — an unreachable or older daemon has no
        // draft slot, and guessing "no draft" is the same as guessing "a draft
        // the user will never see again". Leaving the offer unset is the only
        // honest reading; the next launch asks again.
        if status != 200 {
            return;
        }
        let Some(found) = parse_probe_response(&body) else {
            return;
        };
        apply_probe(&inner_for_response, found);
    });
    crate::live_sync::get_with_status(&url, on_response)
}

/// Read a `GET /api/recovery` body.
///
/// `None` for anything unreadable: a body that cannot be parsed is not
/// evidence of a draft, and offering a recovery the daemon never mentioned
/// would be worse than saying nothing.
pub(crate) fn parse_probe_response(body: &str) -> Option<Option<RecoveryDraft>> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return None;
    }
    let exists = value
        .get("exists")
        .and_then(|exists| exists.as_bool())
        .unwrap_or(false);
    if !exists {
        return Some(None);
    }
    Some(Some(RecoveryDraft {
        saved_at: value
            .get("savedAt")
            .and_then(|saved| saved.as_u64())
            .unwrap_or(0),
        size: value
            .get("size")
            .and_then(|size| size.as_u64())
            .unwrap_or(0),
    }))
}

/// Put a probe answer into the state, or park it for the next frame.
fn apply_probe<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, draft: Option<RecoveryDraft>) {
    let Ok(mut borrowed) = inner.try_borrow_mut() else {
        PENDING_PROBE.with(|pending| *pending.borrow_mut() = Some(draft));
        return;
    };
    borrowed
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .note_recovery_probe(draft);
    borrowed.host_mut().mark_editor_state_dirty();
    let _ = borrowed.repaint();
}

/// Per-frame work: deliver a parked probe answer, then perform the answer the
/// banner recorded.
pub(crate) fn tick<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let arrived = PENDING_PROBE.with(|pending| pending.borrow_mut().take());
    if let Some(draft) = arrived {
        apply_probe(inner, draft);
    }
    let request = {
        let Ok(mut borrowed) = inner.try_borrow_mut() else {
            return;
        };
        borrowed
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .recovery_request
            .take()
    };
    match request {
        Some(RecoveryRequest::Restore) => restore(inner),
        Some(RecoveryRequest::Discard) => discard(inner),
        None => {}
    }
}

/// Adopt the draft: the daemon applies it to its open document and clears the
/// slot, then the tab pulls the document it now holds.
fn restore<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let url = format!("{}/api/recovery/restore", crate::daemon_base::daemon_base());
    let inner_for_response = inner.clone();
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, _body| {
        if status != 200 {
            return;
        }
        // The daemon adopted a draft, and a draft has no key — that is what
        // makes it a draft (`restore_draft` clears the daemon's key and its
        // bound path). So this tab stops being the server document it was
        // showing: keeping the key would point Save / autosave / comments at a
        // stored document that no longer holds this content (issue #92).
        if let Ok(mut borrowed) = inner_for_response.try_borrow_mut() {
            adopt_restored_draft(borrowed.host_mut().editor_state_mut());
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        // The daemon replaced the open document; the tab is still showing the
        // one it had. The pull is the same one the file screen uses after
        // `/api/files/{key}/open`.
        crate::live_sync_glue::request_document_pull(&inner_for_response);
    });
    // `post_json_with_status`, not `post_json`: a rejection is a real outcome
    // here (the collaboration gate can refuse a document replace), and the
    // body of an error is not a document to pull.
    let _ = crate::live_sync::post_json_with_status(&url, "{}", on_response);
}

/// Let the draft go. Nothing depends on the reply: the user's answer is a
/// page-lifetime statement, and the slot is the daemon's to clear.
fn discard<C: RepaintContext + 'static>(_inner: &Rc<RefCell<C>>) {
    let url = format!("{}/api/recovery", crate::daemon_base::daemon_base());
    let _ = crate::live_sync::delete_json(&url, None);
}

/// What a successful restore means for the tab's own state.
///
/// The restored document is the daemon's draft — key-less by construction —
/// so the tab drops the server identity it had. Split out from the response
/// closure so the rule is testable without a wire (the same reason the comment
/// transport keeps its decoding separate).
pub(crate) fn adopt_restored_draft(state: &mut op_editor_core::EditorState) {
    state.editor_ui.set_document_key(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_host::WidgetHost;
    use wasm_bindgen::JsValue;

    struct ProbeContext {
        host: WidgetHost,
        repaints: usize,
    }

    impl RepaintContext for ProbeContext {
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

    fn context() -> Rc<RefCell<ProbeContext>> {
        reset_probe_state_for_test();
        Rc::new(RefCell::new(ProbeContext {
            host: WidgetHost::new(),
            repaints: 0,
        }))
    }

    #[test]
    fn the_probe_slot_is_claimed_once() {
        reset_probe_state_for_test();
        assert!(claim_probe_slot(), "the first mount asks");
        assert!(!claim_probe_slot(), "and no later frame asks again");
    }

    #[test]
    fn an_empty_slot_parses_as_nothing_to_offer() {
        assert_eq!(
            parse_probe_response(r#"{"ok":true,"exists":false}"#),
            Some(None)
        );
    }

    #[test]
    fn a_draft_parses_with_its_stamp_and_size() {
        let parsed =
            parse_probe_response(r#"{"ok":true,"exists":true,"savedAt":1700,"size":4096}"#);
        assert_eq!(
            parsed,
            Some(Some(RecoveryDraft {
                saved_at: 1_700,
                size: 4_096
            }))
        );
    }

    #[test]
    fn an_unreadable_or_refused_body_is_not_an_answer() {
        for body in [
            "not json",
            r#"{"ok":false,"error":"nope"}"#,
            r#"{"exists":true}"#,
            "",
        ] {
            assert_eq!(parse_probe_response(body), None, "{body}");
        }
    }

    #[test]
    fn a_probe_answer_reaches_the_state_and_repaints() {
        let inner = context();
        apply_probe(
            &inner,
            Some(RecoveryDraft {
                saved_at: 9,
                size: 1,
            }),
        );

        let borrowed = inner.borrow();
        assert_eq!(
            borrowed.host.editor_state().editor_ui.recovery_draft,
            Some(RecoveryDraft {
                saved_at: 9,
                size: 1
            })
        );
        assert_eq!(
            borrowed.repaints, 1,
            "the bar cannot appear without a frame"
        );
    }

    #[test]
    fn a_probe_answer_that_arrives_mid_event_waits_for_the_frame() {
        // The XHR callback can fire while the shell is borrowed. Dropping the
        // answer there is how a user's work goes missing without a word.
        let inner = context();
        let held = inner.borrow_mut();
        apply_probe(
            &inner,
            Some(RecoveryDraft {
                saved_at: 9,
                size: 1,
            }),
        );
        assert!(
            PENDING_PROBE.with(|pending| pending.borrow().is_some()),
            "a borrowed shell parks the answer instead of dropping it"
        );
        drop(held);

        tick(&inner);
        assert_eq!(
            inner.borrow().host.editor_state().editor_ui.recovery_draft,
            Some(RecoveryDraft {
                saved_at: 9,
                size: 1
            })
        );
        assert!(PENDING_PROBE.with(|pending| pending.borrow().is_none()));
    }

    #[test]
    fn a_restored_draft_is_not_the_server_document_it_replaced() {
        // Issue #92, on the recovery path: the banner can be answered while a
        // stored document is open, and the daemon's draft it adopts has no key.
        // A tab that kept its old key would autosave the draft's contents into
        // that stored document.
        let mut host = WidgetHost::new();
        host.editor_state_mut()
            .editor_ui
            .set_document_key(Some("key1".to_string()));

        adopt_restored_draft(host.editor_state_mut());

        assert_eq!(host.editor_state().editor_ui.file_key, None);
    }
}
