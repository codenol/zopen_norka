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

