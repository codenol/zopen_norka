//! Sharing, the tenant query parameter, and eviction persistence, exercised
//! end to end through the online accept loop.
//!
//! Split out of `online_run_loop_tests.rs` at the 800-line cap; nested under
//! it so `use super::*` still reaches the request builder and helpers.

use super::*;
use crate::web_canvas_server::tenant_store::TenantStore;

/// The document every share in this file is about. One constant because the
/// key is now part of what a share means: a grant without one used to open
/// every document the account owned (issue #127).
const DOCUMENT: &str = "docA";

/// A registry with a real on-disk store rooted in a temp directory.
struct PersistentRegistry {
    root: std::path::PathBuf,
    registry: TenantRegistry,
}

impl PersistentRegistry {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "op-online-share-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        Self {
            registry: TenantRegistry::with_store(
                3102,
                TenantLimits {
                    idle_evict_secs: 1,
                    ..TenantLimits::default()
                },
                vec![PUBLIC_ORIGIN.to_string()],
                TenantStore::new(Some(root.clone())),
            ),
            root,
        }
    }
}

impl Drop for PersistentRegistry {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Address a request at another account's tenant.
fn as_tenant(mut request: Request, owner: &'static str) -> Request {
    request.tenant = Some(owner);
    // A request addressed at another account's tenant must also say WHICH
    // document: admission is a property of that document's access list
    // (issue #127).
    request.file = Some(DOCUMENT);
    request
}

fn share(
    registry: &TenantRegistry,
    token: &'static str,
    route: &'static str,
    target: &str,
) -> String {
    serve(
        registry,
        &verifier(),
        Request::json(
            "POST",
            route,
            &serde_json::json!({ "userId": target, "file": DOCUMENT }).to_string(),
        )
        .with_bearer(token),
    )
}

/// A visitor cannot change who else may open the document they were given.
///
/// The route family administers sharing on the CALLER's own tenant, so a
/// visitor's grant used to land on their own access list and be answered
/// `200 changed:true` — a success reported for a document they have no
/// authority over. Found by running the share scenario against a real
/// deployment (issue #120).
#[test]
fn a_visitor_cannot_change_who_else_may_open_the_document() {
    let registry = registry();
    share(
        &registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    let verifier = verifier();

    // Rows before: the owner's list names userB.
    let before = serve(
        &registry,
        &verifier,
        Request::new("GET", op_editor_core::share_routes::LIST)
            .with_bearer("tokA")
            .with_file(DOCUMENT),
    );

    for route in [
        op_editor_core::share_routes::GRANT,
        op_editor_core::share_routes::REVOKE,
    ] {
        let hostile = serve(
            &registry,
            &verifier,
            as_tenant(
                Request::json(
                    "POST",
                    route,
                    &serde_json::json!({ "userId": "userC" }).to_string(),
                )
                .with_bearer("tokB"),
                "userA",
            ),
        );
        assert_eq!(
            status_line(&hostile),
            "HTTP/1.1 403 Forbidden",
            "{route}: {hostile}"
        );
        assert_eq!(body(&hostile)["error"], "cannot-reshare", "{route}");
    }

    // And nothing moved: the owner's list is the one it was.
    let after = serve(
        &registry,
        &verifier,
        Request::new("GET", op_editor_core::share_routes::LIST)
            .with_bearer("tokA")
            .with_file(DOCUMENT),
    );
    assert_eq!(body(&before), body(&after));
}

#[test]
fn a_visitor_reaches_the_owner_document_only_after_a_grant() {
    let registry = registry();
    let verifier = verifier();
    // One document request against userA's tenant as userB — an account with no
    // editing role, which is what a development verifier sends for everyone.
    let visit = |method: &'static str, body: &str| {
        serve(
            &registry,
            &verifier,
            as_tenant(
                Request::json(method, "/api/mcp/document", body).with_bearer("tokB"),
                "userA",
            ),
        )
    };

    // Before the grant, addressing userA's tenant is refused.
    let refused = visit("GET", "");
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
    assert_eq!(body(&refused)["error"], "tenant-not-shared");

    // userA shares.
    let granted = share(
        &registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    assert_eq!(status_line(&granted), "HTTP/1.1 200 OK", "{granted}");

    // Now userB reads userA's document, and cannot write it: the grant answers
    // "may this caller reach the document", not "may this caller change it"
    // (#33 — this used to write). A visitor the hub DOES give an editing role
    // is proven through this accept loop in `online_mcp_tests`.
    let visited = visit("GET", "");
    assert_eq!(status_line(&visited), "HTTP/1.1 200 OK", "{visited}");
    assert_eq!(body(&visited)["version"], 0, "{visited}");
    let pushed = visit("POST", SYNC_BODY);
    assert_eq!(status_line(&pushed), "HTTP/1.1 403 Forbidden", "{pushed}");
    assert_eq!(body(&pushed)["error"], "read-only-role", "{pushed}");

    // userA's document is untouched, and userA sees it with no parameter.
    let owner_view = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert_eq!(body(&owner_view)["version"], 0, "{owner_view}");
    assert!(!owner_view.contains("Tenant Rect"), "{owner_view}");

    // userB's OWN document is untouched — the parameter addressed a tenant,
    // it did not move the visitor into it.
    let visitor_own = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokB"),
    );
    assert_eq!(body(&visitor_own)["version"], 0, "{visitor_own}");
}

#[test]
fn a_revoke_locks_the_visitor_out_again() {
    let registry = registry();
    share(
        &registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    let allowed = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(status_line(&allowed), "HTTP/1.1 200 OK");

    share(
        &registry,
        "tokA",
        op_editor_core::share_routes::REVOKE,
        "userB",
    );
    let refused = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
}

#[test]
fn a_tenant_parameter_naming_an_unshared_account_is_refused() {
    let registry = registry();
    for owner in ["userA", "nobody-at-all"] {
        let response = serve(
            &registry,
            &verifier(),
            Request {
                tenant: Some(Box::leak(owner.to_string().into_boxed_str())),
                ..Request::new("GET", "/api/mcp/document")
            }
            .with_bearer("tokB"),
        );
        assert_eq!(status_line(&response), "HTTP/1.1 403 Forbidden", "{owner}");
    }
}

#[test]
fn an_account_may_always_address_its_own_tenant_explicitly() {
    let registry = registry();
    let response = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokA"),
            "userA",
        ),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
}

#[test]
fn the_share_list_reports_both_directions_over_the_wire() {
    let registry = registry();
    share(
        &registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );

    let owner = serve(
        &registry,
        &verifier(),
        Request::new("GET", op_editor_core::share_routes::LIST)
            .with_bearer("tokA")
            .with_file(DOCUMENT),
    );
    // One entry per account, with the level it was given: the list answers
    // "what may they do", not merely "are they on it" (#56).
    assert_eq!(body(&owner)["sharedWith"][0]["account"], "userB", "{owner}");
    assert_eq!(body(&owner)["sharedWith"][0]["level"], "viewer", "{owner}");

    let visitor = serve(
        &registry,
        &verifier(),
        Request::new("GET", op_editor_core::share_routes::LIST)
            .with_bearer("tokB")
            .with_file(DOCUMENT),
    );
    assert_eq!(
        body(&visitor)["sharedWithMe"][0]["owner"],
        "userA",
        "{visitor}"
    );
    assert_eq!(
        body(&visitor)["sharedWithMe"][0]["level"],
        "viewer",
        "the visitor is told what they hold, not only whose document it is"
    );
}

#[test]
fn a_share_route_never_lets_a_visitor_change_the_owners_list() {
    // A `?tenant=` parameter pointing at somebody else's document used to make
    // the visitor's grant land on the VISITOR's own list while answering
    // `200 changed:true` — a success reported for a document they have no
    // authority over (#120). It is refused now.
    let registry = registry();
    share(
        &registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    let response = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::json(
                "POST",
                op_editor_core::share_routes::GRANT,
                r#"{"userId":"userC"}"#,
            )
            .with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "cannot-reshare", "{response}");

    // userC still cannot reach userA.
    let stranger = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokC"),
            "userA",
        ),
    );
    assert_eq!(
        status_line(&stranger),
        "HTTP/1.1 401 Unauthorized",
        "{stranger}"
    );
}

#[test]
fn an_evicted_tenant_is_written_and_read_back() {
    let temp = PersistentRegistry::new("roundtrip");
    let verifier = verifier();

    serve(
        &temp.registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    share(
        &temp.registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );

    assert_eq!(temp.registry.evict_idle(now_unix() + 3600), 1);
    assert_eq!(temp.registry.tenant_count(), 0);
    assert!(temp.registry.store().has_document("userA"));

    // The document comes back…
    let restored = serve(
        &temp.registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert!(restored.contains("Tenant Rect"), "{restored}");

    // …and so does the access list, so a share survives a reclaim.
    let visitor = serve(
        &temp.registry,
        &verifier,
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(status_line(&visitor), "HTTP/1.1 200 OK", "{visitor}");
}

#[test]
fn a_grant_is_persisted_immediately_rather_than_at_eviction() {
    // A share the user was told had succeeded must survive a restart, and the
    // document it applies to may not be written for another half hour.
    let temp = PersistentRegistry::new("acl-now");
    share(
        &temp.registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    assert!(
        temp.registry
            .store()
            .load_acl("userA", DOCUMENT)
            .shared_with
            .contains("userB"),
        "the grant must be on disk before any eviction"
    );
}

#[test]
fn a_corrupt_stored_document_yields_a_starter_and_keeps_the_bytes() {
    let temp = PersistentRegistry::new("corrupt");
    let verifier = verifier();
    serve(
        &temp.registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(temp.registry.evict_idle(now_unix() + 3600), 1);

    let dir = temp.registry.store().tenant_dir("userA").expect("dir");
    std::fs::write(dir.join("current.op"), b"not a document").expect("corrupt");

    let served = serve(
        &temp.registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert_eq!(status_line(&served), "HTTP/1.1 200 OK", "{served}");
    assert!(
        !served.contains("Tenant Rect"),
        "an unreadable document must yield a starter, not a failure: {served}"
    );
    let quarantined: Vec<String> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("corrupt"))
        .collect();
    assert_eq!(
        quarantined.len(),
        1,
        "the bytes must be kept, not overwritten"
    );
}

#[test]
fn an_account_id_full_of_traversal_cannot_escape_the_data_directory() {
    let temp = PersistentRegistry::new("traversal");
    let hostile = "../../../../etc/op-escape";
    let verifier = StaticVerifier::parse(&format!("tokX={hostile}"));

    serve(
        &temp.registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokX"),
    );
    assert_eq!(temp.registry.evict_idle(now_unix() + 3600), 1);

    let dir = temp.registry.store().tenant_dir(hostile).expect("dir");
    assert!(
        dir.starts_with(&temp.root),
        "{dir:?} escaped {:?}",
        temp.root
    );
    assert!(
        dir.join("current.op").is_file(),
        "the document still round-trips"
    );
    // Nothing was created outside the store.
    assert!(!std::path::Path::new("/etc/op-escape").exists());
}

#[test]
fn a_tenant_that_cannot_be_written_stays_resident_rather_than_losing_its_document() {
    let temp = PersistentRegistry::new("unwritable");
    let verifier = verifier();
    serve(
        &temp.registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    // Make the store root a file so `create_dir_all` cannot succeed.
    std::fs::remove_dir_all(&temp.root).expect("clear root");
    std::fs::write(&temp.root, b"not a directory").expect("block the root");

    assert_eq!(
        temp.registry.evict_idle(now_unix() + 3600),
        0,
        "reclaiming memory must not be worth discarding a document"
    );
    assert_eq!(temp.registry.tenant_count(), 1);
    let still_there = serve(
        &temp.registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert!(still_there.contains("Tenant Rect"), "{still_there}");

    let _ = std::fs::remove_file(&temp.root);
}

// The persistence lifecycle and the tenant-slot scenarios are `include!`d
// rather than moved into a `mod`, so every test keeps its exact path and
// its position in the registration order. The fragment is the original
// lines 515-691, byte-for-byte.
include!("online_share_tests_persistence.rs");

// The concurrent-edit, identity-projection and start-up-probe scenarios,
// same mechanism: the original lines 692-903, byte-for-byte.
include!("online_share_tests_identity.rs");

#[cfg(test)]
#[path = "online_shutdown_tests.rs"]
mod shutdown;
