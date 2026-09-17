// ---------------------------------------------------------------------------
// The locked route table.
// ---------------------------------------------------------------------------

#[test]
fn saving_to_the_daemon_filesystem_is_forbidden() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::json("POST", "/api/file/save", "{}").with_bearer("tokA"),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "online-local-file-disabled");
}

#[test]
fn opening_a_recent_local_file_is_forbidden() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::json("POST", "/api/file/open-recent", r#"{"path":"/etc/passwd"}"#)
            .with_bearer("tokA"),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "online-local-file-disabled");
}

#[test]
fn the_root_json_rpc_alias_is_gone_and_only_slash_mcp_dispatches() {
    let registry = registry();
    let verifier = verifier();
    let aliased = serve(
        &registry,
        &verifier,
        Request::json(
            "POST",
            "/",
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .with_bearer("tokA"),
    );
    assert_eq!(
        status_line(&aliased),
        "HTTP/1.1 405 Method Not Allowed",
        "{aliased}"
    );

    let canonical = serve(
        &registry,
        &verifier,
        Request::json(
            "POST",
            "/mcp",
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .with_bearer("tokA"),
    );
    assert_eq!(status_line(&canonical), "HTTP/1.1 200 OK", "{canonical}");
}

#[test]
fn sync_reset_answers_without_touching_the_account_document() {
    let registry = registry();
    let verifier = verifier();
    serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );

    // The wasm shell posts this on every mount. Locally it resets; here it
    // must not, or a returning account loses the document it left behind.
    let reset = serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/sync-reset", "{}").with_bearer("tokA"),
    );
    assert_eq!(status_line(&reset), "HTTP/1.1 200 OK", "{reset}");
    assert_eq!(body(&reset)["ok"], true);
    assert_eq!(body(&reset)["skipped"], true);

    let after = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert_eq!(body(&after)["version"], 1, "the document survived: {after}");
    assert!(after.contains("Tenant Rect"), "{after}");
}

#[test]
fn the_agent_indicator_relay_is_empty() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", "/api/mcp/indicators").with_bearer("tokA"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK");
    let payload = body(&response);
    assert_eq!(payload["active"], false);
    assert_eq!(payload["nodes"].as_array().map(Vec::len), Some(0));
    assert_eq!(payload["frames"].as_array().map(Vec::len), Some(0));
    assert_eq!(payload["previews"].as_array().map(Vec::len), Some(0));
    // Still parseable by the browser mirror, so the shell simply paints none.
    assert!(op_editor_core::agent_indicators::parse_relay_json(
        response.split("\r\n\r\n").nth(1).unwrap_or_default()
    )
    .is_some());
}

#[test]
fn the_device_login_proxy_is_not_routed() {
    // `/api/auth/status` is deliberately EXCLUDED: it is the read-only account
    // projection the shell needs to detect an account switch, and it answers
    // from the connection's verified identity rather than from the daemon's
    // process-wide device session. Everything that drives that session stays
    // unreachable. See `the_sign_in_and_sign_out_routes_stay_unreachable_online`.
    let registry = registry();
    let verifier = verifier();
    for request in [
        Request::json("POST", op_editor_core::auth_routes::LOGOUT, "{}").with_bearer("tokA"),
        Request::json("POST", op_editor_core::auth_routes::LOGIN_BEGIN, "{}").with_bearer("tokA"),
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
fn collaboration_actions_that_reach_a_caller_named_address_are_forbidden() {
    let registry = registry();
    let verifier = verifier();
    for body_json in [
        r#"{"type":"startLan"}"#,
        r#"{"type":"beginDiscovery"}"#,
        r#"{"type":"joinDiscovered","discoveryId":"whatever"}"#,
        r#"{"type":"joinAddress","endpoint":"169.254.169.254:80"}"#,
    ] {
        let response = serve(
            &registry,
            &verifier,
            Request::json("POST", op_editor_core::collab_routes::ACTION, body_json)
                .with_bearer("tokA"),
        );
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 403 Forbidden",
            "{body_json}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "online-network-action-disabled",
            "{body_json}"
        );
    }
}

#[test]
fn collaboration_actions_that_stay_local_are_still_accepted() {
    // The panel is a pure projection online, but refusing every action would
    // be a different bug from refusing the network ones — the whitelist has
    // to have something on the allowed side.
    let response = serve(
        &registry(),
        &verifier(),
        Request::json(
            "POST",
            op_editor_core::collab_routes::ACTION,
            r#"{"type":"openCreate"}"#,
        )
        .with_bearer("tokA"),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 202 Accepted",
        "{response}"
    );
}

#[test]
fn collaboration_stays_unavailable_for_every_account() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", op_editor_core::collab_routes::STATE).with_bearer("tokA"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
    // No driver runs online, so nothing ever raises availability past its
    // default — relay sessions need a per-account device ticket the process
    // cannot mint. M4 replaces this with in-service sessions.
    assert_eq!(body(&response)["availability"], "unavailable", "{response}");
}

#[test]
fn a_settings_write_never_reaches_the_process_settings_file() {
    // The route still answers and the change still lands in this account's
    // in-memory editor; what must not happen is a write to the ONE settings
    // file the whole process shares, which would overwrite every other
    // account's providers and credentials with this account's.
    let registry = registry();
    let verifier = verifier();
    let lease = registry
        .lease_for(
            &verifier
                .resolve(&PresentedCredentials {
                    bearer: Some("tokA".into()),
                    session_cookie: None,
                })
                .expect("userA"),
        )
        .expect("lease");
    let mut guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(guard.mode, ServeMode::Online);

    let fingerprint = crate::settings_io::fingerprint(&guard.editor);
    let rollback = guard.editor.editor_ui.agent_settings.clone();
    guard.editor.editor_ui.agent_settings.mcp_server.port = 5123;
    let reply = persist_api_settings(
        "POST",
        "/api/settings/credentials",
        &mut guard,
        fingerprint,
        Some(rollback),
        WebReply {
            status: "200 OK",
            body: r#"{"ok":true}"#.into(),
        },
        |_| panic!("an online deployment must never write the process settings file"),
    );
    assert_eq!(reply.status, "200 OK");
    assert_eq!(
        guard.editor.editor_ui.agent_settings.mcp_server.port, 5123,
        "the change still lands in this account's own editor"
    );
}

#[test]
fn an_evicted_account_comes_back_to_a_fresh_starter_document() {
    let registry = TenantRegistry::new(
        3102,
        TenantLimits {
            idle_evict_secs: 1,
            ..TenantLimits::default()
        },
        vec![PUBLIC_ORIGIN.to_string()],
    );
    let verifier = verifier();
    serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );

    // Every connection released its lease when it finished, so the sweep can
    // reclaim the tenant.
    assert_eq!(registry.evict_idle(now_unix() + 3600), 1);

    let after = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    // M1 does not persist, so losing the document is the DOCUMENTED outcome
    // of eviction, not an accident. M4 loads it back from disk instead.
    assert_eq!(body(&after)["version"], 0, "{after}");
    assert!(!after.contains("Tenant Rect"), "{after}");
}

// ---------------------------------------------------------------------------
// Origin hardening: the CSRF boundary for cookie-authenticated writes.
// ---------------------------------------------------------------------------

/// The static verifier treats the same table as both cookies and tokens, so
/// `sessA` presented as a cookie resolves to `userA`.
fn cookie_verifier() -> StaticVerifier {
    StaticVerifier::parse("tokA=userA,sessA=userA,tokB=userB")
}

#[test]
fn a_cookie_authenticated_write_from_this_deployment_origin_is_allowed() {
    let response = serve(
        &registry(),
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session("sessA")
            .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
}

#[test]
fn a_cookie_authenticated_write_from_another_origin_is_refused() {
    // The browser attaches the session cookie to a cross-site POST all by
    // itself, so without this check any page on the internet could drive a
    // signed-in user's canvas.
    //
    // `http://canvas.example` is NOT in this list: the request's own host is
    // `canvas.example`, so that origin IS this deployment's page — the scheme
    // differs because a browser reaches a TLS-terminating proxy over `https`
    // while the daemon sees the forwarded host. It is asserted allowed below.
    for hostile in ["https://evil.example", "null"] {
        let response = serve(
            &registry(),
            &cookie_verifier(),
            Request::json("POST", "/api/mcp/document", SYNC_BODY)
                .with_session("sessA")
                .with_origin(hostile),
        );
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 403 Forbidden",
            "{hostile}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "cross-origin-write-forbidden",
            "{hostile}"
        );
    }
}

#[test]
fn a_cookie_authenticated_write_from_this_deployments_own_host_is_allowed() {
    // No allowlist entry, no environment variable: the Origin names the host
    // this request arrived at, and a page cannot forge that header. Requiring
    // an operator to name their own origin instead made a fresh online
    // deployment refuse every cookie write from its own editor — found by
    // running the share scenario against a real deployment.
    let registry = TenantRegistry::new(3102, TenantLimits::default(), Vec::new());
    let response = serve(
        &registry,
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session("sessA")
            .with_origin("http://canvas.example"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
}

#[test]
fn a_cookie_authenticated_write_with_no_origin_at_all_is_refused() {
    let response = serve(
        &registry(),
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_session("sessA"),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
}

#[test]
fn a_cookie_authenticated_read_is_not_subject_to_the_write_gate() {
    // A GET changes nothing, and the browser's own CORS rules already stop a
    // hostile page from reading the response.
    let response = serve(
        &registry(),
        &cookie_verifier(),
        Request::new("GET", "/api/mcp/document")
            .with_session("sessA")
            .with_origin("https://evil.example"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
}

#[test]
fn a_bearer_authenticated_write_is_exempt_from_the_origin_gate() {
    // A token is only ever attached by code that already holds it, so there
    // is no confused deputy to protect against — and an MCP client has no
    // Origin to send.
    let response = serve(
        &registry(),
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_bearer("tokA")
            .with_origin("https://evil.example"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
}

#[test]
fn a_deployment_with_no_configured_origin_still_refuses_a_stranger() {
    // No allowlist: this deployment's own page is admitted (its Origin names
    // the host the request arrived at), and a page somewhere else is not.
    let registry = TenantRegistry::new(3102, TenantLimits::default(), Vec::new());
    let stranger = serve(
        &registry,
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session("sessA")
            .with_origin("https://evil.example"),
    );
    assert_eq!(
        status_line(&stranger),
        "HTTP/1.1 403 Forbidden",
        "{stranger}"
    );
    assert_eq!(body(&stranger)["error"], "cross-origin-write-forbidden");
}

#[test]
fn the_allowed_origin_is_echoed_and_a_wildcard_is_never_sent() {
    let allowed = serve(
        &registry(),
        &verifier(),
        Request::new("GET", "/api/mcp/version")
            .with_bearer("tokA")
            .with_origin(PUBLIC_ORIGIN),
    );
    assert!(
        allowed.contains(&format!("Access-Control-Allow-Origin: {PUBLIC_ORIGIN}")),
        "{allowed}"
    );
    // Credentialed requests plus `*` is exactly the combination that lets any
    // page read another account's document.
    assert!(
        !allowed.contains("Access-Control-Allow-Origin: *"),
        "{allowed}"
    );
}

#[test]
fn a_disallowed_origin_gets_no_cors_header_at_all() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("GET", "/api/mcp/version")
            .with_bearer("tokA")
            .with_origin("https://evil.example"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
    assert!(
        !response.contains("Access-Control-Allow-Origin"),
        "omitting the header is what makes the browser withhold the body: {response}"
    );
}

