//! The autosave half of the turn-result guard (issues #247/#248), kept beside
//! the save route for the 800-line cap and tested where it is decided.
//!
//! `save_document` writes what it is given — "save means what I see" (#169),
//! and that is right for a person pressing Save. It is wrong for an autosave
//! from a tab that never took the document the daemon drew: that body is an
//! older copy, and adopting it put the starter back over a fresh screen, in the
//! file AND in the daemon's memory.
//!
//! So a quiet write is refused with `stale-autosave` while the daemon holds a
//! result the write does not carry. See [`super::turn_result_guard`] for the
//! sequence as it was measured, and for why the guard is settled by the write's
//! CONTENT rather than by anyone reading the document.

use super::files_routes::WriteKind;
use super::*;

/// The refusal a stale autosave gets, or `None` when the write may proceed.
///
/// `body` is the raw request body. A body that does not parse is left to the
/// save path, which reports it with its own error rather than this route
/// inventing a verdict; an empty body is "write the daemon's own document",
/// which cannot be stale.
pub(super) fn stale_autosave_refusal(
    state: &WebCanvasState,
    kind: WriteKind,
    body: &str,
) -> Option<WebReply> {
    if !matches!(kind, WriteKind::Quiet) || body.is_empty() || !state.turn_result.is_ahead() {
        return None;
    }
    let carries_the_result = super::turn_result_guard::top_level_ids_in_body(body)
        .is_some_and(|incoming| !state.turn_result.refuses(&incoming));
    if carries_the_result {
        return None;
    }
    Some(WebReply {
        status: "409 Conflict",
        body: serde_json::json!({
            "ok": false,
            "error": "stale-autosave",
            "message": "the daemon holds a newer copy of this document than this tab; \
                        reload the page to take it before autosaving",
        })
        .to_string(),
    })
}
