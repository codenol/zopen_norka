//! The stale-autosave guard (issues #247 and #248), tested where it is decided.
//!
//! Split out of `files_routes_store_tests.rs` at the 800-line cap. Declared
//! with `#[path]` so the test name is unchanged.

use super::*;

/// The document the daemon drew, as a tab that took it would send it back:
/// both top-level frames the turn committed.
const TAKEN_BODY: &str = r#"{"document":{"version":"1.0.0","pages":[{"id":"n11","name":"Page 1","children":[
    {"id":"n492","type":"frame","children":[]},
    {"id":"n553","type":"frame","children":[]}
]}]}}"#;

/// The same document before the turn: the starter the behind tab still holds.
const STARTER_BODY: &str = r#"{"document":{"version":"1.0.0","pages":[{"id":"n11","name":"Page 1","children":[
    {"id":"n10","type":"frame","children":[]}
]}]}}"#;

/// A daemon holding a screen it drew itself: two top-level frames the turn
/// committed, which every tab that takes the document will hold.
fn daemon_drew(state: &mut WebCanvasState) {
    state
        .turn_result
        .note_turn_result(["n492".to_string(), "n553".to_string()]);
}

#[test]
fn a_quiet_autosave_cannot_put_a_stale_tab_over_a_turn_the_daemon_ran() {
    // Issue #247, measured: a turn drew a dashboard of 211 nodes, the daemon
    // held it, and the file on disk afterwards was the empty starter — because
    // the tab's own autosave carried its older copy and the daemon adopted it.
    // While the daemon holds a result the write does not carry, it is refused.
    let dir = TempDir::new("stale-autosave");
    let store = dir.open();
    let entry = seed(&store, "Screen", Some("userA"));
    let access = owner_access();
    let mut state = online_state(&store);
    // The daemon is holding this document — a tab has it open — which is the
    // state the save route requires before it will write anything.
    state.editor.editor_ui.file_key = Some(entry.key.clone());

    // The daemon has just drawn the screen; no tab has taken it yet.
    daemon_drew(&mut state);

    let autosave = handle(
        "POST",
        &format!("/api/files/{}/autosave", entry.key),
        STARTER_BODY,
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
        state.turn_result.is_ahead(),
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
        state.turn_result.is_ahead(),
        "a write that was refused does not settle the divergence"
    );

    // And the explicit save is NOT subject to the guard: the person saying
    // "this is what I see" is their call to make (#169), not a rule to guess.
    // Proven by the route reaching its own preconditions rather than the guard:
    // the refusal above is `missing document`, not `stale-autosave`.
    assert_eq!(error_code(&refused), "missing document");
}

#[test]
fn reading_the_document_does_not_reopen_the_door_for_a_behind_tab() {
    // The #248 regression, as a test. The guard used to come down on ANY
    // `GET /api/mcp/document` — and every reader issues one: the tab's own sync
    // poll, a second tab, an outside observer watching a run. So a tab that
    // never took the screen could still write its older copy over it, and the
    // measured loss (230 nodes on `pages[0]`, then none) came back.
    let dir = TempDir::new("read-does-not-settle");
    let store = dir.open();
    let entry = seed(&store, "Screen", Some("userA"));
    let access = owner_access();
    let mut state = online_state(&store);
    state.editor.editor_ui.file_key = Some(entry.key.clone());
    daemon_drew(&mut state);

    // Someone reads the document — the observer in #248's run, or merely the
    // tab's own poll. That is not the tab taking it.
    let read = super::super::super::handle_web_canvas_request(
        "GET",
        "/api/mcp/document",
        "",
        &mut state,
        &access,
    );
    assert_eq!(read.status, "200 OK", "{}", read.body);
    assert!(
        state.turn_result.is_ahead(),
        "reading the document is not taking it"
    );

    // The behind tab's autosave is still refused.
    let autosave = handle(
        "POST",
        &format!("/api/files/{}/autosave", entry.key),
        STARTER_BODY,
        &mut state,
        &access,
    );
    assert_eq!(
        autosave.status, "409 Conflict",
        "a reader must not have opened the door: {}",
        autosave.body
    );
}

#[test]
fn a_tab_that_took_the_screen_may_write_it_back() {
    // The other half of the guard: a tab that DID take the document holds the
    // turn's own nodes, so its autosave is the document plus whatever the
    // person changed — it must land, and settle the divergence.
    let dir = TempDir::new("taken-write-lands");
    let store = dir.open();
    let entry = seed(&store, "Screen", Some("userA"));
    let access = owner_access();
    let mut state = online_state(&store);
    state.editor.editor_ui.file_key = Some(entry.key.clone());
    daemon_drew(&mut state);

    let autosave = handle(
        "POST",
        &format!("/api/files/{}/autosave", entry.key),
        TAKEN_BODY,
        &mut state,
        &access,
    );
    assert_eq!(
        autosave.status, "200 OK",
        "a tab that took the screen may write it back: {}",
        autosave.body
    );
    assert!(
        !state.turn_result.is_ahead(),
        "an accepted write settles the divergence"
    );
}
