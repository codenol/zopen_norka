//! What a status code from the analytics route means for a link (#145).
//!
//! The rule itself is `op_editor_core::section::read_outcome`'s and is tested
//! there; what is tested here is that BOTH readers of this route — the canvas
//! marks and the selected section's panel — go through it, and that a read
//! which did not complete is never spoken of as a deletion. That is the bug
//! this file exists for: a 5xx used to arrive as `Answered(None)`, which is the
//! same value a 404 produces, and the section was drawn with the octagon that
//! means "the analytics document is gone".

use super::answer::{mark_answer, resolution, MarkAnswer, Resolution};
use op_editor_core::section::{refused_link_state, unchecked_link_state, LinkState, SectionDigest};

/// A link, for the state a read's answer produces.
fn link() -> op_editor_core::section::AnalyticsLink {
    op_editor_core::section::AnalyticsLink::new(
        "k1",
        "Checkout analytics",
        SectionDigest::of_text("then"),
        SectionDigest::of_text("screens"),
        1,
        None,
    )
}

#[test]
fn a_server_error_is_not_a_deletion() {
    // The store failed to answer. That says nothing about the document, and the
    // section must not be told it was deleted.
    let answer = mark_answer(500, r#"{"ok":false,"error":"analytics-io"}"#);
    assert!(
        matches!(answer, MarkAnswer::Failed),
        "a 5xx establishes nothing, not an absent document"
    );
    let state = unchecked_link_state(Some(&link()));
    assert_eq!(state, LinkState::CheckFailed);
    assert_ne!(
        state,
        LinkState::AssetMissing,
        "a deletion nobody confirmed must never be claimed"
    );
    // The panel's reader answers the same 5xx the same way.
    assert!(matches!(
        resolution(500, r#"{"ok":false,"error":"analytics-io"}"#),
        Resolution::Failed
    ));
}

#[test]
fn only_a_404_is_read_as_a_deletion() {
    // The one status that confirms it, and the one the octagon belongs to.
    let answer = mark_answer(404, r#"{"ok":false,"error":"analytics-not-found"}"#);
    assert!(matches!(answer, MarkAnswer::Answered(None)));
    assert!(matches!(
        resolution(404, r#"{"ok":false,"error":"analytics-not-found"}"#),
        Resolution::Answered(None)
    ));
}

#[test]
fn a_reader_may_not_fetch_it_is_still_a_refusal_and_not_a_failure() {
    // #110's case, kept: a 403 is a fact about the reader, with its own
    // sentence and its own padlock, and it must not become "could not check".
    let answer = mark_answer(403, r#"{"ok":false,"error":"analytics-not-yours"}"#);
    assert!(matches!(answer, MarkAnswer::Refused));
    assert!(matches!(
        resolution(403, r#"{"ok":false,"error":"analytics-not-yours"}"#),
        Resolution::Refused
    ));
    assert_eq!(
        refused_link_state(Some(&link())),
        LinkState::NotReadable,
        "and the state it produces is the refusal, not the failed check"
    );
}

#[test]
fn a_request_that_never_arrived_establishes_nothing() {
    // A transport that could not connect reports no status at all. The section
    // used to be marked "gone" for a daemon that was simply not running.
    for status in [0, 502, 503, 429, 400] {
        let answer = mark_answer(status, "");
        assert!(
            matches!(answer, MarkAnswer::Failed),
            "{status} must not be read as an absent document"
        );
        assert!(
            matches!(resolution(status, ""), Resolution::Failed),
            "{status}"
        );
    }
}

#[test]
fn a_success_with_no_digest_in_it_establishes_nothing_either() {
    // A 200 whose body carries no digest is not a deletion: the store answered,
    // and what it answered with is not the fingerprint this needed.
    assert!(matches!(mark_answer(200, "{}"), MarkAnswer::Failed));
    assert!(matches!(mark_answer(200, "<html>"), MarkAnswer::Failed));
    assert!(matches!(
        resolution(200, r#"{"ok":true}"#),
        Resolution::Failed
    ));
}

#[test]
fn a_success_with_a_digest_carries_it() {
    match mark_answer(200, r#"{"digest":"ab12"}"#) {
        MarkAnswer::Answered(Some(digest)) => {
            assert_eq!(digest, SectionDigest::of_hex("ab12"));
        }
        _ => panic!("a 200 with a digest in it is the one answer that carries one"),
    }
}
