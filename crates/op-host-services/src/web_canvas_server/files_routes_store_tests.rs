//! The stored-document routes against a real database.
//!
//! `files_routes_access_tests` proves the gate, and stops at an invalid key
//! before the store is reached; these prove the other half — that an allowed
//! caller's list, rename and delete are answered by the database, and that the
//! JSON index the daemon used to keep is not written any more.
//!
//! The store is installed on the state directly (`WebCanvasState::documents`)
//! rather than through `NORKA_DOCUMENTS_DIR`: a test that set the variable
//! would race every other test in this binary that resolves a directory, and
//! what these check is the wiring, not the resolver.

use super::*;
use crate::document_test_dir::TempDir;

fn body_json(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("{}: {error}", reply.body))
}

#[test]
fn an_allowed_caller_lists_and_renames_what_the_database_holds() {
    let dir = TempDir::new("routes-store");
    let store = dir.open();
    let entry = document_store::create_with(&store, Some("Before"), |path| {
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
