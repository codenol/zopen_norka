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
                    .update_acl(lease.owner_id(), lease.tenant(), DOCUMENT, change)
                    .expect("persisted");
            });
        }
    });

    let shared = lease.tenant().shared_with(DOCUMENT);
    assert_eq!(shared.len(), 16, "every grant must survive: {shared:?}");
    // …and the persisted list agrees with memory.
    assert_eq!(
        temp.registry
            .store()
            .load_acl("userA", DOCUMENT)
            .shared_with,
        shared
    );
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

