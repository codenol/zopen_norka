//! Sharing, the tenant query parameter, and eviction persistence, exercised
//! end to end through the online accept loop.
//!
//! Split out of `online_run_loop_tests.rs` at the 800-line cap; nested under
//! it so `use super::*` still reaches the request builder and helpers.

use super::*;
use crate::web_canvas_server::tenant_store::TenantStore;

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
            &serde_json::json!({ "userId": target }).to_string(),
        )
        .with_bearer(token),
    )
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
        Request::new("GET", op_editor_core::share_routes::LIST).with_bearer("tokA"),
    );
    assert_eq!(body(&owner)["sharedWith"][0], "userB", "{owner}");

    let visitor = serve(
        &registry,
        &verifier(),
        Request::new("GET", op_editor_core::share_routes::LIST).with_bearer("tokB"),
    );
    assert_eq!(body(&visitor)["sharedWithMe"][0], "userA", "{visitor}");
}

#[test]
fn a_share_route_always_administers_the_callers_own_tenant() {
    // Even with a `?tenant=` parameter pointing at the owner, a visitor's
    // grant lands on the visitor's own access list.
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
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");

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
        temp.registry.store().load_acl("userA").contains("userB"),
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

// ---------------------------------------------------------------------------
// The persistence lifecycle: sweep cadence, shutdown flush, fail-closed start.
// ---------------------------------------------------------------------------

#[test]
fn the_sweep_interval_tracks_the_idle_deadline_within_bounds() {
    use crate::web_canvas_server::online_run_loop::sweep_interval_secs;
    // A quarter of the deadline bounds how long past its timer a tenant can
    // linger — but the production default's quarter (450 s) exceeds the
    // ceiling, so it clamps.
    assert_eq!(sweep_interval_secs(1800), 300);
    assert_eq!(sweep_interval_secs(400), 100);
    // …with a ceiling, so a very long deadline still sweeps regularly…
    assert_eq!(sweep_interval_secs(86_400), 300);
    // …and a floor, so a short test deadline does not spin the thread.
    for tiny in [0, 1, 2, 3] {
        assert_eq!(sweep_interval_secs(tiny), 1, "{tiny}");
    }
}

#[test]
fn an_idle_account_is_reclaimed_without_any_new_connection() {
    // The regression: eviction used to run only when a connection arrived, so
    // the one state it exists for — an idle daemon — was the state it never
    // ran in, and nothing was ever written to disk.
    let temp = PersistentRegistry::new("no-traffic");
    serve(
        &temp.registry,
        &verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(temp.registry.tenant_count(), 1);

    // No further requests — exactly what the sweeper thread calls.
    assert_eq!(temp.registry.evict_idle(now_unix() + 3600), 1);
    assert_eq!(temp.registry.tenant_count(), 0);
    assert!(temp.registry.store().has_document("userA"));
}

#[test]
fn a_controlled_shutdown_flushes_every_resident_account() {
    // Without this, every account active at the moment of a deploy loses
    // whatever had not happened to be evicted.
    let temp = PersistentRegistry::new("flush");
    let verifier = verifier();
    for token in ["tokA", "tokB"] {
        serve(
            &temp.registry,
            &verifier,
            Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer(token),
        );
    }
    assert_eq!(temp.registry.tenant_count(), 2);
    assert!(!temp.registry.store().has_document("userA"));

    assert_eq!(temp.registry.flush_all(), 2);
    assert!(temp.registry.store().has_document("userA"));
    assert!(temp.registry.store().has_document("userB"));
    // Flushing does not evict: requests still draining must keep working.
    assert_eq!(temp.registry.tenant_count(), 2);
}

#[test]
fn flushing_a_deployment_that_persists_nothing_is_a_no_op() {
    let registry = registry();
    serve(
        &registry,
        &verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(registry.flush_all(), 0);
}

#[test]
fn a_deployment_that_evicts_but_persists_nothing_refuses_to_start() {
    use crate::web_canvas_server::online_run_loop::check_persistence_configured;

    // The dangerous default: eviction on, data directory unset. Starting here
    // means every idle account's document is destroyed on its first timer.
    let refused = check_persistence_configured(false, false, 1800).unwrap_err();
    let message = refused.to_string();
    assert!(message.contains("OPENPENCIL_ONLINE_DATA_DIR"), "{message}");
    assert!(message.contains("OPENPENCIL_ONLINE_EPHEMERAL"), "{message}");

    // Configured persistence is the normal deployment.
    assert!(check_persistence_configured(true, false, 1800).is_ok());
    // An explicit opt-in is the demo, and says so.
    assert!(check_persistence_configured(false, true, 1800).is_ok());
    // Both is fine — the data directory simply wins.
    assert!(check_persistence_configured(true, true, 1800).is_ok());
}

// ---------------------------------------------------------------------------
// H5: an unauthorised `?tenant=` must not cost a tenant slot.
// ---------------------------------------------------------------------------

#[test]
fn unauthorised_tenant_requests_never_materialise_a_tenant() {
    // The exhaustion this closes: `?tenant=` names an arbitrary account, and
    // creating the tenant to discover the caller is not on its list means
    // every refused request still spends the daemon's tenant budget.
    let registry = registry();
    let verifier = verifier();
    for index in 0..64 {
        let owner: &'static str = Box::leak(format!("victim-{index}").into_boxed_str());
        let response = serve(
            &registry,
            &verifier,
            Request {
                tenant: Some(owner),
                ..Request::new("GET", "/api/mcp/document")
            }
            .with_bearer("tokB"),
        );
        assert_eq!(status_line(&response), "HTTP/1.1 403 Forbidden", "{owner}");
    }
    assert_eq!(
        registry.tenant_count(),
        0,
        "a refused visitor must not leave a tenant behind"
    );
}

#[test]
fn an_authorised_visitor_still_materialises_an_offline_owners_tenant() {
    // The other half: admission-before-materialisation must not break the
    // case sharing exists for — opening a document whose owner is offline.
    let temp = PersistentRegistry::new("offline-owner");
    let verifier = verifier();
    share(
        &temp.registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    serve(
        &temp.registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(temp.registry.evict_idle(now_unix() + 3600), 1);
    assert_eq!(temp.registry.tenant_count(), 0);

    // The owner is gone from memory; the visitor is admitted from the
    // persisted list and the tenant is restored for them.
    let visited = serve(
        &temp.registry,
        &verifier,
        as_tenant(
            Request::new("GET", "/api/mcp/document").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(status_line(&visited), "HTTP/1.1 200 OK", "{visited}");
    assert!(visited.contains("Tenant Rect"), "{visited}");
}

#[test]
fn a_non_resident_tenant_admits_nobody_when_nothing_was_persisted() {
    // Fail-closed: with no store the share was never durable, so a visitor
    // cannot be admitted to a tenant that is not in memory.
    let registry = registry();
    let response = serve(
        &registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
}

// ---------------------------------------------------------------------------
// H6: concurrent access-list edits.
// ---------------------------------------------------------------------------

#[test]
fn concurrent_grants_all_survive() {
    // Snapshot-then-write let two grants each read the list before the
    // other's insert and write back a version missing it, silently dropping
    // one. The edit and its write now happen under one lock.
    let temp = std::sync::Arc::new(PersistentRegistry::new("concurrent-grants"));
    let identity = crate::web_canvas_server::tenant_auth::ResolvedIdentity {
        user_id: "userA".into(),
        username: "userA".into(),
        display_name: "userA".into(),
        roles: op_editor_core::access::RoleSet::empty(),
        via: crate::web_canvas_server::tenant_auth::IdentityVia::ApiToken,
        scopes: crate::mcp_serve::tool_profile::McpScopes::FULL,
    };
    let lease = temp.registry.lease_for(&identity).expect("lease");
    let tenant = std::sync::Arc::new(());
    let _ = tenant;

    std::thread::scope(|scope| {
        for index in 0..16 {
            let temp = std::sync::Arc::clone(&temp);
            let lease = &lease;
            scope.spawn(move || {
                let change = crate::web_canvas_server::tenant::AclChange::Grant {
                    account: format!("guest-{index}"),
                    level: op_editor_core::ShareLevel::Viewer,
                    invited_by: Some("userA".to_string()),
                };
                temp.registry
                    .update_acl(lease.owner_id(), lease.tenant(), change)
                    .expect("persisted");
            });
        }
    });

    let shared = lease.tenant().shared_with();
    assert_eq!(shared.len(), 16, "every grant must survive: {shared:?}");
    // …and the persisted list agrees with memory.
    assert_eq!(temp.registry.store().load_acl("userA"), shared);
}

#[test]
fn a_share_that_cannot_be_persisted_is_reported_and_rolled_back() {
    // Reporting 200 here told the user a share had succeeded that would
    // vanish on the next restart.
    let temp = PersistentRegistry::new("unpersistable-share");
    std::fs::remove_dir_all(&temp.root).expect("clear root");
    std::fs::write(&temp.root, b"not a directory").expect("block the root");

    let response = share(
        &temp.registry,
        "tokA",
        op_editor_core::share_routes::GRANT,
        "userB",
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 500 Internal Server Error",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "share-not-persisted");

    // Rolled back, so memory and disk agree and a retry starts from a known
    // state — the visitor is NOT quietly admitted in the meantime.
    let visitor = serve(
        &temp.registry,
        &verifier(),
        as_tenant(
            Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
            "userA",
        ),
    );
    assert_eq!(status_line(&visitor), "HTTP/1.1 403 Forbidden", "{visitor}");

    let _ = std::fs::remove_file(&temp.root);
}

// ---------------------------------------------------------------------------
// B2: the shell must be able to learn which account it is showing.
// ---------------------------------------------------------------------------

#[test]
fn the_online_status_route_projects_the_verified_identity() {
    // Without this the shell's identity epoch never fires, and an account
    // switch in one tab leaks the previous account's document.
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", op_editor_core::auth_routes::STATUS).with_bearer("tokA"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
    let payload = body(&response);
    assert_eq!(payload["signed_in"], true);
    assert_eq!(payload["available"], true);
    assert_eq!(payload["username"], "userA");
}

#[test]
fn two_accounts_see_their_own_identity_on_the_status_route() {
    let registry = registry();
    let verifier = verifier();
    for (token, account) in [("tokA", "userA"), ("tokB", "userB")] {
        let response = serve(
            &registry,
            &verifier,
            Request::new("GET", op_editor_core::auth_routes::STATUS).with_bearer(token),
        );
        assert_eq!(body(&response)["username"], account, "{token}");
    }
}

#[test]
fn the_sign_in_and_sign_out_routes_stay_unreachable_online() {
    // Only the read-only projection is exposed: the rest drive the
    // process-wide device session an online deployment must never share.
    let registry = registry();
    let verifier = verifier();
    for request in [
        Request::json("POST", op_editor_core::auth_routes::LOGIN_BEGIN, "{}").with_bearer("tokA"),
        Request::json("POST", op_editor_core::auth_routes::LOGOUT, "{}").with_bearer("tokA"),
        Request::new("GET", op_editor_core::auth_routes::LOGIN_STATUS).with_bearer("tokA"),
    ] {
        let path = request.path;
        let response = serve(&registry, &verifier, request);
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 404 Not Found",
            "{path}: {response}"
        );
    }
}

#[test]
fn an_unauthenticated_caller_cannot_reach_the_status_projection() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", op_editor_core::auth_routes::STATUS),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 401 Unauthorized",
        "{response}"
    );
}

// ---------------------------------------------------------------------------
// B3 residue: start-up probes and the shutdown drain.
// ---------------------------------------------------------------------------

#[test]
fn an_unwritable_data_directory_is_refused_at_start_up() {
    // The alternative is an eviction failing silently half an hour after
    // start, one account at a time.
    let root = std::env::temp_dir().join(format!("op-unwritable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::write(&root, b"not a directory").expect("block the root");

    let store = TenantStore::new(Some(root.clone()));
    let error = store.check_writable().expect_err("must refuse");
    let message = error.to_string();
    assert!(
        message.contains("10001"),
        "must name the likely cause: {message}"
    );

    let _ = std::fs::remove_file(&root);
}

#[test]
fn a_writable_data_directory_passes_the_probe() {
    let temp = PersistentRegistry::new("probe-ok");
    temp.registry.store().check_writable().expect("writable");
    // The probe cleans up after itself.
    assert!(
        !temp.root.join(".probe").join("write.ok").exists(),
        "the probe must not leave files behind"
    );
}

#[test]
fn a_disabled_store_passes_the_probe_trivially() {
    // Nothing to write, so nothing to prove.
    TenantStore::new(None)
        .check_writable()
        .expect("no store, no probe");
}

#[test]
fn the_status_projection_carries_a_stable_subject_distinct_from_the_handle() {
    // The shell partitions its storage by this, so it must be the same key the
    // tenant registry uses — not a renameable display handle.
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", op_editor_core::auth_routes::STATUS).with_bearer("tokA"),
    );
    let payload = body(&response);
    assert_eq!(payload["subject"], "userA");
    assert!(payload["username"].is_string());
}

#[cfg(test)]
#[path = "online_shutdown_tests.rs"]
mod shutdown;
