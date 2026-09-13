//! End-to-end tests for the multi-account accept loop.
//!
//! These drive real requests through `serve_one_online`, so they cover the
//! whole path a public deployment exposes: header parse → identity → tenant →
//! route table. The isolation cases are the hard gate — a failure there means
//! one account can read another account's document and the AI credentials
//! sitting in its in-memory editor.

use super::*;

struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One request, as the wire sees it.
/// The public origin this test deployment answers for.
const PUBLIC_ORIGIN: &str = "https://canvas.example";

struct Request {
    method: &'static str,
    path: &'static str,
    body: String,
    token: Option<&'static str>,
    content_type: Option<&'static str>,
    cookie: Option<&'static str>,
    origin: Option<&'static str>,
    /// Addresses the request at another account's tenant, as the browser does
    /// with `?tenant=` on the page URL.
    tenant: Option<&'static str>,
}

impl Request {
    fn new(method: &'static str, path: &'static str) -> Self {
        Self {
            method,
            path,
            body: String::new(),
            token: None,
            content_type: None,
            cookie: None,
            origin: None,
            tenant: None,
        }
    }

    fn json(method: &'static str, path: &'static str, body: &str) -> Self {
        Self {
            body: body.to_string(),
            content_type: Some("application/json"),
            ..Self::new(method, path)
        }
    }

    fn with_bearer(mut self, token: &'static str) -> Self {
        self.token = Some(token);
        self
    }

    /// Present the deployment's session cookie, as a browser would.
    fn with_session(mut self, session: &'static str) -> Self {
        self.cookie = Some(session);
        self
    }

    fn with_origin(mut self, origin: &'static str) -> Self {
        self.origin = Some(origin);
        self
    }

    fn wire(&self) -> String {
        let auth = self
            .token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let content_type = self
            .content_type
            .map(|t| format!("Content-Type: {t}\r\n"))
            .unwrap_or_default();
        let cookie = self
            .cookie
            .map(|c| format!("Cookie: op_hub_session={c}\r\n"))
            .unwrap_or_default();
        let origin = self
            .origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default();
        let target = match self.tenant {
            Some(tenant) => format!("{}?tenant={tenant}", self.path),
            None => self.path.to_string(),
        };
        format!(
            "{} {target} HTTP/1.1\r\nHost: canvas.example\r\n{auth}{cookie}{origin}{content_type}\
             Content-Length: {}\r\n\r\n{}",
            self.method,
            self.body.len(),
            self.body
        )
    }
}

fn verifier() -> StaticVerifier {
    StaticVerifier::parse("tokA=userA,tokB=userB")
}

fn registry() -> TenantRegistry {
    TenantRegistry::new(
        3102,
        TenantLimits::default(),
        vec![PUBLIC_ORIGIN.to_string()],
    )
}

/// Drive one request through the online loop and return the raw response.
fn serve(registry: &TenantRegistry, verifier: &StaticVerifier, request: Request) -> String {
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    // An open barrier: the shutdown path is exercised by its own tests.
    let barrier = crate::web_canvas_server::tenant::WriteBarrier::default();
    serve_one_online(&mut stream, registry, verifier, &barrier).expect("serve_one_online");
    String::from_utf8_lossy(&stream.output).into_owned()
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or_default()
}

fn body(response: &str) -> serde_json::Value {
    let payload = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has a body");
    serde_json::from_str(payload).unwrap_or(serde_json::Value::Null)
}

/// A minimal canonical document, in the shape `POST /api/mcp/document` takes.
const SYNC_BODY: &str = r##"{"document":{"version":"1.0.0","children":[{"id":"n9","type":"rectangle","name":"Tenant Rect","x":1,"y":2,"width":80,"height":40,"fill":[{"type":"solid","color":"#123456"}]}]},"sourceClientId":"web"}"##;

// ---------------------------------------------------------------------------
// Isolation — the hard gate.
// ---------------------------------------------------------------------------

#[test]
fn one_account_document_write_is_invisible_to_another_account() {
    let registry = registry();
    let verifier = verifier();

    let pushed = serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(status_line(&pushed), "HTTP/1.1 200 OK", "{pushed}");

    let a = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    let b = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokB"),
    );

    assert_eq!(body(&a)["version"], 1);
    assert!(
        a.contains("Tenant Rect"),
        "the writer sees its own document: {a}"
    );
    assert_eq!(body(&b)["version"], 0, "{b}");
    assert!(
        !b.contains("Tenant Rect"),
        "one account's document must never appear in another's: {b}"
    );
}

#[test]
fn one_account_editor_credentials_are_invisible_to_another_account() {
    let registry = registry();
    let verifier = verifier();
    // Reach into A's tenant the way a credential write would, then ask B for
    // the models its own editor can serve. A shared editor would leak the
    // provider list — and the keys behind it — across accounts.
    {
        let lease = registry
            .lease_for(
                &verifier
                    .resolve(&PresentedCredentials {
                        bearer: Some("tokA".into()),
                        session_cookie: None,
                    })
                    .expect("userA"),
            )
            .expect("lease A");
        let mut guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
        guard.editor.editor_ui.agent_settings.mcp_server.port = 4242;
    }

    let lease_b = registry
        .lease_for(
            &verifier
                .resolve(&PresentedCredentials {
                    bearer: Some("tokB".into()),
                    session_cookie: None,
                })
                .expect("userB"),
        )
        .expect("lease B");
    let guard = lease_b.state().lock().unwrap_or_else(|p| p.into_inner());
    assert_ne!(
        guard.editor.editor_ui.agent_settings.mcp_server.port, 4242,
        "account settings, and the credentials beside them, must not be shared"
    );
}

#[test]
fn each_account_polls_its_own_version_counter() {
    let registry = registry();
    let verifier = verifier();
    serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    let a = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/version").with_bearer("tokA"),
    );
    let b = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
    );
    assert_eq!(body(&a)["version"], 1);
    assert_eq!(body(&b)["version"], 0);
}

// ---------------------------------------------------------------------------
// Authentication.
// ---------------------------------------------------------------------------

#[test]
fn a_request_with_no_credential_is_refused_before_any_tenant_exists() {
    let registry = registry();
    let response = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/document"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 401 Unauthorized");
    assert_eq!(
        registry.tenant_count(),
        0,
        "an unauthenticated request must not be able to make the daemon allocate a tenant"
    );
}

#[test]
fn an_unknown_token_is_refused_with_the_same_answer_as_a_missing_one() {
    let registry = registry();
    let missing = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/version"),
    );
    let unknown = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/version").with_bearer("tokZ"),
    );
    assert_eq!(status_line(&missing), status_line(&unknown));
    assert_eq!(body(&missing)["error"], body(&unknown)["error"]);
}

#[test]
fn the_static_bundle_is_reachable_without_any_credential() {
    // The host page has to load before the browser can present a session, so
    // the static tier answers anonymously. Which page it serves depends on
    // whether a wasm bundle is present in this checkout — what matters here
    // is that the static layer, not the 401 gate, is what answered.
    let response = serve(&registry(), &verifier(), Request::new("GET", "/"));
    assert!(
        response.contains("text/html"),
        "the static tier must answer `/` without a credential: {response}"
    );
    assert!(
        !response.contains("unauthorized"),
        "`/` must not be behind the identity gate: {response}"
    );
}

#[test]
fn a_cors_preflight_is_answered_without_a_credential() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("OPTIONS", "/api/mcp/document"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 204 No Content");
    assert!(response.contains("Authorization"), "{response}");
}

#[test]
fn a_deployment_with_no_verifier_answers_503_rather_than_serving_anyone() {
    let response = serve(
        &registry(),
        &StaticVerifier::parse(""),
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 503 Service Unavailable");
    assert_eq!(body(&response)["error"], "verifier-unavailable");
}

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
    for hostile in ["https://evil.example", "http://canvas.example", "null"] {
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
fn a_deployment_with_no_configured_origin_refuses_every_cookie_write() {
    let registry = TenantRegistry::new(3102, TenantLimits::default(), Vec::new());
    let response = serve(
        &registry,
        &cookie_verifier(),
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session("sessA")
            .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
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

#[cfg(test)]
#[path = "online_mcp_tests.rs"]
mod mcp_profile;

#[cfg(test)]
#[path = "online_share_tests.rs"]
mod share;

#[cfg(test)]
#[path = "online_files_tests.rs"]
mod files;
