//! The stale-autosave guard (issue #247), tested where it is decided.
//!
//! Split out of `files_routes_store_tests.rs` at the 800-line cap. Declared
//! with `#[path]` so the test name is unchanged.

use super::*;

#[test]
fn a_quiet_autosave_cannot_put_a_stale_tab_over_a_turn_the_daemon_ran() {
    // Issue #247, measured: a turn drew a dashboard of 211 nodes, the daemon
    // held it, and the file on disk afterwards was the empty starter — because
    // the tab's own autosave carried its older copy and the daemon adopted it.
    // While the daemon is ahead of the tabs, that write is refused.
    let dir = TempDir::new("stale-autosave");
    let store = dir.open();
    let entry = seed(&store, "Screen", Some("userA"));
    let access = owner_access();
    let mut state = online_state(&store);
    // The daemon is holding this document — a tab has it open — which is the
    // state the save route requires before it will write anything.
    state.editor.editor_ui.file_key = Some(entry.key.clone());

    // The daemon has just applied a turn of its own; no tab has taken it yet.
    state.daemon_document_ahead = true;

    let autosave = handle(
        "POST",
        &format!("/api/files/{}/autosave", entry.key),
        r#"{"pages":[]}"#,
        &mut state,
        &access,
    );
    assert_eq!(
        autosave.status, "409 Conflict",
        "a stale autosave must not be written: {}",
        autosave.body
    );
    assert_eq!(error_code(&autosave), "stale-autosave");
    assert!(
        state.daemon_document_ahead,
        "a refused write leaves the daemon's copy in place"
    );

    // The guard comes down only when a write is actually TAKEN. A refusal —
    // any refusal, including this route's own preconditions — must leave it
    // standing, or one rejected request would reopen the door the guard exists
    // to close.
    let refused = handle(
        "POST",
        &format!("/api/files/{}/save", entry.key),
        r#"{"pages":[]}"#,
        &mut state,
        &access,
    );
    assert_ne!(refused.status, "200 OK", "{}", refused.body);
    assert!(
        state.daemon_document_ahead,
        "a write that was refused does not settle the divergence"
    );

    // And the explicit save is NOT subject to the guard: the person saying
    // "this is what I see" is their call to make (#169), not a rule to guess.
    // Proven by the route reaching its own preconditions rather than the guard:
    // the refusal above is `missing document`, not `stale-autosave`.
    assert_eq!(error_code(&refused), "missing document");
}
