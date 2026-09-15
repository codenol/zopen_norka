//! The section routes against a real database: what a section says, who may
//! change it, and what a write does not touch.
//!
//! The subject matrix itself is proved next door (`section_rights_tests`) — and
//! deliberately not re-proved here. What these drive is the whole request
//! through the daemon's entry point: the gate on the key, the store, the
//! comparison that decides WHICH subject a body touches, and the answer shape a
//! panel reads.
//!
//! The store is installed on the state (`WebCanvasState::documents`) rather than
//! through `NORKA_DOCUMENTS_DIR`, for the reason the file routes give: a test
//! that set the variable would race every other test in this binary that
//! resolves a directory.

use super::*;
use crate::document_store::DocumentEntry;
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::{
    handle_web_canvas_request, RequestAccess, ServeMode, WebCanvasState,
};
use op_editor_core::access::RoleSet;
use op_editor_core::section::{AnalyticsLink, SectionDigest, SectionProperties, SectionSummary};
use op_editor_core::{EditorState, NodeId, ShareLevel};

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

fn error_code(reply: &WebReply) -> String {
    body_json(reply)["error"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

/// A verified account holding `roles`.
fn account(user_id: &str, roles: &[&str]) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: RoleSet::from_wire(roles),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

/// A stored document belonging to `owner`, with a real file behind it.
fn seed(store: &DocumentDb, name: &str, owner: Option<&str>) -> DocumentEntry {
    crate::document_store::create_with(store, Some(name), owner, |path| {
        crate::doc_io::save_to_path(&op_pen_loader::new_skala_editor_state(), path).map_err(
            |error| {
                crate::document_store::DocumentStoreError::Io(format!(
                    "save {}: {error}",
                    path.display()
                ))
            },
        )
    })
    .expect("seed a document")
}

fn local_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    state
}

fn tenant_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(store.clone());
    state
}

/// The collection route of a document's sections.
fn sections(entry: &DocumentEntry) -> String {
    format!("/api/files/{}/sections", entry.key)
}

/// One section's route.
fn section(entry: &DocumentEntry, node: &NodeId) -> String {
    format!("/api/files/{}/sections/{}", entry.key, node.as_str())
}

/// Properties with all four summary questions answered.
fn summary_properties() -> SectionProperties {
    SectionProperties {
        summary: SectionSummary {
            what_it_is: "Checkout".to_string(),
            where_to_look: "The basket screen".to_string(),
            use_cases: "Paying for a basket".to_string(),
            what_to_check: "The total matches the line items".to_string(),
        },
        ..SectionProperties::empty()
    }
}

#[test]
fn a_section_nobody_wrote_about_reads_back_empty_and_unstored() {
    let dir = TempDir::new("section-routes-empty");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");

    let reply = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);

    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    let body = body_json(&reply);
    assert_eq!(body["nodeId"], "frame-1");
    assert_eq!(
        body["stored"], false,
        "an unwritten section is not the same fact as an erased one"
    );
    assert_eq!(body["properties"]["summary"]["whatItIs"], "");
    assert_eq!(
        body["properties"]["analytics"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn a_write_is_read_back_and_appears_in_the_list() {
    let dir = TempDir::new("section-routes-write");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");
    let properties = summary_properties();

    let written = handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        &serde_json::to_string(&properties).expect("properties serialize"),
        &mut state,
        &access,
    );
    assert_eq!(written.status, "200 OK", "{}", written.body);
    assert_eq!(body_json(&written)["changed"], true);

    let read = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);
    let body = body_json(&read);
    assert_eq!(body["stored"], true);
    assert_eq!(body["properties"]["summary"]["whatItIs"], "Checkout");

    // The list is every section that says something — the empty ones are
    // absent because they have no row, which is not a failure.
    let listed = handle_web_canvas_request("GET", &sections(&entry), "", &mut state, &access);
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    let rows = body_json(&listed)["sections"].clone();
    let rows = rows.as_array().expect("a list of sections");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["nodeId"], "frame-1");
    assert!(rows[0]["updatedAt"].as_u64().is_some_and(|at| at > 0));
}

#[test]
fn writing_what_is_already_there_changes_nothing_and_asks_for_nothing() {
    let dir = TempDir::new("section-routes-idempotent");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");
    let body = serde_json::to_string(&summary_properties()).expect("properties serialize");

    let first =
        handle_web_canvas_request("POST", &section(&entry, &node), &body, &mut state, &access);
    assert_eq!(body_json(&first)["changed"], true);

    let second =
        handle_web_canvas_request("POST", &section(&entry, &node), &body, &mut state, &access);
    assert_eq!(second.status, "200 OK", "{}", second.body);
    assert_eq!(
        body_json(&second)["changed"],
        false,
        "an unchanged write is a no-op, not a second edit"
    );
}

#[test]
fn deleting_forgets_the_properties_and_leaves_the_route_answering() {
    let dir = TempDir::new("section-routes-delete");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");
    let body = serde_json::to_string(&summary_properties()).expect("properties serialize");
    handle_web_canvas_request("POST", &section(&entry, &node), &body, &mut state, &access);

    let removed =
        handle_web_canvas_request("DELETE", &section(&entry, &node), "", &mut state, &access);
    assert_eq!(removed.status, "200 OK", "{}", removed.body);
    assert_eq!(body_json(&removed)["removed"], true);

    let read = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);
    assert_eq!(read.status, "200 OK", "{}", read.body);
    assert_eq!(body_json(&read)["stored"], false);

    // Deleting again is not an error: the section is simply not saying
    // anything, which is the state the button is for.
    let again =
        handle_web_canvas_request("DELETE", &section(&entry, &node), "", &mut state, &access);
    assert_eq!(again.status, "200 OK", "{}", again.body);
    assert_eq!(body_json(&again)["removed"], false);
}

#[test]
fn a_node_the_document_does_not_have_reads_back_empty() {
    // The route addresses properties by the id of the frame that marks the
    // section, and the document is not opened to check that the frame exists:
    // the frames are the document's business, and a read that went looking for
    // one would make this table a second authority on the node tree.
    let dir = TempDir::new("section-routes-unknown-node");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);

    let reply = handle_web_canvas_request(
        "GET",
        &section(&entry, &NodeId::new("a-frame-that-is-not-there")),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    assert_eq!(body_json(&reply)["stored"], false);
}

#[test]
fn a_path_deeper_than_a_section_is_not_a_route() {
    let dir = TempDir::new("section-routes-deep-path");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);

    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/files/{}/sections//", entry.key),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "404 Not Found", "{}", reply.body);
}

#[test]
fn properties_that_are_not_this_shape_are_refused() {
    let dir = TempDir::new("section-routes-malformed");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");

    let reply = handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        r#"{"summary":"just a string"}"#,
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(error_code(&reply), "malformed-section-properties");
}

#[test]
fn a_visitor_may_read_what_a_section_says() {
    // The point of analytics is that somebody who was not in the conversation
    // can reach the reasoning, so reading follows the document and asks
    // nothing else.
    let dir = TempDir::new("section-routes-reader");
    let store = dir.open();
    let entry = seed(&store, "Work", Some("userA"));
    let mut state = tenant_state(&store);
    let owner = account("userA", &["admin"]);
    let access_as_owner = RequestAccess::online("userA", &owner, Some(ShareLevel::Editor));
    let node = NodeId::new("frame-1");
    let body = serde_json::to_string(&summary_properties()).expect("properties serialize");
    handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        &body,
        &mut state,
        &access_as_owner,
    );

    let guest = account("userB", &[]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Editor));
    let reply = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);

    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    assert_eq!(
        body_json(&reply)["properties"]["summary"]["whatItIs"],
        "Checkout"
    );
}

#[test]
fn a_visitor_may_not_write_the_summary_through_the_route() {
    // The right to change a summary is not the right to edit the document: it
    // belongs to whoever wrote the analytics, and a guest holding an edit
    // grant does not become that person. The route asks the SUBJECT question
    // after reading what is being replaced.
    let dir = TempDir::new("section-routes-guest-write");
    let store = dir.open();
    let entry = seed(&store, "Work", Some("userA"));
    let mut state = tenant_state(&store);
    let guest = account("userB", &[]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Editor));
    let node = NodeId::new("frame-1");

    let reply = handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        &serde_json::to_string(&summary_properties()).expect("properties serialize"),
        &mut state,
        &access,
    );

    assert!(
        reply.status.starts_with("403"),
        "a guest must be refused the summary, got {} {}",
        reply.status,
        reply.body
    );
}

#[test]
fn an_analyst_writes_the_summary_and_not_the_flow() {
    // The same route, two subjects, two answers — which is the whole reason the
    // decision is taken per subject rather than per route.
    let dir = TempDir::new("section-routes-analyst");
    let store = dir.open();
    let entry = seed(&store, "Work", Some("userA"));
    let mut state = tenant_state(&store);
    let analyst = account("userB", &["analyst"]);
    let node = NodeId::new("frame-1");

    let summary_ok = {
        let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Editor));
        handle_web_canvas_request(
            "POST",
            &section(&entry, &node),
            &serde_json::to_string(&summary_properties()).expect("properties serialize"),
            &mut state,
            &access,
        )
    };
    assert_eq!(summary_ok.status, "200 OK", "{}", summary_ok.body);

    let flow = SectionProperties {
        flows: vec![op_editor_core::section::UxFlow::new("main", "Main path")],
        ..SectionProperties::empty()
    };
    let flow_refused = {
        let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Editor));
        handle_web_canvas_request(
            "POST",
            &section(&entry, &node),
            &serde_json::to_string(&flow).expect("properties serialize"),
            &mut state,
            &access,
        )
    };
    assert!(
        flow_refused.status.starts_with("403"),
        "the flow is the designer's, got {} {}",
        flow_refused.status,
        flow_refused.body
    );

    // And the summary the analyst wrote is still there: a refused write must
    // not have landed.
    let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Editor));
    let read = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);
    assert_eq!(
        body_json(&read)["properties"]["summary"]["whatItIs"],
        "Checkout"
    );
    assert_eq!(
        body_json(&read)["properties"]["flows"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn a_link_to_the_analytics_is_what_makes_a_section_accountable() {
    let dir = TempDir::new("section-routes-link");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");
    let linked = SectionProperties {
        analytics: vec![AnalyticsLink::new(
            "abc123",
            "Checkout analytics",
            SectionDigest::of_text("the markdown"),
            SectionDigest::of_text("the mockups"),
            1_700_000_000,
            None,
        )],
        ..SectionProperties::empty()
    };

    let reply = handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        &serde_json::to_string(&linked).expect("properties serialize"),
        &mut state,
        &access,
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);

    let read = handle_web_canvas_request("GET", &section(&entry, &node), "", &mut state, &access);
    let body = body_json(&read);
    assert_eq!(body["properties"]["analytics"][0]["key"], "abc123");
    // The digest is the fingerprint the link was made with, not a caption: the
    // whole point of the section is that it can say the analytics has moved
    // since.
    assert_eq!(
        body["properties"]["analytics"][0]["digest"],
        serde_json::json!(SectionDigest::of_text("the markdown"))
    );
}

#[test]
fn the_route_is_not_a_way_into_the_document() {
    // A section's properties are not the document's content: writing them must
    // not move the version a client polls for, or a collaboration session would
    // see an edit that never touched the canvas.
    let dir = TempDir::new("section-routes-version");
    let store = dir.open();
    let entry = seed(&store, "Work", None);
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let node = NodeId::new("frame-1");
    let before = state.version;

    handle_web_canvas_request(
        "POST",
        &section(&entry, &node),
        &serde_json::to_string(&summary_properties()).expect("properties serialize"),
        &mut state,
        &access,
    );

    assert_eq!(state.version, before);
}
