//! The launch-time recovery offer (issue #26).
//!
//! A document with no server key — an untitled screen — has no file to save
//! into, so autosave writes it into the daemon's single draft slot instead
//! (`op_host_web::web_autosave`). On the next launch that draft is offered
//! back with a banner carrying two answers: take it, or let it go.
//!
//! The state here is deliberately *three plain fields and no timer*. The
//! question is asked once per page and answered once; everything else — when
//! to ask, what the server says, what the answer costs — belongs to the host.
//! Keeping the widget layer ignorant of all three is what lets paint and
//! hit-test derive from the same state without either knowing about a socket.

use crate::editor_ui_state::EditorUiState;

/// What the daemon's draft slot held when it was last asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryDraft {
    /// Unix seconds the daemon wrote the draft.
    ///
    /// Carried rather than turned into a sentence here: the age is a
    /// presentation decision, and `EditorUiState` outlives any one locale.
    pub saved_at: u64,
    /// Draft size in bytes.
    ///
    /// Kept beside the stamp because both come from the same answer and both
    /// describe the same thing: how much unrecovered work is at stake.
    pub size: u64,
}

/// What a press on the banner asked the host to do.
///
/// The widget layer owns no transport, so a press can only record the ask;
/// the browser host drains it on the next frame and makes the request — the
/// same hand-off the file screen uses for open / rename / delete, and the
/// reason this is an enum in *core* state rather than a call in the widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryRequest {
    /// Adopt the draft as the open document (`POST /api/recovery/restore`).
    Restore,
    /// Drop the draft (`DELETE /api/recovery`).
    Discard,
}

impl EditorUiState {
    /// Record what the launch probe found.
    ///
    /// Ignored once the user has answered: a probe is a question, and a
    /// question the user already answered must not be asked again on a later
    /// frame (a re-probe, a route change, or a host that retries on wake).
    pub fn note_recovery_probe(&mut self, draft: Option<RecoveryDraft>) {
        if self.recovery_answered {
            return;
        }
        self.recovery_draft = draft;
    }

    /// The offer the banner should paint right now, if any.
    ///
    /// Reads the answer flag as well as the slot so a caller can never show a
    /// question the user has already answered, whatever order the two were
    /// written in.
    pub const fn visible_recovery_offer(&self) -> Option<RecoveryDraft> {
        if self.recovery_answered {
            None
        } else {
            self.recovery_draft
        }
    }

    /// Take the offer off the screen for the rest of this page's life.
    ///
    /// Called both when the user answers *and* when a restore succeeds, which
    /// is why it latches: the banner is a question, and the two ways of
    /// answering it are the only two. A request the daemon rejects leaves the
    /// work in its slot and is offered again on the next launch — the user's
    /// answer is a page-lifetime statement, not a promise about the server.
    pub fn answer_recovery_offer(&mut self) {
        self.recovery_draft = None;
        self.recovery_answered = true;
    }

    /// Ask the host to act on the draft.
    ///
    /// Hides the offer in the same call: the banner is a question, and it has
    /// been answered by the time the request exists. The request itself is
    /// performed by the frame tick, so a press that arrives while the shell is
    /// borrowed cannot be lost.
    pub fn request_recovery(&mut self, request: RecoveryRequest) {
        self.recovery_request = Some(request);
        self.answer_recovery_offer();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> RecoveryDraft {
        RecoveryDraft {
            saved_at: 1_700_000_000,
            size: 4_096,
        }
    }

    #[test]
    fn a_fresh_editor_offers_nothing() {
        assert!(EditorUiState::new().visible_recovery_offer().is_none());
    }

    #[test]
    fn a_probe_that_finds_a_draft_puts_it_on_offer() {
        let mut ui = EditorUiState::new();
        ui.note_recovery_probe(Some(draft()));
        assert_eq!(ui.visible_recovery_offer(), Some(draft()));
    }

    #[test]
    fn a_probe_that_finds_nothing_offers_nothing() {
        let mut ui = EditorUiState::new();
        ui.note_recovery_probe(None);
        assert!(ui.visible_recovery_offer().is_none());
    }

    #[test]
    fn an_answer_latches_even_when_the_slot_still_holds_the_draft() {
        // The server's slot is not the state's business: a refused draft may
        // still be sitting in it (a DELETE that failed, a re-probe), and the
        // banner must not come back to a user who already said no.
        let mut ui = EditorUiState::new();
        ui.note_recovery_probe(Some(draft()));
        ui.answer_recovery_offer();
        assert!(ui.visible_recovery_offer().is_none());
        assert!(
            ui.recovery_draft.is_none(),
            "answering must also empty the slot, not merely hide it"
        );

        ui.note_recovery_probe(Some(draft()));
        assert!(
            ui.visible_recovery_offer().is_none(),
            "a later probe must not re-open an answered question"
        );
    }

    #[test]
    fn a_request_answers_the_offer_and_records_the_ask() {
        for request in [RecoveryRequest::Restore, RecoveryRequest::Discard] {
            let mut ui = EditorUiState::new();
            ui.note_recovery_probe(Some(draft()));
            ui.request_recovery(request);

            assert_eq!(ui.recovery_request, Some(request));
            assert!(
                ui.visible_recovery_offer().is_none(),
                "the question is answered the moment its answer is recorded"
            );
        }
    }
}
