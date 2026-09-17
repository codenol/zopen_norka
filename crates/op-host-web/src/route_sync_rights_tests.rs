//! What an open answer says about writing the document (#43).

use super::parse::can_write_from_open;

#[test]
fn the_open_answer_decides_who_may_write() {
    assert_eq!(
        can_write_from_open(&serde_json::json!({ "ok": true, "canWrite": true })),
        Some(true)
    );
    assert_eq!(
        can_write_from_open(&serde_json::json!({ "ok": true, "canWrite": false })),
        Some(false),
        "a reader is told so with the open, not by a refused push"
    );
}

#[test]
fn an_answer_that_does_not_say_leaves_the_question_open() {
    // An older daemon, or a local one with no accounts to decide about.
    assert_eq!(
        can_write_from_open(&serde_json::json!({ "ok": true, "version": 3 })),
        None
    );
    assert_eq!(
        can_write_from_open(&serde_json::json!({ "ok": true, "canWrite": null })),
        None
    );
    assert_eq!(
        can_write_from_open(&serde_json::json!({ "ok": true, "canWrite": "yes" })),
        None,
        "a string is not a boolean"
    );
}
