//! The turn-result guard (issues #247, #248), at the decision it makes.

use super::*;

fn ids(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

#[test]
fn a_tab_that_never_took_the_result_is_refused() {
    // The daemon drew a screen (root n492); the tab still holds the starter.
    let mut guard = TurnResultGuard::default();
    guard.note_turn_result(["n492".to_string()]);
    assert!(guard.is_ahead());
    assert!(
        guard.refuses(&ids(&["n10"])),
        "a copy without the turn's own root is the older copy"
    );
}

#[test]
fn a_tab_that_took_the_result_may_write_it_back() {
    // The same tab, after taking the document: it holds n492, and its autosave
    // is the document the daemon drew plus whatever the person changed.
    let mut guard = TurnResultGuard::default();
    guard.note_turn_result(["n492".to_string()]);
    assert!(!guard.refuses(&ids(&["n492", "n553"])));
}

#[test]
fn a_partial_copy_is_still_a_behind_tab() {
    // #248's run: the daemon had 230 nodes, a tab held 116 of them. Two roots
    // committed, only one taken — the write is refused.
    let mut guard = TurnResultGuard::default();
    guard.note_turn_result(["n492".to_string(), "n553".to_string()]);
    assert!(guard.refuses(&ids(&["n492"])));
}

#[test]
fn nothing_is_protected_until_the_daemon_draws_something() {
    let guard = TurnResultGuard::default();
    assert!(!guard.is_ahead());
    assert!(!guard.refuses(&ids(&["n10"])));
}

#[test]
fn the_latest_turn_replaces_an_earlier_one() {
    // What has to be held is the document as it stands NOW: a node an earlier
    // turn left behind, and this one removed, is not something to wait for.
    let mut guard = TurnResultGuard::default();
    guard.note_turn_result(["n1".to_string()]);
    guard.note_turn_result(["n2".to_string()]);
    assert!(guard.refuses(&ids(&["n1"])));
    assert!(!guard.refuses(&ids(&["n2"])));
}

#[test]
fn clearing_lets_the_next_write_land() {
    let mut guard = TurnResultGuard::default();
    guard.note_turn_result(["n492".to_string()]);
    guard.clear();
    assert!(!guard.is_ahead());
    assert!(!guard.refuses(&ids(&["n10"])), "the divergence is settled");
}

#[test]
fn ids_come_from_every_page_of_a_push_body() {
    // A tab whose active page differs from the daemon's still holds the result:
    // the walk collects the top level of every page, not only page 0.
    let body = r#"{"document":{"pages":[
        {"id":"p1","children":[{"id":"n492","children":[{"id":"n493"}]}]},
        {"id":"p2","children":[{"id":"n700"}]}
    ]}}"#;
    let found = top_level_ids_in_body(body).expect("a document");
    assert_eq!(found, ids(&["n492", "n700"]));
}

#[test]
fn an_unparsable_body_has_no_ids() {
    // The save path reports a malformed body itself, with its own error; the
    // guard must not invent a verdict on top of that.
    assert!(top_level_ids_in_body("not json").is_none());
    assert!(top_level_ids_in_body(r#"{"document":{"nope":true}}"#).is_none());
}
