//! The stored-document routes on a public deployment, driven end to end.
//!
//! Split out of `online_run_loop_tests.rs` (which supplies the request builder
//! and the registry) for the 800-line cap.
//!
//! These are the tests that used to pin the OPPOSITE answer. An online
//! deployment refused `/api/files*` and `/api/recovery*` wholesale, in front of
//! the per-request access gate, because the document directory had no owner
//! dimension: a role check could say who may edit and never whose file it is
//! (#20). The owner column is what that refusal was waiting for, so what is
//! asserted here is the new answer — through the real accept loop, with the
//! real identity verifier, the real tenant lease and two accounts sharing one
//! directory.
//!
//! The three callers, in the order the routes meet them:
//!
//! - **the owner** lists, creates, opens, saves, renames and deletes their own
//!   documents, exactly as the local operator does;
//! - **a granted visitor** reads the owner's document by key and is refused
//!   every write their roles do not grant (`read-only-role`);
//! - **a stranger** — and any account naming a key that belongs to somebody
//!   else, or to nobody — gets `tenant-not-shared` for that document.
//!
//! ## Why the store is installed on the tenant
//!
//! `documents_dir()` is resolved from the environment the first time a store is
//! needed, and the registry keeps a resident tenant between requests. Taking a
//! lease here and putting a `TempDir`'s store on that state is therefore the
//! same state the next request is dispatched against — which is how this file
//! exercises a shared directory without touching the machine's own documents.

use super::*;
use crate::document_db::DocumentDb;
use crate::document_store::{self, DocumentStoreError};
use crate::document_test_dir::TempDir;

/// Give `tokens`' tenants one shared store, in `dir`.
///
/// One directory for both accounts is the deployment's actual shape, and the
/// reason every assertion below is about ownership rather than about paths.
fn install_shared_store(
    registry: &TenantRegistry,
    verifier: &StaticVerifier,
    dir: &TempDir,
    tokens: &[&str],
) -> DocumentDb {
    let store = dir.open();
    for token in tokens {
        let identity = verifier
            .resolve(&PresentedCredentials {
                bearer: Some((*token).into()),
                session_cookie: None,
            })
            .unwrap_or_else(|error| panic!("resolve {token}: {error}"));
        let lease = registry
            .lease_for(&identity)
            .unwrap_or_else(|error| panic!("lease {token}: {error}"));
        let mut state = lease.state().lock().unwrap_or_else(|p| p.into_inner());
        state.documents = Some(store.clone());
    }
    store
}

/// A document route for `key`.
///
/// Leaked on purpose: the request builder in the parent module takes a
/// `&'static str` path, which is what keeps every other request in these tests
/// a literal. A test-sized string per case is the cheaper trade.
fn key_path(key: &str, action: &str) -> &'static str {
    let path = if action.is_empty() {
        format!("/api/files/{key}")
    } else {
        format!("/api/files/{key}/{action}")
    };
    Box::leak(path.into_boxed_str())
}

/// A request addressed at `owner`'s tenant, as the browser's `?tenant=` does.
fn as_visitor(method: &'static str, path: &'static str, owner: &'static str) -> Request {
    let mut request = Request::new(method, path).with_bearer("tokB");
    request.tenant = Some(owner);
    request
}

fn keys_of(response: &str) -> Vec<String> {
    body(response)["files"]
        .as_array()
        .unwrap_or_else(|| panic!("no files array in {response}"))
        .iter()
        .filter_map(|file| file["key"].as_str().map(str::to_string))
        .collect()
}

/// Create one document as `tokA` and return its key.
fn create_as_owner(registry: &TenantRegistry, verifier: &StaticVerifier, name: &str) -> String {
    let created = serve(
        registry,
        verifier,
        Request::json("POST", "/api/files", &format!(r#"{{"name":"{name}"}}"#)).with_bearer("tokA"),
    );
    assert_eq!(status_line(&created), "HTTP/1.1 200 OK", "{created}");
    body(&created)["file"]["key"]
        .as_str()
        .unwrap_or_else(|| panic!("no key in {created}"))
        .to_string()
}

#[test]
fn the_owner_of_a_tenant_works_on_its_own_stored_documents() {
    // What the lifted refusal is FOR: a public deployment's file list, create,
    // open, save, rename and delete, for the account they belong to.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-owner");
    let store = install_shared_store(&registry, &verifier, &dir, &["tokA", "tokB"]);

    let key = create_as_owner(&registry, &verifier, "Online");

    // The creator is the owner, and the owner is the verified identity rather
    // than anything the request body could claim.
    let row = document_store::find(&store, &key)
        .expect("find")
        .expect("the row the create wrote");
    assert_eq!(row.owner_id.as_deref(), Some("userA"));
    assert_eq!(row.name, "Online");

    let listed = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/files").with_bearer("tokA"),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    assert_eq!(keys_of(&listed), vec![key.clone()]);

    let saved = serve(
        &registry,
        &verifier,
        Request::json("POST", key_path(&key, "save"), "").with_bearer("tokA"),
    );
    assert_eq!(status_line(&saved), "HTTP/1.1 200 OK", "{saved}");

    let renamed = serve(
        &registry,
        &verifier,
        Request::json("POST", key_path(&key, "rename"), r#"{"name":"Renamed"}"#)
            .with_bearer("tokA"),
    );
    assert_eq!(status_line(&renamed), "HTTP/1.1 200 OK", "{renamed}");
    assert_eq!(body(&renamed)["file"]["name"], "Renamed");

    // Opening it again comes back with the same document, under its own name.
    let opened = serve(
        &registry,
        &verifier,
        Request::json("POST", key_path(&key, "open"), "").with_bearer("tokA"),
    );
    assert_eq!(status_line(&opened), "HTTP/1.1 200 OK", "{opened}");
    assert_eq!(body(&opened)["name"], "Renamed");

    let deleted = serve(
        &registry,
        &verifier,
        Request::new("DELETE", key_path(&key, "")).with_bearer("tokA"),
    );
    assert_eq!(status_line(&deleted), "HTTP/1.1 200 OK", "{deleted}");
    assert!(document_store::list(&store).expect("list").is_empty());
}

#[test]
fn a_document_belonging_to_another_account_is_refused_by_key() {
    // The key is the whole request. userB holds a lease on their OWN workspace
    // and names userA's document, so nothing but the row's owner can refuse it
    // — which is exactly what the owner column is for.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-stranger");
    let store = install_shared_store(&registry, &verifier, &dir, &["tokA", "tokB"]);
    let key = create_as_owner(&registry, &verifier, "Private");

    for (method, path, payload) in [
        ("POST", key_path(&key, "open"), ""),
        ("POST", key_path(&key, "save"), "{}"),
        ("POST", key_path(&key, "autosave"), ""),
        ("POST", key_path(&key, "rename"), r#"{"name":"Stolen"}"#),
        ("GET", key_path(&key, "thumb"), ""),
        ("DELETE", key_path(&key, ""), ""),
    ] {
        let refused = serve(
            &registry,
            &verifier,
            Request::json(method, path, payload).with_bearer("tokB"),
        );
        assert_eq!(
            status_line(&refused),
            "HTTP/1.1 403 Forbidden",
            "{method} {path}: {refused}"
        );
        assert_eq!(
            body(&refused)["error"],
            "tenant-not-shared",
            "{method} {path}"
        );
    }

    // Neither does the answer hand over the document through a door that is
    // open by accident: the name is not in the refusal, and the stranger's own
    // list does not carry the row.
    let listed = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/files").with_bearer("tokB"),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    assert!(keys_of(&listed).is_empty(), "{listed}");
    assert!(!listed.contains("Private"), "{listed}");

    // And naming the owner's tenant does not step around it: the lease itself
    // is refused, with the same code, because it is the same statement.
    let mut request = Request::json("POST", key_path(&key, "open"), "");
    request.token = Some("tokB");
    request.tenant = Some("userA");
    let refused = serve(&registry, &verifier, request);
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
    assert_eq!(body(&refused)["error"], "tenant-not-shared");

    // The document is untouched by every one of those attempts.
    assert_eq!(
        document_store::find(&store, &key)
            .expect("find")
            .map(|entry| (entry.name, entry.owner_id)),
        Some(("Private".to_string(), Some("userA".to_string())))
    );
}

#[test]
fn a_granted_visitor_reads_the_shared_document_by_key_and_cannot_write_it() {
    // The share is what admits the visitor to the tenant at all, so this is the
    // case the owner column must NOT refuse: the key names a document of the
    // workspace the visitor was admitted to.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-guest");
    install_shared_store(&registry, &verifier, &dir, &["tokA", "tokB"]);
    let key = create_as_owner(&registry, &verifier, "Shared");

    let granted = serve(
        &registry,
        &verifier,
        Request::json(
            "POST",
            op_editor_core::share_routes::GRANT,
            &serde_json::json!({ "userId": "userB" }).to_string(),
        )
        .with_bearer("tokA"),
    );
    assert_eq!(status_line(&granted), "HTTP/1.1 200 OK", "{granted}");

    let opened = serve(&registry, &verifier, {
        let mut request = Request::json("POST", key_path(&key, "open"), "");
        request.token = Some("tokB");
        request.tenant = Some("userA");
        request
    });
    assert_eq!(status_line(&opened), "HTTP/1.1 200 OK", "{opened}");
    assert_eq!(
        body(&opened)["name"],
        "Shared",
        "a visitor with no roles still READS the document they were given"
    );

    // …and writes nothing. The static identity table has no roles to give, so
    // this is the empty-role answer the deployment floors at view-only.
    for (method, path, payload) in [
        ("POST", key_path(&key, "save"), "{}"),
        ("POST", key_path(&key, "autosave"), ""),
        ("POST", key_path(&key, "rename"), r#"{"name":"Mine now"}"#),
        ("DELETE", key_path(&key, ""), ""),
    ] {
        let mut request = Request::json(method, path, payload);
        request.token = Some("tokB");
        request.tenant = Some("userA");
        let refused = serve(&registry, &verifier, request);
        assert_eq!(
            status_line(&refused),
            "HTTP/1.1 403 Forbidden",
            "{method} {path}: {refused}"
        );
        assert_eq!(body(&refused)["error"], "read-only-role", "{method} {path}");
    }

    // The visitor's LIST is their own documents, not the owner's inventory: a
    // share admits them to the document its link names, and a list of every
    // name the owner has is a different thing to hand out.
    let listed = serve(
        &registry,
        &verifier,
        as_visitor("GET", "/api/files", "userA"),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    assert!(keys_of(&listed).is_empty(), "{listed}");
    assert!(!listed.contains("Shared"), "{listed}");
}

#[test]
fn a_document_no_account_owns_is_nobodys_online() {
    // Rows with a NULL owner are what the legacy `index.json` import brought
    // over and what a daemon without accounts made. Online there is no operator
    // to attribute them to, so the fail-closed answer is that no account can
    // list or reach them — not that the first account to name a key gets the
    // file.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-ownerless");
    let store = install_shared_store(&registry, &verifier, &dir, &["tokA"]);
    let orphan = document_store::create_with(&store, Some("Orphan"), None, |path| {
        std::fs::write(path, b"an older file")
            .map_err(|error| DocumentStoreError::Io(error.to_string()))
    })
    .expect("seed an ownerless row");

    let listed = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/files").with_bearer("tokA"),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    assert!(keys_of(&listed).is_empty(), "{listed}");
    assert!(!listed.contains("Orphan"), "{listed}");

    let refused = serve(
        &registry,
        &verifier,
        Request::json("POST", key_path(&orphan.key, "open"), "").with_bearer("tokA"),
    );
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
    assert_eq!(body(&refused)["error"], "tenant-not-shared");

    // It is still there, untouched, for the operator whose directory it is.
    assert_eq!(document_store::list(&store).expect("list").len(), 1);
}

#[test]
fn one_accounts_unsaved_draft_is_invisible_and_unrestorable_to_another() {
    // The draft is a whole document's unsaved work in one file, and it used to
    // be one file for the whole process — the same defect as the document
    // directory, and the reason this family was refused with it. It is keyed by
    // workspace now, so the answer is a per-account slot rather than a refusal.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-draft");
    install_shared_store(&registry, &verifier, &dir, &["tokA", "tokB"]);

    // autosave of an untitled document, through the route the shell uses.
    let written = serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/recovery", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(status_line(&written), "HTTP/1.1 200 OK", "{written}");

    let mine = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/recovery").with_bearer("tokA"),
    );
    assert_eq!(status_line(&mine), "HTTP/1.1 200 OK", "{mine}");
    assert_eq!(body(&mine)["exists"], true, "{mine}");

    let theirs = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/recovery").with_bearer("tokB"),
    );
    assert_eq!(status_line(&theirs), "HTTP/1.1 200 OK", "{theirs}");
    assert_eq!(
        body(&theirs)["exists"],
        false,
        "another account is not even told the draft exists: {theirs}"
    );

    let restored = serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/recovery/restore", "").with_bearer("tokB"),
    );
    assert_eq!(
        status_line(&restored),
        "HTTP/1.1 404 Not Found",
        "there is nothing of theirs to adopt: {restored}"
    );

    // And the document the stranger's editor holds is still their own starter,
    // not the work that was sitting in the other account's draft.
    let document = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokB"),
    );
    assert!(
        !document.contains("Tenant Rect"),
        "one account's unsaved work must never reach another's editor: {document}"
    );
}

#[test]
fn the_draft_a_visitor_writes_lands_in_the_workspace_they_were_granted() {
    // The draft of an untitled document belongs to the workspace the request is
    // served against — the same owner the access decision is made about — so a
    // granted visitor's autosave is the owner's draft, and the visitor's own
    // slot stays empty.
    let registry = registry();
    let verifier = verifier();
    let dir = TempDir::new("online-files-draft-guest");
    install_shared_store(&registry, &verifier, &dir, &["tokA", "tokB"]);
    let granted = serve(
        &registry,
        &verifier,
        Request::json(
            "POST",
            op_editor_core::share_routes::GRANT,
            &serde_json::json!({ "userId": "userB" }).to_string(),
        )
        .with_bearer("tokA"),
    );
    assert_eq!(status_line(&granted), "HTTP/1.1 200 OK", "{granted}");

    // A visitor with no roles may not write the owner's draft at all — the
    // write is refused, and refused BEFORE the slot is touched.
    let refused = serve(&registry, &verifier, {
        let mut request = Request::json("POST", "/api/recovery", SYNC_BODY);
        request.token = Some("tokB");
        request.tenant = Some("userA");
        request
    });
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
    assert_eq!(body(&refused)["error"], "read-only-role");

    // The visitor may still be ASKED about it — the bar is drawn for anyone who
    // may see the workspace — and the answer is the owner's slot, which is
    // empty.
    let asked = serve(
        &registry,
        &verifier,
        as_visitor("GET", "/api/recovery", "userA"),
    );
    assert_eq!(status_line(&asked), "HTTP/1.1 200 OK", "{asked}");
    assert_eq!(body(&asked)["exists"], false, "{asked}");
    assert!(
        !dir.join("recovery.op").exists(),
        "no account's draft is the local operator's slot"
    );
}
