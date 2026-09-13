//! The stored-document routes against a real database.
//!
//! `files_routes_access_tests` proves the gate, and stops at an invalid key
//! before the store is reached; these prove the other half — that an allowed
//! caller's list, rename and delete are answered by the database, that the JSON
//! index the daemon used to keep is not written any more, and that the owner
//! recorded on a row is what keeps one account's documents out of another
//! account's list and off another account's key.
//!
//! The store is installed on the state directly (`WebCanvasState::documents`)
//! rather than through `NORKA_DOCUMENTS_DIR`: a test that set the variable
//! would race every other test in this binary that resolves a directory, and
//! what these check is the wiring, not the resolver.

use super::*;
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use op_editor_core::access::RoleSet;

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

fn error_code(reply: &WebReply) -> String {
    serde_json::from_str::<serde_json::Value>(&reply.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
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

/// Online state for `owner`'s workspace, backed by `store`'s directory.
fn online_state(store: &DocumentDb) -> WebCanvasState {
    let mut state = WebCanvasState::new_for_tenant(EditorState::starter(), 3102);
    state.documents = Some(store.clone());
    state
}

/// Seed a document belonging to `owner`, with a real file behind it.
fn seed(store: &DocumentDb, name: &str, owner: Option<&str>) -> document_store::DocumentEntry {
    document_store::create_with(store, Some(name), owner, |path| {
        crate::doc_io::save_to_path(&op_pen_loader::new_skala_editor_state(), path)
            .map_err(|error| DocumentStoreError::Io(format!("save {}: {error}", path.display())))
    })
    .expect("seed a document")
}

fn keys_of(reply: &WebReply) -> Vec<String> {
    body_json(reply)["files"]
        .as_array()
        .unwrap_or_else(|| panic!("no files array in {}", reply.body))
        .iter()
        .filter_map(|file| file["key"].as_str().map(str::to_string))
        .collect()
}

/// Drive one request on an online state whose store is `store`.
fn serve_online(
    store: &DocumentDb,
    access: &RequestAccess<'_>,
    method: &str,
    path: &str,
    body: &str,
) -> WebReply {
    handle(method, path, body, &mut online_state(store), access)
}

#[test]
fn an_allowed_caller_lists_and_renames_what_the_database_holds() {
    let dir = TempDir::new("routes-store");
    let store = dir.open();
    let entry = document_store::create_with(&store, Some("Before"), None, |path| {
        std::fs::write(path, b"body")
            .map_err(|error| DocumentStoreError::Io(format!("write: {error}")))
    })
    .expect("seed a document");

    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    let access = RequestAccess::local_operator(ServeMode::Local);

    let listed = handle("GET", "/api/files", "", &mut state, &access);
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    let files = body_json(&listed);
    assert_eq!(files["ok"], true);
    assert_eq!(files["files"][0]["key"], entry.key.as_str());
    assert_eq!(files["files"][0]["name"], "Before");

    let renamed = handle(
        "POST",
        &format!("/api/files/{}/rename", entry.key),
        r#"{"name":"After"}"#,
        &mut state,
        &access,
    );
    assert_eq!(renamed.status, "200 OK", "{}", renamed.body);
    assert_eq!(body_json(&renamed)["file"]["name"], "After");
    // The rename reached the row, not a file on the side.
    assert_eq!(document_store::list(&store).expect("list")[0].name, "After");

    let deleted = handle(
        "DELETE",
        &format!("/api/files/{}", entry.key),
        "",
        &mut state,
        &access,
    );
    assert_eq!(deleted.status, "200 OK", "{}", deleted.body);
    assert!(document_store::list(&store).expect("list").is_empty());
    assert!(
        !dir.join("index.json").exists(),
        "the JSON index is not written any more"
    );
}

#[test]
fn creating_a_document_writes_the_file_the_row_and_the_last_opened_pointer() {
    let dir = TempDir::new("routes-create");
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(dir.open());
    let access = RequestAccess::local_operator(ServeMode::Local);

    let created = handle(
        "POST",
        "/api/files",
        r#"{"name":"Made here"}"#,
        &mut state,
        &access,
    );
    assert_eq!(created.status, "200 OK", "{}", created.body);
    let reply = body_json(&created);
    let key = reply["file"]["key"]
        .as_str()
        .unwrap_or_else(|| panic!("no key in {}", created.body))
        .to_string();
    assert_eq!(reply["file"]["name"], "Made here");

    let store = state.documents.clone().expect("store");
    let stored = document_store::list(&store).expect("list");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].key, key);
    assert!(
        document_store::path_for(store.dir(), &key)
            .expect("path")
            .is_file(),
        "the row and the file it describes arrive together"
    );
    assert_eq!(
        document_store::last_document(&store).map(|entry| entry.key),
        Some(key),
        "a new document is the one a restart comes back to"
    );
}

#[test]
fn an_invalid_key_neither_reads_nor_creates_the_store() {
    // The refusal happens before the store is touched, so a request carrying a
    // key this daemon could never have issued leaves the state's slot empty:
    // no database was opened and no directory was created.
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    let access = RequestAccess::local_operator(ServeMode::Local);
    let reply = handle(
        "GET",
        "/api/files/not-a-valid-key/thumb",
        "",
        &mut state,
        &access,
    );
    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert!(
        state.documents.is_none(),
        "the store must not have been opened"
    );
}

// ---------------------------------------------------------------------------
// One directory, several accounts.
// ---------------------------------------------------------------------------

/// The owner of a tenant, asking from their own workspace.
fn owner_access() -> RequestAccess<'static> {
    // Leaked on purpose: `RequestAccess` borrows the identity, and a test that
    // has to keep one alive beside the value it is passed to buys nothing.
    let identity: &'static ResolvedIdentity = Box::leak(Box::new(account("userA", &[])));
    RequestAccess::online("userA", identity, false)
}

#[test]
fn an_account_lists_only_its_own_documents() {
    // The list is where the shared directory would leak first: every row in it
    // is handed over whole — name, size, timestamps — so "list everything" is
    // not a smaller answer than "open everything".
    let dir = TempDir::new("list-owners");
    let store = dir.open();
    let mine = seed(&store, "Mine", Some("userA"));
    let theirs = seed(&store, "Theirs", Some("userB"));
    let ownerless = seed(&store, "Nobody's", None);

    let access = owner_access();
    let listed = serve_online(&store, &access, "GET", "/api/files", "");
    assert_eq!(listed.status, "200 OK", "{}", listed.body);
    assert_eq!(keys_of(&listed), vec![mine.key.clone()]);
    assert!(
        !listed.body.contains("Theirs") && !listed.body.contains("Nobody's"),
        "another account's document, and an unattributed one, are absent: {}",
        listed.body
    );

    // The other account's own list is the mirror image, not a superset.
    let other: &'static ResolvedIdentity = Box::leak(Box::new(account("userB", &[])));
    let theirs_listed = serve_online(
        &store,
        &RequestAccess::online("userB", other, false),
        "GET",
        "/api/files",
        "",
    );
    assert_eq!(keys_of(&theirs_listed), vec![theirs.key.clone()]);
    assert!(ownerless.owner_id.is_none());
}

#[test]
fn creating_a_document_records_the_verified_caller_as_its_owner() {
    let dir = TempDir::new("create-owner");
    let store = dir.open();
    let access = owner_access();
    let created = serve_online(
        &store,
        &access,
        "POST",
        "/api/files",
        r#"{"name":"Made online"}"#,
    );
    assert_eq!(created.status, "200 OK", "{}", created.body);
    let key = body_json(&created)["file"]["key"]
        .as_str()
        .unwrap_or_else(|| panic!("no key in {}", created.body))
        .to_string();

    let row = document_store::find(&store, &key)
        .expect("find")
        .expect("the row the create wrote");
    assert_eq!(
        row.owner_id.as_deref(),
        Some("userA"),
        "the owner is the identity the request was verified as, not the body"
    );
    // …and that is what puts it in the owner's list.
    assert_eq!(
        keys_of(&serve_online(&store, &access, "GET", "/api/files", "")),
        vec![key.clone()]
    );

    // Nothing in a shared deployment writes the operator's "reopen this" slot:
    // it is one row, and every account's open would overwrite it.
    assert_eq!(
        document_store::last_document(&store),
        None,
        "an online create does not touch the local operator's pointer"
    );
}

#[test]
fn a_document_belonging_to_another_account_is_refused_by_key() {
    // The key is the whole request. Nothing else in it says which document is
    // meant, and the caller's own lease is on their OWN workspace, so a row
    // that belongs to somebody else has to be refused here or not at all.
    let dir = TempDir::new("foreign-key");
    let store = dir.open();
    let theirs = seed(&store, "Theirs", Some("userB"));
    let ownerless = seed(&store, "Nobody's", None);

    let access = owner_access();
    for (method, path, body) in [
        (
            "POST",
            format!("/api/files/{}/open", theirs.key),
            String::new(),
        ),
        (
            "POST",
            format!("/api/files/{}/save", theirs.key),
            "{}".into(),
        ),
        (
            "POST",
            format!("/api/files/{}/autosave", theirs.key),
            String::new(),
        ),
        (
            "POST",
            format!("/api/files/{}/rename", theirs.key),
            r#"{"name":"x"}"#.into(),
        ),
        (
            "DELETE",
            format!("/api/files/{}", theirs.key),
            String::new(),
        ),
        (
            "GET",
            format!("/api/files/{}/thumb", theirs.key),
            String::new(),
        ),
        (
            "POST",
            format!("/api/files/{}/open", ownerless.key),
            String::new(),
        ),
    ] {
        let reply = serve_online(&store, &access, method, &path, &body);
        assert_eq!(
            reply.status, "403 Forbidden",
            "{method} {path}: {}",
            reply.body
        );
        assert_eq!(
            error_code(&reply),
            "tenant-not-shared",
            "{method} {path}: the answer a stranger gets, because it is the same statement"
        );
    }
    // The refused routes touched nothing.
    assert_eq!(
        document_store::find(&store, &theirs.key)
            .expect("find")
            .map(|entry| entry.name),
        Some("Theirs".to_string())
    );
}

#[test]
fn a_key_no_row_carries_is_not_found_online_rather_than_adopted() {
    // Locally the store adopts a file that arrived without a row; online the
    // same save would create an ownerless document nobody could ever list or
    // reach again — including the account that wrote it.
    let dir = TempDir::new("adopt-online");
    let store = dir.open();
    let key = crate::document_store::new_key();
    let body = r#"{"document":{"version":"1.0.0","children":[]}}"#;
    let access = owner_access();
    let refused = serve_online(
        &store,
        &access,
        "POST",
        &format!("/api/files/{key}/save"),
        body,
    );
    assert_eq!(refused.status, "404 Not Found", "{}", refused.body);
    assert!(document_store::list(&store).expect("list").is_empty());

    // The local operator's daemon still adopts it: a file dropped into their
    // own directory is a document.
    let mut state = WebCanvasState::new(EditorState::starter(), 3100);
    state.documents = Some(store.clone());
    let local = RequestAccess::local_operator(ServeMode::Local);
    let adopted = handle(
        "POST",
        &format!("/api/files/{key}/save"),
        "",
        &mut state,
        &local,
    );
    assert_eq!(adopted.status, "200 OK", "{}", adopted.body);
    assert_eq!(document_store::list(&store).expect("list").len(), 1);
}

#[test]
fn a_granted_visitor_reads_the_owners_document_and_cannot_write_it() {
    // The point of the share: a visitor admitted to the owner's workspace
    // addresses the owner's documents BY KEY, reads them without any role, and
    // is refused the writes their roles do not grant.
    let dir = TempDir::new("shared-read");
    let store = dir.open();
    let shared = seed(&store, "Shared", Some("userA"));
    let unshared = seed(&store, "Other", Some("userA"));

    let visitor: &'static ResolvedIdentity = Box::leak(Box::new(account("userB", &[])));
    let granted = RequestAccess::online("userA", visitor, true);
    let opened = serve_online(
        &store,
        &granted,
        "POST",
        &format!("/api/files/{}/open", shared.key),
        "",
    );
    assert_eq!(opened.status, "200 OK", "{}", opened.body);
    assert_eq!(
        body_json(&opened)["name"],
        "Shared",
        "the visitor reads the document the key names"
    );

    for (method, path, body) in [
        (
            "POST",
            format!("/api/files/{}/save", shared.key),
            "{}".to_string(),
        ),
        (
            "DELETE",
            format!("/api/files/{}", shared.key),
            String::new(),
        ),
        (
            "POST",
            format!("/api/files/{}/rename", shared.key),
            r#"{"name":"x"}"#.into(),
        ),
    ] {
        let reply = serve_online(&store, &granted, method, &path, &body);
        assert_eq!(
            reply.status, "403 Forbidden",
            "{method} {path}: {}",
            reply.body
        );
        assert_eq!(error_code(&reply), "read-only-role", "{method} {path}");
    }

    // A share is per account, not per document: the visitor reaches every
    // document of the workspace they were admitted to — and none of anyone
    // else's.
    let other = serve_online(
        &store,
        &granted,
        "POST",
        &format!("/api/files/{}/open", unshared.key),
        "",
    );
    assert_eq!(other.status, "200 OK", "{}", other.body);

    let third = seed(&store, "userC's", Some("userC"));
    let refused = serve_online(
        &store,
        &granted,
        "POST",
        &format!("/api/files/{}/open", third.key),
        "",
    );
    assert_eq!(refused.status, "403 Forbidden", "{}", refused.body);
    assert_eq!(error_code(&refused), "tenant-not-shared");

    // And a visitor with no grant addresses nothing, not even their own
    // workspace's rows — they have none, and the store's rows are not theirs.
    let stranger: &'static ResolvedIdentity = Box::leak(Box::new(account("userC", &["admin"])));
    let unshared_access = RequestAccess::online("userA", stranger, false);
    let refused = serve_online(
        &store,
        &unshared_access,
        "GET",
        &format!("/api/files/{}/thumb", shared.key),
        "",
    );
    assert_eq!(refused.status, "403 Forbidden", "{}", refused.body);
    assert_eq!(error_code(&refused), "tenant-not-shared");
}

#[test]
fn a_document_a_visitor_creates_belongs_to_the_visitor() {
    // The one place "the owner is the caller" and "the owner is the workspace"
    // could disagree: a visitor with an editing role creating a document while
    // they are in someone else's workspace. The creator owns it — the account
    // id is the identity the create was verified as — so it lands in THEIR
    // list and the workspace's owner does not see, or reach, a document they
    // did not make. Pinned here because the opposite rule is defensible too,
    // and a reviewer should find the decision rather than infer it.
    let dir = TempDir::new("visitor-create");
    let store = dir.open();
    let editor: &'static ResolvedIdentity = Box::leak(Box::new(account("userB", &["ux_ui"])));
    let granted = RequestAccess::online("userA", editor, true);

    let created = serve_online(
        &store,
        &granted,
        "POST",
        "/api/files",
        r#"{"name":"Visitor's own"}"#,
    );
    assert_eq!(created.status, "200 OK", "{}", created.body);
    let key = body_json(&created)["file"]["key"]
        .as_str()
        .unwrap_or_else(|| panic!("no key in {}", created.body))
        .to_string();
    assert_eq!(
        document_store::find(&store, &key)
            .expect("find")
            .and_then(|entry| entry.owner_id),
        Some("userB".to_string())
    );

    // The visitor's own list carries it; the owner's does not.
    assert_eq!(
        keys_of(&serve_online(&store, &granted, "GET", "/api/files", "")),
        vec![key.clone()],
        "the creator's own list"
    );
    let owner_identity: &'static ResolvedIdentity = Box::leak(Box::new(account("userA", &[])));
    let owner = RequestAccess::online("userA", owner_identity, false);
    assert!(keys_of(&serve_online(&store, &owner, "GET", "/api/files", "")).is_empty());
    let refused = serve_online(
        &store,
        &owner,
        "POST",
        &format!("/api/files/{key}/open"),
        "",
    );
    assert_eq!(refused.status, "403 Forbidden", "{}", refused.body);
    assert_eq!(error_code(&refused), "tenant-not-shared");
}
