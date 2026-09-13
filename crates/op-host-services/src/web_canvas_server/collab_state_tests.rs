//! Gateway + split-borrow coverage.
//!
//! These drive the *projection* into each phase directly rather than standing
//! up a real session: the gate reads only `phase` / `role` / `pending_edit`, so
//! a projected state exercises exactly the decision under test without a
//! network peer.

use op_editor_core::{
    AuthenticatedCollabSession, CollabConnectionPhase, CollabDocumentMutation, CollabEditSource,
    CollabGateAction, CollabPendingEditUi, CollabUiRole, EditorCommand, EditorState,
};

use super::super::WebCanvasState;

fn daemon() -> WebCanvasState {
    WebCanvasState::new(EditorState::starter(), 0)
}

fn in_session(state: &mut WebCanvasState, phase: CollabConnectionPhase, role: CollabUiRole) {
    state.editor.editor_ui.collab.set_authenticated_session(
        phase,
        AuthenticatedCollabSession {
            session_name: "studio".into(),
            role,
            share_endpoint: None,
        },
        Vec::new(),
    );
}

const DOCUMENT_EDIT: CollabGateAction =
    CollabGateAction::Document(CollabDocumentMutation::NodePropertyBatch);

#[test]
fn an_idle_daemon_passes_every_source_through_untouched() {
    let state = daemon();
    for source in [
        CollabEditSource::User,
        CollabEditSource::Ai,
        CollabEditSource::Mcp,
        CollabEditSource::Import,
        CollabEditSource::ExternalSync,
    ] {
        assert!(
            state.gate_daemon_mutation(DOCUMENT_EDIT, source).is_ok(),
            "{source:?} must be unaffected when there is no session"
        );
        assert!(state
            .gate_daemon_mutation(CollabGateAction::ReplaceDocument, source)
            .is_ok());
    }
    assert!(!state.collab_session_is_active());
}

#[test]
fn an_active_owner_may_write_but_ai_and_mcp_may_not() {
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    assert!(state.collab_session_is_active());

    assert!(state
        .gate_daemon_mutation(DOCUMENT_EDIT, CollabEditSource::User)
        .is_ok());
    for source in [CollabEditSource::Ai, CollabEditSource::Mcp] {
        let refusal = state
            .gate_daemon_mutation(DOCUMENT_EDIT, source)
            .expect_err("desktop refuses these during a session, so the daemon must too");
        assert_eq!(refusal.code(), "collab-active");
        assert_eq!(refusal.http_status(), "409 Conflict");
    }
}

#[test]
fn a_viewer_is_read_only_for_its_own_writes() {
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Viewer,
    );
    let refusal = state
        .gate_daemon_mutation(DOCUMENT_EDIT, CollabEditSource::User)
        .expect_err("a viewer cannot write");
    assert_eq!(refusal.code(), "collab-readonly");
}

#[test]
fn a_frozen_phase_reports_busy_rather_than_read_only() {
    for phase in [
        CollabConnectionPhase::Starting,
        CollabConnectionPhase::Joining,
        CollabConnectionPhase::Authenticating,
    ] {
        let mut state = daemon();
        state.editor.editor_ui.collab.set_phase(phase);
        let refusal = state
            .gate_daemon_mutation(DOCUMENT_EDIT, CollabEditSource::User)
            .expect_err("a transitioning session accepts no writes");
        assert_eq!(refusal.code(), "collab-busy", "{phase:?}");
    }
}

#[test]
fn a_pending_edit_reports_busy() {
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    state.editor.editor_ui.collab.pending_edit = CollabPendingEditUi::Submitting;
    let refusal = state
        .gate_daemon_mutation(DOCUMENT_EDIT, CollabEditSource::User)
        .expect_err("one edit at a time");
    assert_eq!(refusal.code(), "collab-busy");
}

#[test]
fn replacing_the_whole_document_is_refused_for_every_source_in_a_session() {
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    for source in [
        CollabEditSource::User,
        CollabEditSource::Ai,
        CollabEditSource::Mcp,
        CollabEditSource::ExternalSync,
    ] {
        let refusal = state
            .gate_daemon_mutation(CollabGateAction::ReplaceDocument, source)
            .expect_err("a whole-document swap cannot be sequenced");
        assert_eq!(refusal.code(), "collab-active", "{source:?}");
    }
}

#[test]
fn apply_gated_separates_a_refusal_from_a_no_op() {
    let mut state = daemon();
    // No session: the editor's own ack rides through untouched, whatever it is.
    // What matters is that it arrives as `Ok`, so the caller can still tell a
    // refusal from whatever the editor decided.
    assert!(
        state
            .apply_gated(EditorCommand::ClearSelection, CollabEditSource::Mcp)
            .is_ok(),
        "no session refuses nothing"
    );

    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    let insert = EditorCommand::InsertNode {
        kind: "rect".into(),
        name: "From MCP".into(),
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        fill_hex: None,
        target_parent: op_editor_core::NodeId::NONE,
        page_id: None,
    };
    let refusal = state
        .apply_gated(insert, CollabEditSource::Mcp)
        .expect_err("an MCP write during a session must be a typed refusal");
    assert_eq!(refusal.code(), "collab-active");
}

#[test]
fn the_split_borrow_yields_a_host_over_the_same_editor() {
    use op_editor_host_core::collab::CollaborationEditorHost;

    let mut state = daemon();
    state.editor.editor_ui.preserve_authored_geometry = true;

    let (_runtime, mut host) = state.collab_runtime_and_host();
    assert!(host.editor_state().editor_ui.preserve_authored_geometry);
    host.editor_state_mut().editor_ui.preserve_authored_geometry = false;

    assert!(
        !state.editor.editor_ui.preserve_authored_geometry,
        "the host must borrow the daemon's editor, not a copy of it"
    );
}

#[test]
fn the_host_seam_raises_the_dirty_flag_the_driver_reads() {
    use op_collab_host::CollabHost;

    let mut state = daemon();
    assert!(!state.collab.take_dirty());

    let (_runtime, mut host) = state.collab_runtime_and_host();
    host.mark_editor_state_dirty();

    assert!(state.collab.take_dirty());
    assert!(!state.collab.take_dirty(), "taking the flag clears it");
}

#[test]
fn ingest_refuses_rather_than_writing_when_no_capture_can_open() {
    let mut state = daemon();
    // A projected Active phase with no real session actor: the runtime cannot
    // open a capture, and the ingest must report that instead of writing
    // behind the session's back.
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    let before = state.editor.doc.clone();

    let prepared = op_editor_core::PreparedDocument::prepare(
        serde_json::from_value(serde_json::json!({
            "version": "1.0",
            "children": [{"type": "rectangle", "id": "n1", "x": 0, "y": 0, "width": 5, "height": 5}]
        }))
        .expect("valid document"),
    )
    .expect("validates");

    assert_eq!(
        state.ingest_document_in_session(prepared),
        crate::web_canvas_server::IngestOutcome::Rejected
    );
    assert_eq!(state.editor.doc, before, "a refused ingest writes nothing");
}

#[test]
fn the_projection_sequence_is_independent_of_the_document_version() {
    let mut state = daemon();
    assert_eq!(state.sse_tick().version, 0);
    assert_eq!(state.sse_tick().collab_seq, 0);

    // A presence-shaped change moves only `collabSeq`, so a client polling
    // `version` for document content is not made to refetch the document.
    state.collab.bump_seq();
    assert_eq!(state.sse_tick().version, 0);
    assert_eq!(state.sse_tick().collab_seq, 1);
}

// ---------------------------------------------------------------------------
// H3: `/api/file/save` replaces the editor, so it needs the same gate
// `open-recent` passes.
// ---------------------------------------------------------------------------

/// Drive the REST route table directly, which is where the gate lives.
fn save_reply(state: &mut WebCanvasState) -> crate::web_canvas_server::WebReply {
    crate::web_canvas_server::handle_local_request(
        "POST",
        "/api/file/save",
        r#"{"document":{"version":"1.0.0","children":[]}}"#,
        state,
    )
}

#[test]
fn saving_is_refused_for_an_owner_in_a_live_session() {
    // A save reply REPLACES `state.editor`, so it is a whole-document swap:
    // letting it through would leave the peers editing a document this daemon
    // no longer has.
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Owner,
    );
    let reply = save_reply(&mut state);
    assert_eq!(reply.status, "409 Conflict", "{}", reply.body);
    assert!(reply.body.contains("collab-active"), "{}", reply.body);
}

#[test]
fn saving_is_refused_for_a_guest_in_a_live_session() {
    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Editor,
    );
    let reply = save_reply(&mut state);
    assert_eq!(reply.status, "409 Conflict", "{}", reply.body);
}

#[test]
fn saving_outside_a_session_is_not_gated() {
    // The local daemon's normal path must be untouched: no session, no gate.
    // (It then fails on having no backing path, which is a different answer
    // from the collaboration refusal and is what this asserts.)
    let mut state = daemon();
    let reply = save_reply(&mut state);
    assert_ne!(reply.status, "409 Conflict", "{}", reply.body);
    assert!(!reply.body.contains("collab-active"), "{}", reply.body);
}

// ---------------------------------------------------------------------------
// H4: a rejected ingest must not read as success.
// ---------------------------------------------------------------------------

#[test]
fn a_rejected_ingest_is_distinguishable_from_an_accepted_one() {
    // The `bool` this replaces could not say whether the session took the
    // document, so a discarded push was answered 200 with a version bump and
    // the browser never learned to resync.
    use crate::web_canvas_server::IngestOutcome;
    assert_eq!(IngestOutcome::Committed.error_code(), None);
    assert_eq!(IngestOutcome::NoChange.error_code(), None);
    // Both rejections use the code the browser recovers from: its
    // `parse_push_conflict` only recognises `version-conflict`, so any other
    // code would leave the tab latched with no way to resync.
    assert_eq!(
        IngestOutcome::Rejected.error_code(),
        Some("version-conflict")
    );
    assert_eq!(IngestOutcome::Failed.error_code(), Some("version-conflict"));
}

#[test]
fn a_rejected_ingest_answers_409_with_the_authoritative_version() {
    // The browser's recovery only fires on `version-conflict` carrying a
    // `version` (`WebSyncClient::parse_push_conflict`). A 409 without one
    // leaves the tab latched with nothing to refetch from.
    use crate::web_canvas_server::IngestOutcome;
    for outcome in [IngestOutcome::Rejected, IngestOutcome::Failed] {
        let error = crate::web_canvas_server_error::WebCanvasError::IngestRejected(outcome, 42);
        assert_eq!(error.http_status(), "409 Conflict", "{outcome:?}");
        let reply = crate::web_canvas_server::collab_aware_error_reply_for_test(&error);
        let body: serde_json::Value = serde_json::from_str(&reply.body).expect("json");
        assert_eq!(body["ok"], false, "{outcome:?}");
        assert_eq!(body["error"], "version-conflict", "{outcome:?}");
        assert_eq!(body["version"], 42, "{outcome:?}");
        assert!(
            op_editor_core::web_sync::WebSyncClient::parse_push_conflict(&reply.body).is_some(),
            "{outcome:?}: the browser must be able to parse its recovery version"
        );
    }
}

#[test]
fn a_gated_push_still_owns_its_document_so_the_seed_is_released() {
    // The gate used to run AFTER `prepared.take()`, so a refusal left the
    // push without the document — and its `Drop` had nothing to release. This
    // pins the ordering by exercising it: a session that refuses the push must
    // not leave a seed behind.
    use crate::web_canvas_server::{PendingDocumentPush, ServeMode};

    let mut state = daemon();
    in_session(
        &mut state,
        CollabConnectionPhase::Active,
        CollabUiRole::Viewer,
    );

    let body = serde_json::json!({
        "document": {
            "version": "1.0.0",
            "children": [{
                "id": "n1", "type": "rectangle", "name": "gated",
                "x": 0, "y": 0, "width": 4, "height": 4,
            }],
            "imageThumbs": { "6161": "AQID" },
        },
        "sourceClientId": "s",
    })
    .to_string();

    let push = PendingDocumentPush::parse(&body, ServeMode::Local).expect("parses");
    // A viewer may not write, so the gate refuses before the document moves.
    let refused = state.apply_prepared_document_push(push, None);
    assert!(refused.is_err(), "a viewer's push must be refused");
}

/// Install an activated owner session over this daemon's editor.
///
/// The fixture comes from `op-collab-host`'s `test-support` feature, so the
/// ingest below runs the real begin -> install -> finish path instead of a
/// projected phase with no actor behind it.
fn with_owner_session(
    state: &mut WebCanvasState,
    session: (
        op_collab_host::CollabRuntime,
        op_collab_host::test_support::OwnerLaneGuard,
    ),
) -> op_collab_host::test_support::OwnerLaneGuard {
    let (runtime, guard) = session;
    state.collab.runtime = runtime;
    in_session(state, CollabConnectionPhase::Active, CollabUiRole::Owner);
    // Returned, not dropped: the guard owns the command lane's receiver, and
    // letting it go turns a full lane into a disconnected one — a different
    // failure with a different projection. The caller must hold it until the
    // ingest under test has finished.
    guard
}

/// Seed this daemon with the same document shape a push carries, so the
/// session's diff sees a node-property change rather than an unsupported
/// document-version change.
fn seed_baseline(state: &mut WebCanvasState, name: &str, thumb_id: &str) {
    use crate::web_canvas_server::{PendingDocumentPush, ServeMode};

    let body = seeded_body(name, thumb_id);
    let mut push = PendingDocumentPush::parse(&body, ServeMode::Local).expect("parses");
    let prepared = push.prepared.take().expect("a document push");
    state.editor.doc = prepared.into_document();
}

/// The node name the daemon's document currently carries.
fn node_name(state: &WebCanvasState) -> String {
    let node = serde_json::to_value(&state.editor.active_children()[0]).expect("node serialises");
    node["name"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_session_rejected_ingest_rolls_the_thumbnail_registry_back() {
    // Drives the REAL rejection: the pushed document changes `version`, which
    // `diff_supported` refuses, so the owner session rolls the document back
    // and reports `Rejected`. The predecessor of this test projected Active
    // with no actor, which made `begin_local_edit` refuse and returned
    // `Rejected` from the early-out branch above the rollback entirely - it
    // could never have caught a regression in the restore.
    use crate::web_canvas_server::{IngestOutcome, PendingDocumentPush, ServeMode};

    // Ids no other test uses, so this is immune to the process-global
    // registry being cleared in parallel.
    const BASELINE: u64 = 515_243_616;
    const KEPT: u64 = 515_243_617;
    const REFUSED: u64 = 515_243_618;

    let _registry = crate::web_canvas_server::lock_image_thumb_registry();
    let mut state = daemon();
    seed_baseline(&mut state, "before", &BASELINE.to_string());
    // The session is activated over the SAME document the daemon holds, so the
    // only thing the diff can object to is what this push actually changes.
    let baseline_doc = state.editor.doc.clone();
    let _lane = with_owner_session(
        &mut state,
        op_collab_host::test_support::owner_session(baseline_doc),
    );

    jian_ops_schema::image_thumbs::store_thumb(KEPT, vec![4, 5, 6]);

    // A document-version change is what `diff_supported` refuses outright.
    let body = seeded_body("refused", &REFUSED.to_string()).replace("1.0.0", "9.9.9");
    let mut push = PendingDocumentPush::parse(&body, ServeMode::Local).expect("parses");
    let prepared = push.prepared.take().expect("a document push");

    assert_eq!(
        state.ingest_document_in_session(prepared),
        IngestOutcome::Rejected,
        "an unsupported diff must come back as a session rejection"
    );
    assert_eq!(
        jian_ops_schema::image_thumbs::thumb_for(KEPT).as_deref(),
        Some(&[4u8, 5, 6][..]),
        "a rejected ingest must leave the pre-push registry as it found it"
    );
    assert!(
        jian_ops_schema::image_thumbs::thumb_for(REFUSED).is_none(),
        "the rolled-back document's thumbnails must roll back with it"
    );
    assert_eq!(
        node_name(&state),
        "before",
        "a rejected ingest must leave the document as the session found it"
    );
}

#[test]
fn a_standalone_fallback_failure_keeps_the_new_thumbnails() {
    // The counterpart: a delivery failure retires the session but KEEPS the
    // edit, so restoring the pre-push snapshot would leave the registry
    // describing a document that no longer exists. The predecessor of this
    // test only `matches!`-ed a constructed constant against itself.
    use crate::web_canvas_server::{IngestOutcome, PendingDocumentPush, ServeMode};

    const BASELINE: u64 = 515_243_619;
    const KEPT: u64 = 515_243_620;

    let _registry = crate::web_canvas_server::lock_image_thumb_registry();
    let mut state = daemon();
    seed_baseline(&mut state, "before", &BASELINE.to_string());
    let baseline_doc = state.editor.doc.clone();
    let _lane = with_owner_session(
        &mut state,
        op_collab_host::test_support::owner_session_with_saturated_command_lane(baseline_doc),
    );

    // Same document shape, renamed node: a supported diff, so the session gets
    // as far as broadcasting a commit - which is where the full lane bites.
    let body = seeded_body("after", &KEPT.to_string());
    let mut push = PendingDocumentPush::parse(&body, ServeMode::Local).expect("parses");
    let prepared = push.prepared.take().expect("a document push");

    assert_eq!(
        state.ingest_document_in_session(prepared),
        IngestOutcome::Failed,
        "an undeliverable commit must come back as a failure"
    );
    // The distinction that makes this a REAL delivery failure: a full lane is
    // `ResourceLimit`, a dropped receiver would be `Transport`. Asserting the
    // projection is what proves the lane guard above is doing its job.
    let failures: Vec<_> = state
        .collab
        .runtime
        .drain_status_events()
        .filter_map(|event| match event {
            op_collab_host::CollabStatusEvent::Failed(failure) => Some(failure),
            _ => None,
        })
        .collect();
    assert!(
        failures.contains(&op_collab_host::CollabRuntimeFailure::ResourceLimit),
        "the commit must have failed on a FULL lane, not a dead one: {failures:?}"
    );
    assert_eq!(
        node_name(&state),
        "after",
        "the standalone fallback keeps the edit the push carried"
    );
    assert!(
        jian_ops_schema::image_thumbs::thumb_for(KEPT).is_some(),
        "thumbnails must follow the kept document, not roll back without it"
    );
}

/// A push body carrying one embedded thumbnail.
fn seeded_body(name: &str, thumb_id: &str) -> String {
    serde_json::json!({
        "document": {
            "version": "1.0.0",
            "children": [{
                "id": "n1", "type": "rectangle", "name": name,
                "x": 0, "y": 0, "width": 4, "height": 4,
            }],
            "imageThumbs": { thumb_id: "AQID" },
        },
        "sourceClientId": "s",
    })
    .to_string()
}
