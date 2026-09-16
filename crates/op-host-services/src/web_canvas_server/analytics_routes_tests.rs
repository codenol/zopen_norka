//! The analytics routes against a real store: what an asset is, who may touch
//! it, and what deleting one leaves behind.
//!
//! The store itself is proved beside it (`analytics_store_tests`); what these
//! drive is the request — the path shape, the ownership decision, the roles the
//! matrix names, and the answer a panel reads.

use super::*;
use crate::document_store::DocumentEntry;
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use crate::web_canvas_server::{
    handle_web_canvas_request, RequestAccess, ServeMode, WebCanvasState,
};
use op_editor_core::access::RoleSet;
use op_editor_core::section::{AnalyticsLink, SectionDigest, SectionProperties};
use op_editor_core::{EditorState, NodeId, ShareLevel};

/// The daemon state of the local operator, backed by `store`.
fn local_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    state
}

/// The daemon state of an online tenant, backed by `store`.
fn tenant_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(store.clone());
    state
}

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

fn error_code(reply: &WebReply) -> String {
    body_json(reply)["error"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("no error field in {}", reply.body))
}

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

/// Create an asset through the route and hand back its key.
fn create_asset(
    state: &mut WebCanvasState,
    access: &RequestAccess<'_>,
    name: &str,
    md: &str,
) -> String {
    let reply = handle_web_canvas_request(
        "POST",
        "/api/analytics",
        &serde_json::json!({ "name": name, "markdown": md }).to_string(),
        state,
        access,
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    body_json(&reply)["asset"]["key"]
        .as_str()
        .expect("a key")
        .to_string()
}

#[test]
fn a_local_operator_loads_reads_and_lists_an_asset() {
    let dir = TempDir::new("analytics-routes-local-1");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(
        &mut state,
        &access,
        "  Checkout analytics  ",
        "# Why\n\nPeople abandon at the total.",
    );

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(read.status, "200 OK", "{}", read.body);
    let body = body_json(&read);
    assert_eq!(
        body["asset"]["name"], "Checkout analytics",
        "a name is stored trimmed: it is read in a list"
    );
    assert_eq!(body["markdown"], "# Why\n\nPeople abandon at the total.");
    assert!(
        body["digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the digest is a fingerprint of the text as it is now: {}",
        body["digest"]
    );

    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    let assets = body_json(&listed)["assets"].clone();
    let assets = assets.as_array().expect("a list of assets");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["key"], key.as_str());
    assert_eq!(assets[0]["size"].as_u64(), Some(35));
}

#[test]
fn writing_replaces_the_text_and_moves_the_digest() {
    let dir = TempDir::new("analytics-routes-local-2");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "first");

    let before = body_json(&handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    ))["digest"]
        .clone();

    let written = handle_web_canvas_request(
        "POST",
        &format!("/api/analytics/{key}"),
        r#"{"markdown":"second"}"#,
        &mut state,
        &access,
    );
    assert_eq!(written.status, "200 OK", "{}", written.body);
    let after = body_json(&written)["digest"].clone();
    assert_ne!(before, after, "the fingerprint follows the text");

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(body_json(&read)["markdown"], "second");
}

#[test]
fn renaming_keeps_the_address() {
    // The key is how a section's link points at the asset; a rename that moved
    // it would break every section that was built from this document.
    let dir = TempDir::new("analytics-routes-local-3");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "text");

    let renamed = handle_web_canvas_request(
        "POST",
        &format!("/api/analytics/{key}/rename"),
        r#"{"name":"Checkout analytics"}"#,
        &mut state,
        &access,
    );
    assert_eq!(renamed.status, "200 OK", "{}", renamed.body);
    assert_eq!(body_json(&renamed)["asset"]["key"], key.as_str());
    assert_eq!(body_json(&renamed)["asset"]["name"], "Checkout analytics");
}

#[test]
fn deleting_removes_the_asset_from_every_answer() {
    let dir = TempDir::new("analytics-routes-local-4");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "text");

    let deleted = handle_web_canvas_request(
        "DELETE",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(deleted.status, "200 OK", "{}", deleted.body);

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(read.status, "404 Not Found", "{}", read.body);
    assert_eq!(error_code(&read), "analytics-not-found");

    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);
    assert_eq!(
        body_json(&listed)["assets"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn a_store_that_cannot_answer_is_a_server_error_and_not_a_deletion() {
    // Issue #145's other half, and the contract the browser now reads: the one
    // status that means "there is no such analytics document" is a 404, and a
    // store that failed answers a 5xx. A 404 here would make every client that
    // treats it as GONE — the section panel, the canvas marks — report a
    // deletion nobody confirmed, because a row whose markdown is gone is a
    // damaged store, not an absent asset (`analytics_store::digest` says so).
    let dir = TempDir::new("analytics-routes-broken-store");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let key = create_asset(&mut state, &access, "Analytics", "text\n");
    std::fs::remove_file(crate::analytics_store::assets_dir(store.dir()).join(format!("{key}.md")))
        .expect("remove the markdown the record accounts for");

    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(read.status, "500 Internal Server Error", "{}", read.body);
    assert_ne!(
        error_code(&read),
        "analytics-not-found",
        "the store failed, and did not say the asset was deleted"
    );
}

#[test]
fn a_key_that_is_not_a_key_is_refused_before_a_file_is_built() {
    // The key is joined to a path, so this is the one check standing between a
    // pasted URL and the filesystem.
    let dir = TempDir::new("analytics-routes-local-5");
    let store = dir.open();
    let mut state = local_state(&store);
    let access = RequestAccess::local_operator(ServeMode::Local);

    let reply = handle_web_canvas_request(
        "GET",
        "/api/analytics/..%2Fsecrets",
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(error_code(&reply), "invalid key");
}

#[test]
fn a_caller_without_an_analytics_role_may_not_write() {
    // The matrix's `AnalyticsWrite` row is admin / UX-UI / analyst. A guest who
    // may edit the document is not automatically one of them.
    let dir = TempDir::new("analytics-routes-tenant-6");
    let store = dir.open();
    let mut state = tenant_state(&store);
    let guest = account("userB", &["frontend"]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Editor));

    let reply = handle_web_canvas_request(
        "POST",
        "/api/analytics",
        r#"{"name":"Analytics"}"#,
        &mut state,
        &access,
    );

    assert!(
        reply.status.starts_with("403"),
        "got {} {}",
        reply.status,
        reply.body
    );
}

#[test]
fn an_analyst_in_one_account_cannot_read_another_accounts_asset() {
    let dir = TempDir::new("analytics-routes-tenant-7");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let analyst = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &analyst, None);
    let key = create_asset(&mut state, &own, "Mine", "text");

    // The same role, a different account: holding the right to write analytics
    // is not holding this asset.
    let other = account("userB", &["analyst"]);
    let access = RequestAccess::online("userB", &other, None);
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(error_code(&reply), "analytics-not-yours");
}

#[test]
fn an_account_sees_only_its_own_assets() {
    let dir = TempDir::new("analytics-routes-tenant-8");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let first = account("userA", &["analyst"]);
    let access = RequestAccess::online("userA", &first, None);
    create_asset(&mut state, &access, "Mine", "text");

    let second = account("userB", &["analyst"]);
    let access = RequestAccess::online("userB", &second, None);
    let listed = handle_web_canvas_request("GET", "/api/analytics", "", &mut state, &access);

    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    assert_eq!(
        body_json(&listed)["assets"].as_array().map(Vec::len),
        Some(0),
        "one directory holds every account's assets; the list is what keeps them apart"
    );
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

/// A section whose only content is a link to `asset_key`.
fn linked_to(asset_key: &str, name: &str) -> SectionProperties {
    SectionProperties {
        analytics: vec![AnalyticsLink::new(
            asset_key,
            name,
            SectionDigest::of_text("the markdown"),
            SectionDigest::of_text("the screens"),
            1_700_000_000,
            Some("userA"),
        )],
        ..SectionProperties::empty()
    }
}

/// Store `properties` as the section `frame-1` of `document`.
fn link_section(store: &DocumentDb, document: &str, properties: &SectionProperties) {
    crate::section_store::save(store, document, &NodeId::new("frame-1"), properties)
        .expect("write the section");
}

#[test]
fn a_visitor_who_may_read_the_document_reads_the_analytics_its_section_links() {
    // Issue #110, the half that was missing. A section names the analytics it
    // was built from, and the reader the feature exists for — somebody who was
    // given the document and not the workspace — has to be able to fetch the
    // markdown behind that name. Otherwise the panel offers a link it cannot
    // resolve and the one click from screen to reasoning stops short.
    let dir = TempDir::new("analytics-routes-vouched-read");
    let store = dir.open();
    let mut state = tenant_state(&store);
    let markdown = "# Why\n\nPeople abandon the basket at the total.";

    let owner = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &owner, None);
    let key = create_asset(&mut state, &own, "Checkout analytics", markdown);
    let document = seed(&store, "Work", Some("userA"));
    link_section(
        &store,
        &document.key,
        &linked_to(&key, "Checkout analytics"),
    );

    // A guest with the weakest grant there is: may view this one document, and
    // holds no role at all.
    let guest = account("userB", &[]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Viewer))
        .on_document(Some(&document.key));
    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(read.status, "200 OK", "{}", read.body);
    let body = body_json(&read);
    assert_eq!(body["markdown"], markdown);
    assert_eq!(body["asset"]["name"], "Checkout analytics");
    assert!(
        body["digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "a reader is given the fingerprint a link is compared against: {}",
        body["digest"]
    );
}

#[test]
fn a_document_that_does_not_link_the_asset_does_not_vouch_for_a_reader() {
    // Reading follows the LINK, not the key: being on the access list of some
    // document is not being given every asset its account ever loaded.
    let dir = TempDir::new("analytics-routes-not-linked");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let owner = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &owner, None);
    let key = create_asset(&mut state, &own, "Checkout analytics", "text");
    let document = seed(&store, "Work", Some("userA"));
    // The document has a section; it links something else.
    link_section(
        &store,
        &document.key,
        &linked_to("another-key", "Elsewhere"),
    );

    let guest = account("userB", &[]);
    let access = RequestAccess::online("userA", &guest, Some(ShareLevel::Viewer))
        .on_document(Some(&document.key));
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(error_code(&reply), "analytics-not-yours");
}

#[test]
fn a_link_in_my_own_document_does_not_vouch_for_somebody_elses_asset() {
    // The escalation this rule must not open. A link is just a key in a body:
    // if any document that references an asset vouched for its reader, an
    // account holding the right to write sections could paste a guessed key
    // into a document of its own and read whatever it named. So the document
    // that vouches must belong to the same account as the asset — and here it
    // does not, so nothing is vouched for.
    let dir = TempDir::new("analytics-routes-cross-account-link");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let victim = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &victim, None);
    let key = create_asset(&mut state, &own, "Somebody else's analytics", "secret");

    // userB's own document, linking the key it guessed.
    let document = seed(&store, "Mine", Some("userB"));
    link_section(&store, &document.key, &linked_to(&key, "Stolen"));

    let thief = account("userB", &["admin"]);
    let access = RequestAccess::online("userB", &thief, None).on_document(Some(&document.key));
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(error_code(&reply), "analytics-not-yours");
}

#[test]
fn a_reader_the_named_document_is_not_shared_with_is_not_vouched_for() {
    // The refusal direction of the same rule: a carrier that names a document
    // the caller is not on the access list of reaches nothing. The admit loop
    // refuses such a request before a route sees it; this is the same answer
    // asked at the route, so a carrier built without a grant cannot fall open.
    let dir = TempDir::new("analytics-routes-stranger");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let owner = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &owner, None);
    let key = create_asset(&mut state, &own, "Checkout analytics", "text");
    let document = seed(&store, "Work", Some("userA"));
    link_section(
        &store,
        &document.key,
        &linked_to(&key, "Checkout analytics"),
    );

    let stranger = account("userB", &["analyst"]);
    let access = RequestAccess::online("userA", &stranger, None).on_document(Some(&document.key));
    let reply = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );

    assert_eq!(reply.status, "403 Forbidden", "{}", reply.body);
    assert_eq!(error_code(&reply), "analytics-not-yours");
}

#[test]
fn being_vouched_for_a_read_is_not_authority_over_the_asset() {
    // A reader's share of a document is not a share of the asset: the write
    // rules are unchanged, and an analyst who may write analytics in general
    // still may not rewrite somebody else's asset.
    let dir = TempDir::new("analytics-routes-vouched-write");
    let store = dir.open();
    let mut state = tenant_state(&store);

    let owner = account("userA", &["analyst"]);
    let own = RequestAccess::online("userA", &owner, None);
    let key = create_asset(&mut state, &own, "Checkout analytics", "first");
    let document = seed(&store, "Work", Some("userA"));
    link_section(
        &store,
        &document.key,
        &linked_to(&key, "Checkout analytics"),
    );

    // The analyst holds the role the matrix names for writing analytics, and
    // may read this document — and is still not the asset's owner.
    let analyst = account("userB", &["analyst"]);
    let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Viewer))
        .on_document(Some(&document.key));
    let written = handle_web_canvas_request(
        "POST",
        &format!("/api/analytics/{key}"),
        r#"{"markdown":"rewritten"}"#,
        &mut state,
        &access,
    );
    assert_eq!(written.status, "403 Forbidden", "{}", written.body);
    assert_eq!(error_code(&written), "analytics-not-yours");

    let deleted = handle_web_canvas_request(
        "DELETE",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &access,
    );
    assert_eq!(deleted.status, "403 Forbidden", "{}", deleted.body);

    // And the text is untouched by either attempt.
    let read = handle_web_canvas_request(
        "GET",
        &format!("/api/analytics/{key}"),
        "",
        &mut state,
        &own,
    );
    assert_eq!(body_json(&read)["markdown"], "first");
}
